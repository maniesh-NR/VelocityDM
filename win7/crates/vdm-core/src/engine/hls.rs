//! HLS (.m3u8) download support: master/media playlist parsing, variant
//! selection, AES-128-CBC segment decryption, `EXT-X-BYTERANGE` segments
//! (S11-i B3), concurrent ordered assembly (XDM-style "save streaming
//! video").

use crate::error::{Error, Result};
use crate::http::{ClientCfg, Conn, Request, Url};
use crate::token_bucket::TokenBucket;
use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes128;
use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub bandwidth: u64,
    pub resolution: Option<String>,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SegmentKey {
    pub method: String,
    pub uri: Option<String>,
    pub iv: Option<[u8; 16]>,
}

#[derive(Debug, Clone)]
pub struct Segment {
    /// Resolved absolute URI.
    pub uri: String,
    pub duration: f64,
    pub key: Option<SegmentKey>,
    /// Media sequence number (drives the default AES IV).
    pub seq: u64,
    /// `#EXT-X-BYTERANGE` sub-range of the URI's resource, as
    /// `(length, offset)`; `None` = the whole resource (S11-i B3).
    pub byterange: Option<(u64, u64)>,
}

#[derive(Debug, Clone)]
pub struct MediaPlaylist {
    pub target_duration: f64,
    pub media_sequence: u64,
    pub segments: Vec<Segment>,
    pub endlist: bool,
}

/// Parse a master playlist (variant streams).
pub fn parse_master(text: &str) -> Result<Vec<Variant>> {
    let mut variants = Vec::new();
    let mut pending_bandwidth = None;
    let mut pending_resolution = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("#EXT-X-STREAM-INF:") {
            let attrs = parse_attrs(line.trim_start_matches("#EXT-X-STREAM-INF:"));
            pending_bandwidth = attrs.get("BANDWIDTH").and_then(|b| b.parse::<u64>().ok());
            pending_resolution = attrs.get("RESOLUTION").cloned();
        } else if !line.is_empty() && !line.starts_with('#') {
            variants.push(Variant {
                bandwidth: pending_bandwidth.take().unwrap_or(0),
                resolution: pending_resolution.take(),
                uri: line.to_string(),
            });
        }
    }
    if variants.is_empty() {
        return Err(Error::Hls("master playlist has no variants".into()));
    }
    Ok(variants)
}

fn parse_hhmm_num(s: &str) -> Option<u64> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<u64>().ok()
    }
}

/// Parse `#EXT-X-BYTERANGE:<n>[@<o>]` per RFC 8216 §4.3.2.2. `n` and `o`
/// may be decimal or `0x` hexadecimal; a missing `o` is resolved against
/// the end of the previous sub-range applied to the same URI by
/// [`parse_media`].
fn parse_byterange_attr(rest: &str) -> Option<(u64, Option<u64>)> {
    let rest = rest.trim();
    let (len_s, off_s) = match rest.split_once('@') {
        Some((l, o)) => (l, Some(o)),
        None => (rest, None),
    };
    let len = parse_hhmm_num(len_s)?;
    let off = match off_s {
        Some(o) => Some(parse_hhmm_num(o)?),
        None => None,
    };
    Some((len, off))
}

/// Parse a media playlist; resolves relative URIs against `base`.
pub fn parse_media(text: &str, base: &Url) -> Result<MediaPlaylist> {
    let base_url = base.clone();
    let mut target_duration = 0.0;
    let mut media_sequence = 0u64;
    let mut endlist = false;
    let mut current_key: Option<SegmentKey> = None;
    let mut pending_duration = 0.0f64;
    // S11-i B3: a pending `#EXT-X-BYTERANGE` applies to the next media
    // URI line; `last_byterange_end` tracks the per-URI implicit-offset
    // cursor (end of the previous sub-range of the same resource).
    let mut pending_byterange: Option<(u64, Option<u64>)> = None;
    let mut last_byterange_end: HashMap<String, u64> = HashMap::new();
    let mut segments = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("#EXTINF:") {
            pending_duration = line
                .trim_start_matches("#EXTINF:")
                .split(',')
                .next()
                .unwrap_or("0")
                .trim()
                .parse()
                .unwrap_or(0.0);
        } else if line.starts_with("#EXT-X-KEY:") {
            let attrs = parse_attrs(line.trim_start_matches("#EXT-X-KEY:"));
            let method = attrs
                .get("METHOD")
                .cloned()
                .unwrap_or_else(|| "NONE".into());
            if method == "NONE" {
                current_key = None;
            } else {
                let uri = attrs.get("URI").cloned();
                let iv = attrs.get("IV").and_then(|v| parse_hex_iv(v));
                current_key = Some(SegmentKey { method, uri, iv });
            }
        } else if line.starts_with("#EXT-X-BYTERANGE:") {
            pending_byterange = parse_byterange_attr(line.trim_start_matches("#EXT-X-BYTERANGE:"));
            if pending_byterange.is_none() {
                return Err(Error::Hls(format!("bad #EXT-X-BYTERANGE '{}'", line)));
            }
        } else if line.starts_with("#EXT-X-MAP:") {
            // fMP4 init segments are S7 scope; fail loudly, never
            // mis-assemble an fMP4 stream as TS (spec non-goal).
            return Err(Error::Hls(
                "playlists using #EXT-X-MAP are not supported yet".into(),
            ));
        } else if line.starts_with("#EXT-X-TARGETDURATION:") {
            target_duration = line
                .trim_start_matches("#EXT-X-TARGETDURATION:")
                .trim()
                .parse()
                .unwrap_or(0.0);
        } else if line.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
            media_sequence = line
                .trim_start_matches("#EXT-X-MEDIA-SEQUENCE:")
                .trim()
                .parse()
                .unwrap_or(0);
        } else if line.starts_with("#EXT-X-ENDLIST") {
            endlist = true;
        } else if !line.starts_with('#') {
            let uri = base_url.join(line)?.to_string_full();
            // RFC 8216 §4.3.2.2: an absent offset continues the previous
            // sub-range of the SAME URI; a URI line without a BYTERANGE tag
            // addresses the whole resource and resets that URI's cursor.
            let byterange = match pending_byterange.take() {
                Some((len, off)) => {
                    let offset = match off {
                        Some(o) => o,
                        None => match last_byterange_end.get(&uri) {
                            Some(&end) => end,
                            None => {
                                return Err(Error::Hls(
                                    "#EXT-X-BYTERANGE without offset has no previous range for this URI"
                                        .into(),
                                ));
                            }
                        },
                    };
                    last_byterange_end.insert(uri.clone(), offset + len);
                    Some((len, offset))
                }
                None => {
                    last_byterange_end.remove(&uri);
                    None
                }
            };
            let seq = media_sequence + segments.len() as u64;
            segments.push(Segment {
                uri,
                duration: pending_duration,
                key: current_key.clone(),
                seq,
                byterange,
            });
            pending_duration = 0.0;
        }
    }
    if segments.is_empty() {
        return Err(Error::Hls("media playlist has no segments".into()));
    }
    Ok(MediaPlaylist {
        target_duration,
        media_sequence,
        segments,
        endlist,
    })
}

/// Pick a variant: "best" (default), "worst", or a bandwidth number.
pub fn choose_variant<'a>(variants: &'a [Variant], quality: &str) -> Result<&'a Variant> {
    let q = quality.trim().to_ascii_lowercase();
    if variants.is_empty() {
        return Err(Error::Hls("no variants".into()));
    }
    let pick = match q.as_str() {
        "" | "best" | "highest" => variants.iter().max_by_key(|v| v.bandwidth),
        "worst" | "lowest" => variants.iter().min_by_key(|v| v.bandwidth),
        other => {
            let want: u64 = other.parse().map_err(|_| {
                Error::Hls(format!(
                    "bad quality '{}' (use best|worst|bandwidth)",
                    quality
                ))
            })?;
            variants
                .iter()
                .min_by_key(|v| v.bandwidth.abs_diff(want))
                .or_else(|| variants.first())
        }
    };
    pick.ok_or_else(|| Error::Hls("no variants".into()))
}

fn parse_attrs(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        // key=value or key="quoted,value"
        let eq = match rest.find('=') {
            Some(i) => i,
            None => {
                out.insert(rest.trim().to_string(), String::new());
                break;
            }
        };
        let key = rest[..eq].trim().to_string();
        rest = &rest[eq + 1..];
        let value = if rest.starts_with('"') {
            match rest[1..].find('"') {
                Some(i) => {
                    let v = rest[1..1 + i].to_string();
                    rest = rest[1 + i + 1..].trim_start_matches(',');
                    v
                }
                None => {
                    let v = rest[1..].to_string();
                    rest = "";
                    v
                }
            }
        } else {
            match rest.find(',') {
                Some(i) => {
                    let v = rest[..i].trim().to_string();
                    rest = &rest[i + 1..];
                    v
                }
                None => {
                    let v = rest.trim().to_string();
                    rest = "";
                    v
                }
            }
        };
        out.insert(key, value);
    }
    out
}

fn parse_hex_iv(v: &str) -> Option<[u8; 16]> {
    let hexs = v.trim().trim_start_matches("0x").trim_start_matches("0X");
    let hexs = if hexs.len() % 2 == 1 {
        format!("0{}", hexs)
    } else {
        hexs.to_string()
    };
    let bytes = hex::decode(hexs).ok()?;
    if bytes.is_empty() || bytes.len() > 16 {
        return None;
    }
    let mut iv = [0u8; 16];
    iv[16 - bytes.len()..].copy_from_slice(&bytes);
    Some(iv)
}

/// Default IV: the segment media sequence number as a 16-byte big-endian int.
fn seq_iv(seq: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&seq.to_be_bytes());
    iv
}

/// AES-128-CBC decrypt + PKCS7 unpad (per-segment streams).
pub fn aes128_cbc_decrypt(key: &[u8; 16], iv: &[u8; 16], data: &mut Vec<u8>) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    if data.len() % 16 != 0 {
        return Err(Error::Hls(
            "encrypted segment length not multiple of 16".into(),
        ));
    }
    let cipher = Aes128::new(key.into());
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);
    for chunk in data.chunks_mut(16) {
        let mut block: aes::cipher::generic_array::GenericArray<u8, aes::cipher::typenum::U16> =
            aes::cipher::generic_array::GenericArray::clone_from_slice(chunk);
        let ciphertext = block.clone();
        cipher.decrypt_block(&mut block);
        for i in 0..16 {
            block[i] ^= prev[i];
        }
        prev.copy_from_slice(&ciphertext);
        chunk.copy_from_slice(&block);
    }
    // strip PKCS7 (strict: segments are padded per the HLS spec)
    if data.is_empty() {
        return Ok(());
    }
    let pad = *data.last().unwrap();
    if !(1..=16).contains(&pad) || pad as usize > data.len() {
        return Err(Error::Hls("invalid PKCS7 padding in segment".into()));
    }
    if !data[data.len() - pad as usize..].iter().all(|&b| b == pad) {
        return Err(Error::Hls("bad PKCS7 padding in segment".into()));
    }
    data.truncate(data.len() - pad as usize);
    Ok(())
}

pub fn fetch_text(cfg: &ClientCfg, url: &Url) -> Result<String> {
    let mut conn = Conn::connect(url, cfg)?;
    let mut resp = conn.request(&Request::get(url), cfg)?;
    if resp.status != 200 {
        return Err(Error::Hls(format!(
            "playlist fetch returned {}",
            resp.status
        )));
    }
    let body = resp.read_body(16 * 1024 * 1024)?;
    String::from_utf8(body).map_err(|_| Error::Hls("playlist is not utf-8".into()))
}

/// Resolve a playlist URL into the chosen media playlist (following master).
pub fn resolve_playlist(cfg: &ClientCfg, url: &Url, quality: &str) -> Result<(Url, MediaPlaylist)> {
    let text = fetch_text(cfg, url)?;
    let base = url.clone();
    if text.contains("#EXT-X-STREAM-INF") {
        let variants = parse_master(&text)?;
        let chosen = choose_variant(&variants, quality)?;
        let vurl = base.join(&chosen.uri)?;
        let vtext = fetch_text(cfg, &vurl)?;
        let media = parse_media(&vtext, &vurl)?;
        Ok((vurl, media))
    } else {
        let media = parse_media(&text, &base)?;
        if !media.endlist {
            return Err(Error::Hls(
                "live playlists without #EXT-X-ENDLIST are not supported yet".into(),
            ));
        }
        Ok((base.clone(), media))
    }
}

/// Run the HLS download: concurrent fetch, ordered assembly, AES-128 decrypt.
#[allow(clippy::too_many_arguments)]
pub fn download_hls(
    cfg: &ClientCfg,
    url: &Url,
    quality: &str,
    concurrency: usize,
    out: &Path,
    rate: Option<&Arc<TokenBucket>>,
    pause: &AtomicBool,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let (playlist_url, media) = resolve_playlist(cfg, url, quality)?;
    // fetch keys once
    let mut keys: HashMap<String, [u8; 16]> = HashMap::new();
    for seg in &media.segments {
        if let Some(k) = &seg.key {
            if let Some(uri) = &k.uri {
                if !keys.contains_key(uri) && k.method.eq_ignore_ascii_case("AES-128") {
                    let kurl = playlist_url.join(uri)?;
                    let mut conn = Conn::connect(&kurl, cfg)?;
                    let mut resp = conn.request(&Request::get(&kurl), cfg)?;
                    if resp.status != 200 {
                        return Err(Error::Hls(format!("key fetch returned {}", resp.status)));
                    }
                    let body = resp.read_body(64)?;
                    if body.len() != 16 {
                        return Err(Error::Hls("AES key must be 16 bytes".into()));
                    }
                    let mut key = [0u8; 16];
                    key.copy_from_slice(&body);
                    keys.insert(uri.clone(), key);
                }
            }
        }
    }

    let part_path = crate::engine::state::ResumeState::part_path(out);
    let mut writer = std::io::BufWriter::new(
        std::fs::File::create(&part_path)
            .map_err(|e| Error::Hls(format!("create part {}: {}", part_path.display(), e)))?,
    );
    let done_seg = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let next_idx = Arc::new(Mutex::new(0usize));

    let segments = Arc::new(media.segments);
    let keys = Arc::new(keys);
    // S11-i B3: when every segment carries a byte range the exact total is
    // known up-front; otherwise the first segment's length x count estimate
    // stands (the HLS path never asserts a known total — it estimates).
    let exact_total: Option<u64> = if segments.iter().all(|s| s.byterange.is_some()) {
        Some(segments.iter().map(|s| s.byterange.unwrap().0).sum())
    } else {
        None
    };
    let cfg = Arc::new(cfg.clone());
    let rate = rate.cloned();
    let (tx, rx) = std::sync::mpsc::channel::<(usize, Vec<u8>)>();
    let stop_c = Arc::new(AtomicBool::new(stop.load(Ordering::Relaxed)));

    let workers: Vec<_> = (0..concurrency.max(1))
        .map(|_| {
            let tx = tx.clone();
            let segments = segments.clone();
            let keys = keys.clone();
            let cfg = cfg.clone();
            let next_idx = next_idx.clone();
            let rate = rate.clone();
            let stop_c = stop_c.clone();
            let stop_flag = stop_c.clone();
            let pause = Arc::new(AtomicBool::new(pause.load(Ordering::Relaxed)));
            let playlist_url = playlist_url.clone();
            // keep-alive connection reused across segments of the same origin
            // (declared outside the loop so it survives between segments)
            let mut conn: Option<Conn> = None;
            let mut conn_target: Option<String> = None;
            std::thread::spawn(move || loop {
                if stop_c.load(Ordering::Relaxed) || stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                if pause.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    continue;
                }
                let idx = {
                    let mut n = next_idx.lock().unwrap();
                    let i = *n;
                    *n += 1;
                    i
                };
                if idx >= segments.len() {
                    return;
                }
                let seg = &segments[idx];
                let seg_url = match Url::parse(&seg.uri) {
                    Ok(u) => u,
                    Err(_) => match playlist_url.join(&seg.uri) {
                        Ok(u) => u,
                        Err(_) => return,
                    },
                };
                let data = match fetch_segment(
                    &cfg,
                    &seg_url,
                    rate.as_ref(),
                    &mut conn,
                    &mut conn_target,
                    seg.byterange,
                ) {
                    Ok(d) => d,
                    Err(_) => return, // worker exits; collector notices missing segment
                };
                let data = if let Some(k) = &seg.key {
                    if k.method.eq_ignore_ascii_case("AES-128") {
                        let key = k
                            .uri
                            .as_ref()
                            .and_then(|u| keys.get(u))
                            .copied()
                            .unwrap_or([0u8; 16]);
                        let iv = k.iv.unwrap_or_else(|| seq_iv(seg.seq));
                        let mut d = data;
                        if aes128_cbc_decrypt(&key, &iv, &mut d).is_err() {
                            return;
                        }
                        d
                    } else if k.method.eq_ignore_ascii_case("SAMPLE-AES") {
                        return; // unsupported
                    } else {
                        data
                    }
                } else {
                    data
                };
                if tx.send((idx, data)).is_err() {
                    return;
                }
            })
        })
        .collect();
    drop(tx);

    // ordered writer
    let mut pending: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut write_idx = 0usize;
    let mut total_bytes = 0u64;
    let mut failed = false;
    let mut written_sizes: Option<u64> = exact_total;
    for (idx, data) in rx {
        pending.insert(idx, data);
        while let Some(d) = pending.remove(&write_idx) {
            if writer.write_all(&d).is_err() {
                failed = true;
                break;
            }
            total_bytes += d.len() as u64;
            if written_sizes.is_none() {
                written_sizes = Some(d.len() as u64 * segments.len() as u64);
            }
            let done = done_seg.fetch_add(1, Ordering::Relaxed) + 1;
            let est = written_sizes.unwrap_or(0);
            progress(done.min(est), est.max(total_bytes));
            write_idx += 1;
        }
        if failed {
            break;
        }
    }
    for h in workers {
        let _ = h.join();
    }
    if failed || write_idx < segments.len() {
        return Err(Error::Hls(
            "segment download failed before completion".into(),
        ));
    }
    writer
        .flush()
        .map_err(|e| Error::Hls(format!("flush: {}", e)))?;
    drop(writer);
    std::fs::rename(&part_path, out).map_err(|e| {
        Error::Hls(format!(
            "rename {} -> {}: {}",
            part_path.display(),
            out.display(),
            e
        ))
    })?;
    Ok(())
}

/// Fetch one segment, reusing a keep-alive connection across segments
/// when the target host matches (persistent connections avoid a TCP/TLS
/// handshake per segment; see docs/ALGORITHMS.md "Research notes").
/// With a byte range (S11-i B3) the request carries
/// `Range: bytes=<offset>-<offset+len-1>`, expects `206 Partial Content`,
/// and fails on a short body — a sub-range of the resource, never a whole
/// resource read.
fn fetch_segment(
    cfg: &ClientCfg,
    url: &Url,
    rate: Option<&Arc<TokenBucket>>,
    conn: &mut Option<Conn>,
    conn_target: &mut Option<String>,
    byterange: Option<(u64, u64)>,
) -> Result<Vec<u8>> {
    let target = format!("{}:{}", url.host, url.port);
    if conn.is_some() && conn_target.as_deref() != Some(target.as_str()) {
        // connection is bound to a different origin; drop it
        *conn = None;
    }
    let mut request = Request::get(url);
    if let Some((len, offset)) = byterange {
        request = request.header("Range", &format!("bytes={}-{}", offset, offset + len - 1));
    }
    let mut resp = match conn.as_mut() {
        Some(c) => match c.request(&request, cfg) {
            Ok(r) => r,
            Err(_) => {
                // stale keep-alive: retry once on a fresh connection
                *conn = None;
                let mut fresh = Conn::connect(url, cfg)?;
                let r = fresh.request(&request, cfg)?;
                *conn = Some(fresh);
                r
            }
        },
        None => {
            let mut fresh = Conn::connect(url, cfg)?;
            let r = fresh.request(&request, cfg)?;
            *conn = Some(fresh);
            r
        }
    };
    *conn_target = Some(target);
    let want_status = if byterange.is_some() { 206 } else { 200 };
    if resp.status != want_status {
        let code = resp.status;
        let _ = resp.read_body(4096);
        *conn = resp.into_conn();
        return Err(Error::Hls(format!("segment returned {}", code)));
    }
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = {
            let mut br = resp.body_reader();
            Read::read(&mut br, &mut buf)?
        };
        if n == 0 {
            break;
        }
        if let Some(bucket) = rate {
            bucket.wait_for(n as u64);
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > 512 * 1024 * 1024 {
            return Err(Error::Hls("segment exceeds 512 MiB".into()));
        }
    }
    // a byte-range segment must be exactly the requested sub-range
    if let Some((len, _)) = byterange {
        if out.len() as u64 != len {
            *conn = None; // the body framing is untrusted; drop the conn
            return Err(Error::Hls(format!(
                "byte-range segment short body: wanted {} bytes, got {}",
                len,
                out.len()
            )));
        }
    }
    // cleanly drained body -> recycle for the next segment
    *conn = resp.into_conn();
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_parsing() {
        let text = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=1280x720\n720.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=640000\n360.m3u8\n";
        let v = parse_master(text).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].bandwidth, 1_280_000);
        assert_eq!(v[0].resolution.as_deref(), Some("1280x720"));
        assert_eq!(v[1].uri, "360.m3u8");
        assert!(parse_master("#EXTM3U\n").is_err());
    }

    #[test]
    fn variant_choice() {
        let v = parse_master("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100\na.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=900\nb.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=500\nc.m3u8\n").unwrap();
        assert_eq!(choose_variant(&v, "best").unwrap().uri, "b.m3u8");
        assert_eq!(choose_variant(&v, "worst").unwrap().uri, "a.m3u8");
        assert_eq!(choose_variant(&v, "").unwrap().uri, "b.m3u8");
        assert_eq!(choose_variant(&v, "550").unwrap().uri, "c.m3u8");
        assert!(choose_variant(&v, "abc").is_err());
    }

    #[test]
    fn media_parsing_with_key_and_iv() {
        let base = Url::parse("https://cdn.test/live/master/chunk.m3u8").unwrap();
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:5\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://cdn.test/key?sid=1,2\",IV=0x00112233445566778899aabbccddeeff\n#EXTINF:9.009,\nseg001.ts\n#EXTINF:9.009,\nseg002.ts\n#EXT-X-ENDLIST\n";
        let m = parse_media(text, &base).unwrap();
        assert_eq!(m.target_duration, 10.0);
        assert_eq!(m.media_sequence, 5);
        assert!(m.endlist);
        assert_eq!(m.segments.len(), 2);
        assert_eq!(m.segments[0].seq, 5);
        assert_eq!(m.segments[1].seq, 6);
        assert_eq!(m.segments[0].uri, "https://cdn.test/live/master/seg001.ts");
        let key = m.segments[0].key.as_ref().unwrap();
        assert_eq!(key.method, "AES-128");
        assert_eq!(key.uri.as_deref(), Some("https://cdn.test/key?sid=1,2"));
        assert_eq!(key.iv.unwrap()[0], 0x00);
        assert_eq!(key.iv.unwrap()[15], 0xff);
    }

    #[test]
    fn attrs_with_commas_in_quotes() {
        let a = parse_attrs("METHOD=AES-128,URI=\"https://k.test/a?b=1,2\",IV=0x1");
        assert_eq!(a.get("METHOD").unwrap(), "AES-128");
        assert_eq!(a.get("URI").unwrap(), "https://k.test/a?b=1,2");
        assert_eq!(a.get("IV").unwrap(), "0x1");
    }

    #[test]
    fn iv_parsing() {
        assert_eq!(
            parse_hex_iv("0x00112233445566778899aabbccddeeff").unwrap()[15],
            0xff
        );
        assert_eq!(parse_hex_iv("0xff").unwrap()[15], 0xff);
        assert_eq!(parse_hex_iv("ff").unwrap()[15], 0xff);
        assert!(parse_hex_iv("0x").is_none());
    }

    #[test]
    fn cbc_decrypt_roundtrip_against_reference() {
        // AES-128 key "0123456789abcdef", IV=0, plaintext "VelocityDM rulez!!" (17 bytes -> pad 15? no: 17 -> pad 15? PKCS7 pads to 32 with 15 bytes of 0x0f)
        // Reference: encrypt with our own CBC implementation inverted.
        use aes::cipher::BlockEncrypt;
        let key: [u8; 16] = *b"0123456789abcdef";
        let iv = [0u8; 16];
        let plain = b"VelocityDM test.".to_vec(); // 16 bytes -> padded to 32 with 16 x 0x10
        let mut padded = plain.clone();
        let pad = 16 - padded.len() % 16;
        padded.extend(std::iter::repeat_n(pad as u8, pad));
        // encrypt
        let cipher = Aes128::new(&key.into());
        let mut prev = iv;
        let mut enc = padded.clone();
        for chunk in enc.chunks_mut(16) {
            for i in 0..16 {
                chunk[i] ^= prev[i];
            }
            cipher.encrypt_block(aes::cipher::generic_array::GenericArray::from_mut_slice(
                chunk,
            ));
            prev.copy_from_slice(chunk);
        }
        let mut got = enc.clone();
        aes128_cbc_decrypt(&key, &iv, &mut got).unwrap();
        assert_eq!(got, plain);
        // tampered padding is rejected
        let mut bad = enc.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        assert!(aes128_cbc_decrypt(&key, &iv, &mut bad).is_err());
    }

    #[test]
    fn seq_iv_default() {
        assert_eq!(seq_iv(1)[15], 1);
        assert_eq!(seq_iv(0x0102030405060708)[8], 1);
    }

    // ---- S11-i B3: EXT-X-BYTERANGE ----

    fn byterange_media(playlist: &str) -> Result<MediaPlaylist> {
        let base = Url::parse("https://cdn.test/live/media.m3u8").unwrap();
        parse_media(playlist, &base)
    }

    #[test]
    fn byterange_explicit_and_implicit_offsets() {
        let m = byterange_media(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:1000@0\nmedia.ts\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:500\nmedia.ts\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:250@2000\nmedia.ts\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:100@5\nother.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        assert_eq!(m.segments.len(), 4);
        assert_eq!(m.segments[0].byterange, Some((1000, 0)));
        // implicit offset = end of the previous range of the SAME URI
        assert_eq!(m.segments[1].byterange, Some((500, 1000)));
        assert_eq!(m.segments[2].byterange, Some((250, 2000)));
        // a different URI has no cursor of its own: explicit offset required
        assert_eq!(m.segments[3].byterange, Some((100, 5)));
    }

    #[test]
    fn byterange_implicit_without_history_fails() {
        let err = byterange_media(
            "#EXTM3U\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:500\nmedia.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("no previous range"));
    }

    #[test]
    fn byterange_reset_after_whole_resource_segment() {
        // a URI line without a BYTERANGE tag = the whole resource and the
        // per-URI cursor resets
        let m = byterange_media(
            "#EXTM3U\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:1000@0\nmedia.ts\n#EXTINF:5.0,\nmedia.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        assert_eq!(m.segments[0].byterange, Some((1000, 0)));
        assert_eq!(m.segments[1].byterange, None);
        // and an implicit byterange after the reset has no cursor again
        let err = byterange_media(
            "#EXTM3U\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:1000@0\nmedia.ts\n#EXTINF:5.0,\nmedia.ts\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:10\nmedia.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("no previous range"));
    }

    #[test]
    fn byterange_hex_and_bad_attrs() {
        let m = byterange_media(
            "#EXTM3U\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:0x10@0x20\nmedia.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        assert_eq!(m.segments[0].byterange, Some((16, 32)));
        assert!(byterange_media(
            "#EXTM3U\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:abc\nmedia.ts\n#EXT-X-ENDLIST\n"
        )
        .is_err());
    }

    #[test]
    fn byterange_composes_with_aes_key() {
        let m = byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"/key.bin\",IV=0xdead\n#EXTINF:5.0,\n#EXT-X-BYTERANGE:512@0\nmedia.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        let seg = &m.segments[0];
        assert_eq!(seg.byterange, Some((512, 0)));
        assert_eq!(seg.key.as_ref().unwrap().method, "AES-128");
        // short IVs are right-aligned in the 16-byte array
        assert_eq!(seg.key.as_ref().unwrap().iv.unwrap()[14], 0xde);
        assert_eq!(seg.key.as_ref().unwrap().iv.unwrap()[15], 0xad);
    }

    #[test]
    fn map_playlist_fails_loudly() {
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("EXT-X-MAP"));
    }
}
