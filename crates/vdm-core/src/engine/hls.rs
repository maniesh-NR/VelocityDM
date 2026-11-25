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

/// `#EXT-X-MAP` init-section state (S7): the resolved URI, the resolved
/// BYTERANGE, and the EXT-X-KEY snapshot taken AT the MAP line (per RFC
/// 8216 §4.3.2.5 the key state at the MAP applies to the init section —
/// not the final playlist key).
#[derive(Debug, Clone, PartialEq)]
pub struct MapInfo {
    /// Resolved absolute URI.
    pub uri: String,
    /// Resolved `(length, offset)` sub-range; `None` = whole resource.
    pub byterange: Option<(u64, u64)>,
    /// EXT-X-KEY state at the MAP line.
    pub key: Option<SegmentKey>,
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
    /// The `#EXT-X-MAP` active at this segment's position (S7); the init
    /// bytes are written before the first segment of each distinct
    /// `(uri, byterange)` group.
    pub map: Option<MapInfo>,
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
///
/// S7 hardening (spec `S7-HLS-FMP4-SAMPLEAES.md` §2b–§2d): the METHOD
/// allowlist is {NONE, AES-128, SAMPLE-AES} (case-insensitive); a non-NONE
/// METHOD requires a present non-empty URI (RFC 8216 §4.3.2.4); an `IV`
/// attribute that is PRESENT but invalid loud-fails (the silent
/// seq-IV-fallback is gone); SAMPLE-AES accepts only an absent or
/// `identity` KEYFORMAT; `#EXT-X-MAP` composes (init sections) but never
/// with SAMPLE-AES.
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
    // S7: the pending `#EXT-X-MAP` applies to the media segments that
    // follow it, until a new MAP tag appears; the key state is snapshotted
    // AT the MAP line. The init implicit-offset cursor chains from the end
    // of the previous (ranged) init section.
    let mut current_map: Option<MapInfo> = None;
    let mut last_init_end: Option<u64> = None;
    let mut saw_map = false;
    let mut saw_sample_aes = false;
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
            if method.eq_ignore_ascii_case("NONE") {
                current_key = None;
            } else {
                let m = method.to_ascii_uppercase();
                if m != "AES-128" && m != "SAMPLE-AES" {
                    return Err(Error::Hls(format!(
                        "unsupported EXT-X-KEY METHOD '{}'",
                        sanitize_error_detail(&method)
                    )));
                }
                let uri = match attrs.get("URI") {
                    Some(u) if !u.trim().is_empty() => u.clone(),
                    _ => {
                        return Err(Error::Hls(format!(
                            "EXT-X-KEY METHOD {} requires a URI",
                            sanitize_error_detail(&method)
                        )))
                    }
                };
                // An IV that is present but invalid loud-fails; only a
                // truly absent IV falls back to the media-sequence IV.
                let iv = match attrs.get("IV") {
                    Some(v) => match parse_hex_iv(v) {
                        Some(iv) => Some(iv),
                        None => {
                            return Err(Error::Hls(
                                "EXT-X-KEY carries an invalid IV attribute".into(),
                            ))
                        }
                    },
                    None => None,
                };
                if m == "SAMPLE-AES" {
                    match attrs.get("KEYFORMAT") {
                        None => {}
                        Some(f) if f.eq_ignore_ascii_case("identity") => {}
                        Some(f) => {
                            return Err(Error::Hls(format!(
                                "SAMPLE-AES stream uses unsupported keyformat {}",
                                sanitize_error_detail(f)
                            )))
                        }
                    }
                    saw_sample_aes = true;
                }
                current_key = Some(SegmentKey {
                    method: m,
                    uri: Some(uri),
                    iv,
                });
            }
        } else if line.starts_with("#EXT-X-BYTERANGE:") {
            pending_byterange = parse_byterange_attr(line.trim_start_matches("#EXT-X-BYTERANGE:"));
            if pending_byterange.is_none() {
                return Err(Error::Hls(format!(
                    "bad #EXT-X-BYTERANGE '{}'",
                    sanitize_error_detail(line)
                )));
            }
        } else if line.starts_with("#EXT-X-MAP:") {
            saw_map = true;
            let attrs = parse_attrs(line.trim_start_matches("#EXT-X-MAP:"));
            let uri = match attrs.get("URI") {
                Some(u) if !u.trim().is_empty() => u.trim().to_string(),
                _ => return Err(Error::Hls("#EXT-X-MAP requires a non-empty URI".into())),
            };
            let byterange = match attrs.get("BYTERANGE") {
                Some(br) => {
                    let (len, off) = parse_byterange_attr(br).ok_or_else(|| {
                        Error::Hls(format!(
                            "bad #EXT-X-MAP BYTERANGE '{}'",
                            sanitize_error_detail(br)
                        ))
                    })?;
                    if len == 0 {
                        return Err(Error::Hls(
                            "#EXT-X-MAP BYTERANGE length is 0 (absurd byterange)".into(),
                        ));
                    }
                    if len > 512 * 1024 * 1024 {
                        return Err(Error::Hls("#EXT-X-MAP BYTERANGE exceeds 512 MiB".into()));
                    }
                    let offset = match off {
                        Some(o) => o,
                        None => match last_init_end {
                            // The FIRST MAP in the playlist defaults to
                            // offset 0 (rfc8216bis / Apple authoring-spec
                            // interop choice, declared in the S7 spec); a
                            // subsequent offsetless MAP chains from the
                            // end of the previous init section.
                            Some(end) => end,
                            None if current_map.is_none() => 0,
                            None => {
                                return Err(Error::Hls(
                                    "#EXT-X-MAP BYTERANGE without offset has no previous init to chain from"
                                        .into(),
                                ))
                            }
                        },
                    };
                    let end = offset
                        .checked_add(len)
                        .ok_or_else(|| Error::Hls("absurd #EXT-X-MAP byterange".into()))?;
                    last_init_end = Some(end);
                    Some((len, offset))
                }
                None => None,
            };
            // RFC 8216 §4.3.2.5: AES-128-encrypted init sections REQUIRE
            // the IV attribute (a MAP has no unambiguous media sequence
            // number, so the seq-IV fallback must not apply).
            if let Some(k) = &current_key {
                if k.method.eq_ignore_ascii_case("AES-128") && k.iv.is_none() {
                    return Err(Error::Hls(
                        "AES-128 encrypted #EXT-X-MAP requires an explicit IV".into(),
                    ));
                }
            }
            let uri = base_url.join(&uri)?.to_string_full();
            current_map = Some(MapInfo {
                uri,
                byterange,
                key: current_key.clone(),
            });
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
                                ))
                            }
                        },
                    };
                    let end = offset
                        .checked_add(len)
                        .ok_or_else(|| Error::Hls("absurd #EXT-X-BYTERANGE".into()))?;
                    last_byterange_end.insert(uri.clone(), end);
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
                map: current_map.clone(),
            });
            pending_duration = 0.0;
        }
    }
    if saw_map && saw_sample_aes {
        return Err(Error::Hls("SAMPLE-AES fMP4 is not supported".into()));
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

/// Sanitize a playlist-derived string before it reaches an error message
/// (S7 §2d): C0/C1/DEL controls map to spaces (the B20 engine mapping),
/// and the render length is byte-capped. Callers never interpolate
/// key/init URIs into error strings (auth-token leakage).
fn sanitize_error_detail(s: &str) -> String {
    let mut out = String::new();
    let mut budget = 256usize;
    for c in s.chars() {
        if budget == 0 {
            break;
        }
        let n = c as u32;
        let c = if n < 0x20 || n == 0x7F || (0x80..=0x9F).contains(&n) {
            ' '
        } else {
            c
        };
        let w = c.len_utf8();
        if w > budget {
            break;
        }
        out.push(c);
        budget -= w;
    }
    out
}

/// Raw AES-128-CBC decrypt of a whole multiple-of-16 region: NO PKCS7
/// unpadding, caller-supplied IV, in place. The SAMPLE-AES sub-sample
/// primitive per the S7 spec — `aes128_cbc_decrypt`'s strict padding
/// check would reject arbitrary audio bytes. Contract: `data.len()` is a
/// multiple of 16 by construction (callers compute `floor(x / 16) * 16`).
fn cbc_decrypt_raw(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8]) {
    if data.is_empty() {
        return;
    }
    let cipher = Aes128::new(key.into());
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);
    for chunk in data.chunks_mut(16) {
        // written Copy-free so the win7 mirror stays byte-identical here
        // (the only permitted hls.rs divergences remain the two existing
        // lines + the one authorized test-only line)
        let mut saved = [0u8; 16];
        saved.copy_from_slice(chunk);
        let mut block: aes::cipher::generic_array::GenericArray<u8, aes::cipher::typenum::U16> =
            aes::cipher::generic_array::GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        for i in 0..16 {
            block[i] ^= prev[i];
        }
        prev.copy_from_slice(&saved);
        chunk.copy_from_slice(&block);
    }
}

/// AES-128-CBC decrypt + PKCS7 unpad (per-segment streams).
pub fn aes128_cbc_decrypt(key: &[u8; 16], iv: &[u8; 16], data: &mut Vec<u8>) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    if !data.len().is_multiple_of(16) {
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
        let ciphertext = block;
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

/// Run the HLS download: concurrent fetch, ordered assembly, AES-128 and
/// SAMPLE-AES (AAC) decryption, `#EXT-X-MAP` init sections (S7).
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

    // S7 init identities first: every DISTINCT init section (by resolved
    // `(uri, byterange)` identity) is fetched before the workers spawn —
    // required for ordering and exact-total accounting. Ranged init
    // fetches ride the wave-3 B17 identity discipline
    // (`Accept-Encoding: identity`) [R2-5]; media fetches are unchanged.
    let mut init_order: Vec<MapInfo> = Vec::new();
    for seg in &media.segments {
        if let Some(m) = &seg.map {
            if !init_order
                .iter()
                .any(|i| i.uri == m.uri && i.byterange == m.byterange)
            {
                init_order.push(m.clone());
            }
        }
    }
    /// A resolved init identity: `(uri, byterange)`.
    type InitId = (String, Option<(u64, u64)>);
    let mut inits: HashMap<InitId, Vec<u8>> = HashMap::new();
    {
        let mut init_conn: Option<Conn> = None;
        let mut init_target: Option<String> = None;
        for m in &init_order {
            let iurl = match Url::parse(&m.uri) {
                Ok(u) => u,
                Err(_) => playlist_url
                    .join(&m.uri)
                    .map_err(|e| Error::Hls(format!("bad init URI: {}", e)))?,
            };
            let ranged = m.byterange.is_some();
            let bytes = fetch_segment(
                cfg,
                &iurl,
                rate,
                &mut init_conn,
                &mut init_target,
                m.byterange,
                ranged,
            )?;
            inits.insert((m.uri.clone(), m.byterange), bytes);
        }
    }

    // fetch keys once — AES-128, SAMPLE-AES, and MAP-captured keys share
    // one cache with the same 16-byte bound (S7 §2c). MAP snapshots are
    // walked explicitly: a key replaced by METHOD=NONE before the first
    // segment is still live for the init section it preceded.
    let mut keys: HashMap<String, [u8; 16]> = HashMap::new();
    {
        let mut want: Vec<String> = Vec::new();
        for seg in &media.segments {
            if let Some(k) = &seg.key {
                if let Some(uri) = &k.uri {
                    if !want.contains(uri) {
                        want.push(uri.clone());
                    }
                }
            }
        }
        for m in &init_order {
            if let Some(k) = &m.key {
                if let Some(uri) = &k.uri {
                    if !want.contains(uri) {
                        want.push(uri.clone());
                    }
                }
            }
        }
        for uri in &want {
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
    // AES-128 init decryption (RFC 8216 §4.3.2.5: the key state at the
    // MAP applies; the IV attribute is required — parse-enforced, and
    // re-checked here so the code cannot silently regress).
    for m in &init_order {
        if let Some(k) = &m.key {
            if k.method.eq_ignore_ascii_case("AES-128") {
                let key = k
                    .uri
                    .as_ref()
                    .and_then(|u| keys.get(u))
                    .copied()
                    .ok_or_else(|| Error::Hls("AES-128 key missing for #EXT-X-MAP".into()))?;
                let iv = k.iv.ok_or_else(|| {
                    Error::Hls("AES-128 encrypted #EXT-X-MAP requires an explicit IV".into())
                })?;
                let id = (m.uri.clone(), m.byterange);
                if let Some(bytes) = inits.get_mut(&id) {
                    aes128_cbc_decrypt(&key, &iv, bytes)?;
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
    // S11-i B3 + S7: the exact total is known when every media segment
    // AND every distinct init section carries a byte range; an unranged
    // init (whole-resource MAP URI) degrades the total to an estimate.
    let exact_total: Option<u64> = if segments.iter().all(|s| s.byterange.is_some())
        && init_order.iter().all(|m| m.byterange.is_some())
    {
        Some(
            segments.iter().map(|s| s.byterange.unwrap().0).sum::<u64>()
                + init_order
                    .iter()
                    .map(|m| m.byterange.unwrap().0)
                    .sum::<u64>(),
        )
    } else {
        None
    };
    let cfg = Arc::new(cfg.clone());
    let rate = rate.cloned();
    let (tx, rx) = std::sync::mpsc::channel::<(usize, std::result::Result<Vec<u8>, String>)>();
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
                    false,
                ) {
                    Ok(d) => d,
                    // S7 §2d: workers propagate a classified reason over
                    // the (idx, Result) channel; the collector surfaces
                    // the FIRST reason instead of the generic message.
                    Err(e) => {
                        let _ = tx.send((idx, Err(e.to_string())));
                        return;
                    }
                };
                let data = if let Some(k) = &seg.key {
                    if k.method.eq_ignore_ascii_case("AES-128") {
                        let key = match k.uri.as_ref().and_then(|u| keys.get(u)).copied() {
                            Some(key) => key,
                            None => {
                                // the zero-key fallback is gone (S7 §2c)
                                let _ = tx.send((idx, Err("AES-128 key missing".to_string())));
                                return;
                            }
                        };
                        let iv = k.iv.unwrap_or_else(|| seq_iv(seg.seq));
                        let mut d = data;
                        if let Err(e) = aes128_cbc_decrypt(&key, &iv, &mut d) {
                            let _ = tx.send((idx, Err(e.to_string())));
                            return;
                        }
                        d
                    } else if k.method.eq_ignore_ascii_case("SAMPLE-AES") {
                        let key = match k.uri.as_ref().and_then(|u| keys.get(u)).copied() {
                            Some(key) => key,
                            None => {
                                let _ = tx.send((idx, Err("SAMPLE-AES key missing".to_string())));
                                return;
                            }
                        };
                        let iv = k.iv.unwrap_or_else(|| seq_iv(seg.seq));
                        let mut d = data;
                        if let Err(e) = decrypt_sample_aes_aac(&key, &iv, &mut d) {
                            let _ = tx.send((idx, Err(e.to_string())));
                            return;
                        }
                        d
                    } else {
                        data
                    }
                } else {
                    data
                };
                if tx.send((idx, Ok(data))).is_err() {
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
    let mut first_err: Option<String> = None;
    let mut written_sizes: Option<u64> = exact_total;
    let mut last_map: Option<(String, Option<(u64, u64)>)> = None;
    for (idx, res) in rx {
        match res {
            Ok(data) => {
                pending.insert(idx, data);
            }
            Err(reason) => {
                if first_err.is_none() {
                    first_err = Some(reason);
                }
                continue;
            }
        }
        while let Some(d) = pending.remove(&write_idx) {
            // S7: when this segment begins a new init group, its init
            // bytes are written BEFORE the segment they cover.
            let seg = &segments[write_idx];
            let identity = seg.map.as_ref().map(|m| (m.uri.clone(), m.byterange));
            if identity != last_map {
                if let Some(m) = &seg.map {
                    match inits.get(&(m.uri.clone(), m.byterange)) {
                        Some(bytes) => {
                            if writer.write_all(bytes).is_err() {
                                failed = true;
                                break;
                            }
                            total_bytes += bytes.len() as u64;
                        }
                        None => {
                            // unreachable: the pre-pass fetched every
                            // distinct map identity
                            if first_err.is_none() {
                                first_err = Some("#EXT-X-MAP init bytes missing".to_string());
                            }
                            failed = true;
                            break;
                        }
                    }
                }
                last_map = identity;
            }
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
        return Err(Error::Hls(first_err.unwrap_or_else(|| {
            "segment download failed before completion".into()
        })));
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
    identity: bool,
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
    if identity {
        // S7 [R2-5]: init-only discipline — ranged init fetches insist on
        // an uncompressed transfer so the byte-range math stays honest;
        // media-segment fetches never carry this header.
        request = request.header("Accept-Encoding", "identity");
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
// ---------------------------------------------------------------------------
// S7: SAMPLE-AES (AAC in ADTS over MPEG-TS) — the [SampleEnc] slice
// ---------------------------------------------------------------------------

/// Extract the reassembled PSI section bytes for `want_pid` (first
/// section starting at the first payload-start on that PID). Sync bytes
/// are assumed pre-validated by the caller. Returns `None` when the PID
/// is absent or the section is malformed.
fn psi_section(data: &[u8], want_pid: u16) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut collecting = false;
    let mut pos = 0usize;
    while pos + 188 <= data.len() {
        let pkt = &data[pos..pos + 188];
        let pid = (((pkt[1] & 0x1F) as u16) << 8) | (pkt[2] as u16);
        let pusi = (pkt[1] & 0x40) != 0;
        if pid == want_pid {
            let afc = (pkt[3] >> 4) & 0x03;
            if afc == 0 {
                return None;
            }
            let mut p = 4usize;
            if afc == 0b11 {
                p = 5 + pkt[4] as usize;
            }
            if p >= 188 {
                pos += 188;
                continue;
            }
            if collecting && pusi {
                break; // the next section starts — done
            }
            if pusi {
                collecting = true;
                let ptr = pkt[p] as usize;
                if p + 1 + ptr >= 188 {
                    return None;
                }
                out.extend_from_slice(&pkt[p + 1 + ptr..188]);
            } else if collecting {
                out.extend_from_slice(&pkt[p..188]);
            }
            if collecting && out.len() >= 3 {
                let sl = (((out[1] & 0x0F) as usize) << 8) | out[2] as usize;
                if out.len() >= 3 + sl {
                    break;
                }
            }
        }
        pos += 188;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The FIRST (non-NIT) program of a PAT section -> its PMT PID.
fn pat_first_program(section: &[u8]) -> Option<u16> {
    if section.len() < 12 {
        return None;
    }
    let sl = (((section[1] & 0x0F) as usize) << 8) | section[2] as usize;
    if 3 + sl > section.len() || sl < 9 {
        return None;
    }
    let end = 3 + sl - 4; // entries end (before the CRC)
    let mut q = 8usize;
    while q + 4 <= end {
        let prog = ((section[q] as u16) << 8) | (section[q + 1] as u16);
        let pid = (((section[q + 2] & 0x1F) as u16) << 8) | (section[q + 3] as u16);
        if prog != 0 {
            return Some(pid);
        }
        q += 4;
    }
    None
}

/// PMT section -> its elementary stream list `(stream_type, pid)`.
fn pmt_streams(section: &[u8]) -> Option<Vec<(u8, u16)>> {
    if section.len() < 16 {
        return None;
    }
    let sl = (((section[1] & 0x0F) as usize) << 8) | section[2] as usize;
    if 3 + sl > section.len() || sl < 13 {
        return None;
    }
    let end = 3 + sl - 4;
    let pil = (((section[10] & 0x0F) as usize) << 8) | section[11] as usize;
    let mut q = 12 + pil;
    let mut out = Vec::new();
    while q + 5 <= end {
        let st = section[q];
        let pid = (((section[q + 1] & 0x1F) as u16) << 8) | (section[q + 2] as u16);
        let esil = (((section[q + 3] & 0x0F) as usize) << 8) | (section[q + 4] as usize);
        out.push((st, pid));
        q += 5 + esil;
    }
    Some(out)
}

/// Deterministic audio-PID selection per the S7 spec [R2-6]: the FIRST
/// program announced by the PAT, its PMT, and the FIRST audio PID
/// carrying stream type 0x0F (AAC ADTS). Every other elementary stream
/// type in that program loud-fails with a named reason — the slice
/// cannot honestly leave a sibling stream encrypted in the output.
fn select_audio_pid(data: &[u8]) -> Result<u16> {
    let no_audio = || Error::Hls("no decodable audio stream in SAMPLE-AES segment".into());
    let pat = psi_section(data, 0x0000).ok_or_else(no_audio)?;
    let pmt_pid = pat_first_program(&pat).ok_or_else(no_audio)?;
    let pmt = psi_section(data, pmt_pid).ok_or_else(no_audio)?;
    let streams = pmt_streams(&pmt).ok_or_else(no_audio)?;
    let mut audio: Option<u16> = None;
    for (st, pid) in streams {
        match st {
            0x0F => {
                if audio.is_none() {
                    audio = Some(pid);
                }
            }
            0x1B | 0x24 => {
                return Err(Error::Hls(
                    "SAMPLE-AES video sub-sample decryption is not supported".into(),
                ))
            }
            0x81 | 0x87 => {
                return Err(Error::Hls("SAMPLE-AES AC-3 family is not supported".into()))
            }
            0x11 => return Err(Error::Hls("SAMPLE-AES LATM is not supported".into())),
            other => {
                return Err(Error::Hls(format!(
                    "SAMPLE-AES stream type 0x{:02X} is not supported",
                    other
                )))
            }
        }
    }
    audio.ok_or_else(no_audio)
}

/// Decrypt a SAMPLE-AES AAC segment in place (the TS container is
/// PRESERVED — no ADTS-extraction output change). Sub-sample grammar per
/// [SampleEnc] (Apple, "MPEG-2 Stream Encryption Format for HLS", named
/// at RFC 8216 §4.3.2.4; RFC 8216 defines no sub-sample byte layouts):
/// per audio frame the ADTS header (7 or 9 bytes) and the next 16 bytes
/// are CLEAR, a contiguous run of CBC-encrypted 16-byte blocks follows
/// (possibly ZERO for a short frame — treated as fully clear), and a
/// 0–15-byte clear trailer ends the frame. The IV resets to the segment
/// IV at the start of EVERY frame ([SampleEnc] §2.1 — blocks chain
/// within a frame, never across frames).
fn decrypt_sample_aes_aac(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8]) -> Result<()> {
    // 188-byte packet walk with sync-byte validation (ITU-T H.222.0)
    let mut pos = 0usize;
    while pos + 188 <= data.len() {
        if data[pos] != 0x47 {
            return Err(Error::Hls(
                "malformed TS packet stream (sync byte lost)".into(),
            ));
        }
        pos += 188;
    }
    if pos != data.len() {
        return Err(Error::Hls(
            "malformed TS packet stream (trailing bytes)".into(),
        ));
    }
    let audio_pid = select_audio_pid(data)?;
    // PES reassembly for the selected audio PID (payload-start
    // indicators, PES header length skip). `frags` maps the virtual
    // reassembled stream back to in-buffer offsets.
    let mut frags: Vec<(usize, usize)> = Vec::new();
    let mut pes_open = false;
    let mut pos = 0usize;
    while pos + 188 <= data.len() {
        let pkt = &data[pos..pos + 188];
        let pid = (((pkt[1] & 0x1F) as u16) << 8) | (pkt[2] as u16);
        if pid == audio_pid {
            let pusi = (pkt[1] & 0x40) != 0;
            let afc = (pkt[3] >> 4) & 0x03;
            if afc == 0 {
                return Err(Error::Hls(
                    "malformed TS packet (adaptation field control 0)".into(),
                ));
            }
            let mut payload_start = 4usize;
            if afc == 0b10 {
                payload_start = 188; // adaptation only, no payload
            } else if afc == 0b11 {
                let af_len = pkt[4] as usize;
                payload_start = 5 + af_len;
                if payload_start > 188 {
                    return Err(Error::Hls("malformed TS adaptation field".into()));
                }
            }
            if payload_start < 188 {
                let mut frag = (pos + payload_start, 188 - payload_start);
                if pusi {
                    // PES header: 00 00 01 <stream_id> <len:2> <flags:2>
                    // <PES_header_data_length>
                    let p = &data[frag.0..frag.0 + frag.1];
                    if p.len() < 9 || p[0] != 0x00 || p[1] != 0x00 || p[2] != 0x01 {
                        return Err(Error::Hls(
                            "malformed PES packet (start code mismatch)".into(),
                        ));
                    }
                    let skip = 9 + p[8] as usize;
                    if skip > p.len() {
                        return Err(Error::Hls("malformed PES header length".into()));
                    }
                    frag.0 += skip;
                    frag.1 -= skip;
                    pes_open = true;
                }
                if pes_open && frag.1 > 0 {
                    frags.push(frag);
                }
            }
        }
        pos += 188;
    }
    if frags.is_empty() {
        return Err(Error::Hls(
            "no decodable audio stream in SAMPLE-AES segment".into(),
        ));
    }
    // virtual reassembly of the audio PES payload stream
    let mut stream: Vec<u8> = Vec::new();
    for &(off, len) in &frags {
        stream.extend_from_slice(&data[off..off + len]);
    }
    // ADTS frame walk: the sync word (0xFFF) is validated for every
    // frame — the frame-geometry witness. Scope honesty: the header,
    // 16-byte clear leader, and 0–15-byte trailer are clear bytes, so a
    // WRONG KEY leaves every sync word intact (identity SAMPLE-AES has
    // no integrity channel; no key-correctness loud-fail is claimed).
    let mut fpos = 0usize;
    while fpos < stream.len() {
        if fpos + 7 > stream.len() || stream[fpos] != 0xFF || (stream[fpos + 1] & 0xF0) != 0xF0 {
            return Err(Error::Hls(
                "decrypted stream failed the ADTS sync check".into(),
            ));
        }
        let protection_absent = (stream[fpos + 1] & 0x01) != 0;
        let header_len = if protection_absent { 7 } else { 9 };
        let frame_len = (((stream[fpos + 3] & 0x03) as usize) << 11)
            | ((stream[fpos + 4] as usize) << 3)
            | (((stream[fpos + 5] & 0xE0) as usize) >> 5);
        if frame_len < header_len || fpos + frame_len > stream.len() {
            return Err(Error::Hls(
                "decrypted stream failed the ADTS sync check".into(),
            ));
        }
        // [SampleEnc]: clear header, 16-byte clear leader,
        // floor((payload - leader) / 16) * 16 encrypted bytes, clear trailer
        let enc_start = fpos + header_len + 16;
        let remaining = frame_len.saturating_sub(header_len + 16);
        let enc_len = remaining / 16 * 16;
        if enc_len > 0 {
            // per-frame IV reset — no cross-frame chaining
            cbc_decrypt_raw(key, iv, &mut stream[enc_start..enc_start + enc_len]);
        }
        fpos += frame_len;
    }
    // write the decrypted stream back over the original fragment offsets
    let mut soff = 0usize;
    for &(off, len) in &frags {
        data[off..off + len].copy_from_slice(&stream[soff..soff + len]);
        soff += len;
    }
    Ok(())
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

    // ---- S7: EXT-X-MAP ----

    #[test]
    fn map_uri_only_and_grouping() {
        let m = byterange_media(
            "#EXTM3U\n#EXTINF:5.0,\nplain.ts\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        // segments BEFORE the first MAP are unchanged
        assert!(m.segments[0].map.is_none());
        let map = m.segments[1].map.as_ref().unwrap();
        assert!(map.uri.ends_with("/init.mp4"));
        assert_eq!(map.byterange, None);
        assert!(map.key.is_none());
    }

    #[test]
    fn map_byterange_first_offset_zero() {
        let m = byterange_media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\",BYTERANGE=\"1000\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        assert_eq!(
            m.segments[0].map.as_ref().unwrap().byterange,
            Some((1000, 0))
        );
    }

    #[test]
    fn map_byterange_subsequent_chain() {
        let m = byterange_media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\",BYTERANGE=\"1000@0\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-MAP:URI=\"i.mp4\",BYTERANGE=\"500\"\n#EXTINF:5.0,\nmedia2.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        assert_eq!(
            m.segments[0].map.as_ref().unwrap().byterange,
            Some((1000, 0))
        );
        // implicit offset = end of the previous init section
        assert_eq!(
            m.segments[1].map.as_ref().unwrap().byterange,
            Some((500, 1000))
        );
    }

    #[test]
    fn map_byterange_chain_without_history_fails() {
        // an UNRANGED first init leaves no cursor to chain from
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-MAP:URI=\"i.mp4\",BYTERANGE=\"500\"\n#EXTINF:5.0,\nmedia2.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("no previous init"));
    }

    #[test]
    fn map_malformed_byterange_fails() {
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\",BYTERANGE=\"abc\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("bad #EXT-X-MAP BYTERANGE"));
    }

    #[test]
    fn map_empty_uri_fails() {
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("non-empty URI"));
    }

    #[test]
    fn map_byterange_too_large_fails() {
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\",BYTERANGE=\"536870913@0\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("512 MiB"));
    }

    #[test]
    fn map_byterange_zero_length_fails() {
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\",BYTERANGE=\"0@0\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("absurd"));
    }

    #[test]
    fn map_key_snapshot_at_map_line() {
        let m = byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"/k1\",IV=0x01\n#EXT-X-MAP:URI=\"i.mp4\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-KEY:METHOD=AES-128,URI=\"/k2\",IV=0x02\n#EXTINF:5.0,\nmedia2.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        // the MAP snapshots the key state AT the MAP line (k1), and a
        // later key change does not retroactively alter it
        let map = m.segments[0].map.as_ref().unwrap();
        assert_eq!(map.key.as_ref().unwrap().uri.as_deref(), Some("/k1"));
        // the SECOND segment carries the new key but the same map
        assert_eq!(
            m.segments[1].key.as_ref().unwrap().uri.as_deref(),
            Some("/k2")
        );
        assert_eq!(
            m.segments[1]
                .map
                .as_ref()
                .unwrap()
                .key
                .as_ref()
                .unwrap()
                .uri
                .as_deref(),
            Some("/k1")
        );
    }

    #[test]
    fn map_aes128_without_iv_fails() {
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"/k1\"\n#EXT-X-MAP:URI=\"i.mp4\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("requires an explicit IV"));
    }

    #[test]
    fn map_with_sample_aes_fails() {
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"/k1\"\n#EXT-X-MAP:URI=\"i.mp4\"\n#EXTINF:5.0,\nmedia.m4s\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "hls: SAMPLE-AES fMP4 is not supported");
    }

    // ---- S7: EXT-X-KEY hardening ----

    #[test]
    fn sample_aes_keyformat_allowlist() {
        // absent KEYFORMAT proceeds
        assert!(byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"/k\"\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n"
        )
        .is_ok());
        // identity (any case) proceeds
        assert!(byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"/k\",KEYFORMAT=\"IDENTITY\"\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n"
        )
        .is_ok());
        // DRM keyformat -> named loud-fail
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"/k\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("unsupported keyformat"));
        assert!(err.to_string().contains("streamingkeydelivery"));
        // urn:uuid vendor format -> loud-fail
        let err = byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"/k\",KEYFORMAT=\"urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed\"\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("unsupported keyformat"));
    }

    #[test]
    fn key_method_allowlist() {
        for bad in ["SAMPLE-AES-CTR", "AES-256", "AES-192", "SAMPLE-AESCTR"] {
            let err = byterange_media(&format!(
                "#EXTM3U\n#EXT-X-KEY:METHOD={},URI=\"/k\"\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n",
                bad
            ))
            .unwrap_err();
            assert!(
                err.to_string().contains("unsupported EXT-X-KEY METHOD"),
                "{}",
                bad
            );
        }
        // case-insensitive acceptance, normalized uppercase
        let m = byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=aes-128,URI=\"/k\"\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n",
        )
        .unwrap();
        assert_eq!(m.segments[0].key.as_ref().unwrap().method, "AES-128");
    }

    #[test]
    fn key_uri_required_for_non_none() {
        for method in ["AES-128", "SAMPLE-AES"] {
            let err = byterange_media(&format!(
                "#EXTM3U\n#EXT-X-KEY:METHOD={}\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n",
                method
            ))
            .unwrap_err();
            assert!(err.to_string().contains("requires a URI"));
            let err = byterange_media(&format!(
                "#EXTM3U\n#EXT-X-KEY:METHOD={},URI=\"\"\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n",
                method
            ))
            .unwrap_err();
            assert!(err.to_string().contains("requires a URI"));
        }
        // METHOD=NONE needs no URI
        assert!(byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n"
        )
        .is_ok());
    }

    #[test]
    fn invalid_iv_loud_fails() {
        for bad_iv in ["0xZZ", "0x00112233445566778899aabbccddeeff11", ""] {
            let err = byterange_media(&format!(
                "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"/k\",IV=\"{}\"\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n",
                bad_iv
            ))
            .unwrap_err();
            assert!(err.to_string().contains("invalid IV"), "IV={:?}", bad_iv);
        }
        // a VALID short IV still right-aligns
        assert!(byterange_media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"/k\",IV=0xdead\n#EXTINF:5.0,\nseg.ts\n#EXT-X-ENDLIST\n"
        )
        .is_ok());
    }

    // ---- S7: SAMPLE-AES AAC over MPEG-TS ----

    /// Raw CBC encrypt (no padding) — the test-side mirror of
    /// `cbc_decrypt_raw`, never shipped outside `#[cfg(test)]`.
    fn cbc_encrypt_raw(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8]) {
        use aes::cipher::BlockEncrypt;
        let cipher = Aes128::new(key.into());
        let mut prev = [0u8; 16];
        prev.copy_from_slice(iv);
        for chunk in data.chunks_mut(16) {
            for i in 0..16 {
                chunk[i] ^= prev[i];
            }
            let mut block: aes::cipher::generic_array::GenericArray<u8, aes::cipher::typenum::U16> =
                aes::cipher::generic_array::GenericArray::clone_from_slice(chunk);
            cipher.encrypt_block(&mut block);
            chunk.copy_from_slice(&block);
            prev.copy_from_slice(chunk);
        }
    }

    /// Build one clear ADTS frame (7-byte header, protection_absent=1).
    fn adts_frame(payload: &[u8]) -> Vec<u8> {
        let len = 7 + payload.len();
        assert!(len < (1 << 13));
        let mut f = Vec::with_capacity(len);
        f.push(0xFF);
        f.push(0xF1);
        f.push(0x50);
        f.push(0x80 | ((len >> 11) as u8 & 0x03));
        f.push((len >> 3) as u8);
        f.push((((len & 0x07) as u8) << 5) | 0x1F);
        f.push(0xFC);
        f.extend_from_slice(payload);
        f
    }

    /// Apply the [SampleEnc] AAC sub-sample layout to one clear frame:
    /// clear 7-byte header, clear 16-byte leader, encrypted
    /// floor(remaining/16)*16 bytes, clear trailer. IV resets per frame.
    fn sample_aes_encrypt_frame(frame: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
        let mut f = frame.to_vec();
        let enc_start = 7 + 16;
        let remaining = f.len().saturating_sub(enc_start);
        let enc_len = remaining / 16 * 16;
        if enc_len > 0 {
            cbc_encrypt_raw(key, iv, &mut f[enc_start..enc_start + enc_len]);
        }
        f
    }

    /// One TS packet (188 bytes) with adaptation stuffing for short payloads.
    fn ts_packet(pid: u16, pusi: bool, payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() <= 184);
        let mut pkt = Vec::with_capacity(188);
        pkt.push(0x47);
        pkt.push((((pid >> 8) as u8) & 0x1F) | if pusi { 0x40 } else { 0x00 });
        pkt.push((pid & 0xFF) as u8);
        if payload.len() == 184 {
            pkt.push(0x10);
            pkt.extend_from_slice(payload);
        } else {
            let af_len = 183 - payload.len();
            pkt.push(0x30);
            pkt.push(af_len as u8);
            if af_len > 0 {
                pkt.push(0x00);
                let stuff = af_len - 1;
                pkt.resize(pkt.len() + stuff, 0xFF);
            }
            pkt.extend_from_slice(payload);
        }
        assert_eq!(pkt.len(), 188);
        pkt
    }

    /// A PAT section announcing one program on `pmt_pid`.
    fn pat_section(pmt_pid: u16) -> Vec<u8> {
        let s = vec![
            0x00,
            0xB0,
            0x0D, // table_id, section_length
            0x00,
            0x01, // transport_stream_id
            0xC1,
            0x00,
            0x00, // version, section, last_section
            0x00,
            0x01, // program_number
            0xE0 | ((pmt_pid >> 8) as u8 & 0x1F),
            (pmt_pid & 0xFF) as u8,
            0,
            0,
            0,
            0, // CRC (not validated by the walk)
        ];
        s
    }

    /// A PMT section with the given elementary streams.
    fn pmt_section(entries: &[(u8, u16)]) -> Vec<u8> {
        let body_len = 9 + entries.len() * 5 + 4;
        let mut s = vec![
            0x02, // table_id
            0xB0 | ((body_len >> 8) as u8 & 0x0F),
            (body_len & 0xFF) as u8,
            0x00,
            0x01, // program_number
            0xC1,
            0x00,
            0x00,
            0xE0,
            0x00, // PCR PID
            0xF0,
            0x00, // program_info_length
        ];
        for &(st, pid) in entries {
            s.push(st);
            s.push(0xE0 | ((pid >> 8) as u8 & 0x1F));
            s.push((pid & 0xFF) as u8);
            s.push(0xF0);
            s.push(0x00);
        }
        s.extend_from_slice(&[0, 0, 0, 0]);
        s
    }

    /// PSI table bytes wrapped as one TS packet (pointer_field 0).
    fn psi_packet(pid: u16, section: &[u8]) -> Vec<u8> {
        let mut payload = vec![0x00u8];
        payload.extend_from_slice(section);
        assert!(payload.len() <= 184, "fixture PSI section spans packets");
        ts_packet(pid, true, &payload)
    }

    /// One PES packet (audio stream_id 0xC0, no header data).
    fn pes_packet(payload: &[u8]) -> Vec<u8> {
        let mut pes = Vec::new();
        pes.extend_from_slice(&[0x00, 0x00, 0x01, 0xC0]);
        let plen = 3 + payload.len();
        pes.push((plen >> 8) as u8);
        pes.push((plen & 0xFF) as u8);
        pes.extend_from_slice(&[0x80, 0x00, 0x00]);
        pes.extend_from_slice(payload);
        pes
    }

    /// Assemble a SAMPLE-AES AAC TS segment: PAT + PMT (0x0F on
    /// `audio_pid`) + one PES per frame.
    fn sample_aes_segment(frames: &[Vec<u8>], audio_pid: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(psi_packet(0x0000, &pat_section(0x1000)));
        out.extend(psi_packet(0x1000, &pmt_section(&[(0x0F, audio_pid)])));
        for fr in frames {
            let pes = pes_packet(fr);
            let mut first = true;
            for chunk in pes.chunks(184) {
                out.extend(ts_packet(audio_pid, first, chunk));
                first = false;
            }
        }
        out
    }

    #[test]
    fn sample_aes_aac_roundtrip() {
        let key: [u8; 16] = *b"VelocityDM-SAE-A";
        let iv = [7u8; 16];
        // payloads: leader(16) + 2 blocks + trailer(5); leader + 1 block +
        // trailer(3); and a SHORT frame whose encrypted section is ZERO
        // blocks (fully clear)
        let payloads: [&[u8]; 3] = [&[0x11u8; 53][..], &[0x22u8; 35][..], &[0x33u8; 20][..]];
        let clear_frames: Vec<Vec<u8>> = payloads.iter().map(|p| adts_frame(p)).collect();
        let enc_frames: Vec<Vec<u8>> = clear_frames
            .iter()
            .map(|f| sample_aes_encrypt_frame(f, &key, &iv))
            .collect();
        // the zero-block frame is byte-identical clear
        assert_eq!(enc_frames[2], clear_frames[2]);
        assert_ne!(enc_frames[0], clear_frames[0]);
        let mut seg = sample_aes_segment(&enc_frames, 0x0101);
        let clear_seg = sample_aes_segment(&clear_frames, 0x0101);
        decrypt_sample_aes_aac(&key, &iv, &mut seg).unwrap();
        assert_eq!(seg, clear_seg, "in-place SAMPLE-AES decrypt mismatch");
    }

    #[test]
    fn sample_aes_explicit_iv_and_seq_iv_default() {
        let key: [u8; 16] = *b"VelocityDM-SAE-B";
        let payload = [0xABu8; 100]; // 16 + 5*16 + 4
        let clear = adts_frame(&payload);
        let clear_seg = sample_aes_segment(std::slice::from_ref(&clear), 0x0102);
        // explicit IV path
        let explicit_iv = [9u8; 16];
        let mut seg = sample_aes_segment(
            &[sample_aes_encrypt_frame(&clear, &key, &explicit_iv)],
            0x0102,
        );
        decrypt_sample_aes_aac(&key, &explicit_iv, &mut seg).unwrap();
        assert_eq!(seg, clear_seg);
        // media-sequence IV default (the same helper the worker uses)
        let seq = 42u64;
        let seqiv = seq_iv(seq);
        let mut seg2 =
            sample_aes_segment(&[sample_aes_encrypt_frame(&clear, &key, &seqiv)], 0x0102);
        decrypt_sample_aes_aac(&key, &seqiv, &mut seg2).unwrap();
        assert_eq!(seg2, clear_seg);
    }

    #[test]
    fn sample_aes_wrong_key_differs_no_loud_fail() {
        // Scope honesty [R2-1]: a wrong key corrupts only the encrypted
        // section; the clear header/leader/trailer (and every sync word)
        // survive, so NO loud-fail is claimed — the decrypted output just
        // differs byte-wise from the clear frames.
        let key: [u8; 16] = *b"VelocityDM-SAE-C";
        let wrong: [u8; 16] = *b"WRONGKEY-WRONG!!";
        let iv = [1u8; 16];
        let clear = adts_frame(&[0x55u8; 67]);
        let enc = sample_aes_encrypt_frame(&clear, &key, &iv);
        let clear_seg = sample_aes_segment(std::slice::from_ref(&clear), 0x0103);
        let mut seg = sample_aes_segment(&[enc], 0x0103);
        decrypt_sample_aes_aac(&wrong, &iv, &mut seg).unwrap();
        assert_ne!(seg, clear_seg, "wrong key must differ byte-wise");
        // the clear header (7) + clear leader (16) survive untouched
        // inside the TS payload (position-independent witness)
        assert!(
            seg.windows(7 + 16).any(|w| w == &clear[..7 + 16]),
            "header+leader stay clear"
        );
    }

    #[test]
    fn sample_aes_misframed_demux_sync_fail() {
        // frame-geometry witness: a frame_length field that disagrees
        // with the real layout shifts the walk -> sync-word loud-fail
        let key: [u8; 16] = *b"VelocityDM-SAE-D";
        let iv = [2u8; 16];
        let f1 = adts_frame(&[0x77u8; 53]);
        let f2 = adts_frame(&[0x88u8; 53]);
        let mut tampered = sample_aes_encrypt_frame(&f1, &key, &iv);
        // claim a frame_length 8 bytes shorter (still >= header)
        let real_len = 7 + 53;
        tampered[3] = (tampered[3] & 0xFC) | (((real_len - 8) >> 11) as u8 & 0x03);
        tampered[4] = ((real_len - 8) >> 3) as u8;
        tampered[5] = ((((real_len - 8) & 0x07) as u8) << 5) | 0x1F;
        let mut seg = sample_aes_segment(
            &[tampered, sample_aes_encrypt_frame(&f2, &key, &iv)],
            0x0104,
        );
        let err = decrypt_sample_aes_aac(&key, &iv, &mut seg).unwrap_err();
        assert_eq!(
            err.to_string(),
            "hls: decrypted stream failed the ADTS sync check"
        );
    }

    #[test]
    fn sample_aes_pmt_type_allowlist() {
        let key: [u8; 16] = *b"VelocityDM-SAE-E";
        let iv = [3u8; 16];
        let frame = sample_aes_encrypt_frame(&adts_frame(&[0x99u8; 53]), &key, &iv);
        let cases: [(u8, &str); 5] = [
            (
                0x1B,
                "SAMPLE-AES video sub-sample decryption is not supported",
            ),
            (
                0x24,
                "SAMPLE-AES video sub-sample decryption is not supported",
            ),
            (0x81, "SAMPLE-AES AC-3 family is not supported"),
            (0x11, "SAMPLE-AES LATM is not supported"),
            (0x06, "SAMPLE-AES stream type 0x06 is not supported"),
        ];
        for (st, want) in cases {
            let mut seg = Vec::new();
            seg.extend(psi_packet(0x0000, &pat_section(0x1000)));
            seg.extend(psi_packet(0x1000, &pmt_section(&[(st, 0x0201)])));
            let pes = pes_packet(&frame);
            let mut first = true;
            for chunk in pes.chunks(184) {
                seg.extend(ts_packet(0x0201, first, chunk));
                first = false;
            }
            let err = decrypt_sample_aes_aac(&key, &iv, &mut seg).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("hls: {}", want),
                "type 0x{:02X}",
                st
            );
        }
    }

    #[test]
    fn sample_aes_no_audio_pes_fails() {
        let key: [u8; 16] = *b"VelocityDM-SAE-F";
        let iv = [4u8; 16];
        // PMT announces the AAC PID but the stream carries no PES for it
        let mut seg = Vec::new();
        seg.extend(psi_packet(0x0000, &pat_section(0x1000)));
        seg.extend(psi_packet(0x1000, &pmt_section(&[(0x0F, 0x0301)])));
        let err = decrypt_sample_aes_aac(&key, &iv, &mut seg).unwrap_err();
        assert_eq!(
            err.to_string(),
            "hls: no decodable audio stream in SAMPLE-AES segment"
        );
    }

    #[test]
    fn sample_aes_malformed_ts_fails() {
        let key: [u8; 16] = *b"VelocityDM-SAE-G";
        let iv = [5u8; 16];
        // broken sync byte
        let mut seg = sample_aes_segment(&[adts_frame(&[0u8; 53])], 0x0401);
        seg[0] = 0x00;
        let err = decrypt_sample_aes_aac(&key, &iv, &mut seg).unwrap_err();
        assert!(err.to_string().contains("sync byte lost"));
        // trailing bytes (not a whole number of 188-byte packets)
        let mut seg2 = sample_aes_segment(&[adts_frame(&[0u8; 53])], 0x0401);
        seg2.push(0x47);
        let err2 = decrypt_sample_aes_aac(&key, &iv, &mut seg2).unwrap_err();
        assert!(err2.to_string().contains("trailing bytes"));
    }
}
