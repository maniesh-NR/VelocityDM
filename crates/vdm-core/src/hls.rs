// SPDX-License-Identifier: GPL-3.0-or-later
//! M3U8/HLS playlist downloads.
//!
//! Supports master playlists (highest-bandwidth variant is selected) and
//! media playlists. AES-128 segments (METHOD=AES-128, explicit IV or the
//! media sequence number) are decrypted with PKCS7 padding removed per
//! segment; a playlist without `EXT-X-ENDLIST` is treated as live and
//! rejected. Segments are fetched sequentially with connection reuse and
//! streamed to disk, so memory stays flat regardless of playlist length.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::{Result, VdmError};
use crate::httpc::HttpClient;
use crate::rate_limiter::TokenBucket;

#[derive(Debug, Clone)]
pub struct Segment {
    pub url: String,
    pub seq: u64,
}

#[derive(Debug, Clone)]
pub struct Key {
    pub method: String,
    pub uri: Option<String>,
    pub iv: Option<[u8; 16]>,
}

/// Downloads a playlist (following one master hop) into `out_path`.
#[allow(clippy::too_many_arguments)]
pub async fn download_playlist(
    client: &HttpClient,
    urls: &[String],
    headers: &[(String, String)],
    out_path: &Path,
    pause: &AtomicBool,
    cancel: &AtomicBool,
    limiter: &Arc<TokenBucket>,
    global_limiter: &Arc<TokenBucket>,
) -> Result<()> {
    let mut playlist_url = urls
        .first()
        .ok_or_else(|| VdmError::InvalidInput("no playlist url".into()))?
        .clone();
    let mut body = fetch_text_async(client, &playlist_url, headers).await?;

    // Master playlist: pick the highest BANDWIDTH variant (one hop).
    if let Some(variant) = pick_variant(&body, &playlist_url) {
        playlist_url = variant;
        body = fetch_text_async(client, &playlist_url, headers).await?;
    }

    let base = url::Url::parse(&playlist_url)?;
    let (segments, keys) = parse_media(&body, &base)?;

    let mut file = std::io::BufWriter::with_capacity(256 * 1024, std::fs::File::create(out_path)?);
    let mut current_key: Option<Key> = None;
    let mut key_bytes: Option<Vec<u8>> = None;

    for seg in &segments {
        if cancel.load(Ordering::Relaxed) {
            return Err(VdmError::Cancelled);
        }
        while pause.load(Ordering::Relaxed) {
            if cancel.load(Ordering::Relaxed) {
                return Err(VdmError::Cancelled);
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        if let Some(key) = keys.get(&seg.seq) {
            if key.method != current_key.as_ref().map(|k| k.method.clone()).unwrap_or_default()
                || key.uri != current_key.as_ref().and_then(|k| k.uri.clone())
            {
                current_key = Some(key.clone());
                key_bytes = match key.method.as_str() {
                    "NONE" => None,
                    "AES-128" => {
                        let uri = key
                            .uri
                            .as_ref()
                            .ok_or_else(|| VdmError::Hls("AES-128 key without URI".into()))?;
                        let resolved = base.join(uri)?;
                        let (resp, data) = client.get(resolved.as_str(), headers).await?.read_to_end_limited(64 * 1024).await?;
                        drop(resp);
                        if data.len() != 16 {
                            return Err(VdmError::Hls(format!("key is {} bytes, expected 16", data.len())));
                        }
                        Some(data)
                    }
                    other => return Err(VdmError::Hls(format!("unsupported key method: {}", other))),
                };
            }
        }
        let (resp, mut cipher) = client
            .get(&seg.url, headers)
            .await?
            .read_to_end_limited(256 * 1024 * 1024)
            .await?;
        drop(resp);

        let plain = match (&current_key, &key_bytes) {
            (Some(key), Some(kb)) if key.method == "AES-128" => {
                let iv = key.iv.unwrap_or_else(|| sequence_iv(seg.seq));
                decrypt_aes128_cbc(&cipher, kb, &iv).map_err(VdmError::Hls)?
            }
            _ => std::mem::take(&mut cipher),
        };

        // Rate limiting applies to plaintext bytes (== network bytes for
        // clear segments; for AES segments the wire size is the padded size,
        // close enough for pacing).
        let mut granted_total = 0usize;
        while granted_total < plain.len() {
            let g = global_limiter.acquire(64 * 1024).await;
            let _ = limiter.acquire(g).await;
            granted_total += g as usize;
        }
        file.write_all(&plain)?;
    }
    file.flush()?;
    Ok(())
}

async fn fetch_text_async(client: &HttpClient, url: &str, headers: &[(String, String)]) -> Result<String> {
    let (resp, data) = client.get(url, headers).await?.read_to_end_limited(8 * 1024 * 1024).await?;
    if resp.status >= 400 {
        return Err(VdmError::Status { code: resp.status, url: url.to_string() });
    }
    Ok(String::from_utf8_lossy(&data).into_owned())
}

/// Selects the highest-bandwidth variant URI from a master playlist.
pub fn pick_variant(text: &str, base_url: &str) -> Option<String> {
    let base = url::Url::parse(base_url).ok()?;
    let mut best: Option<(u64, String)> = None;
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("#EXT-X-STREAM-INF") {
            let bandwidth = line
                .split(',')
                .find_map(|attr| {
                    let attr = attr.trim();
                    let upper = attr.to_ascii_uppercase();
                    let idx = upper.find("BANDWIDTH=")?;
                    let value = &attr[idx + "BANDWIDTH=".len()..];
                    value.split(';').next()?.trim().parse::<u64>().ok()
                })
                .unwrap_or(0);
            // URI is the next non-comment, non-empty line.
            while let Some(next) = lines.peek() {
                let next = next.trim();
                if next.is_empty() {
                    lines.next();
                    continue;
                }
                if next.starts_with('#') {
                    break;
                }
                if let Ok(resolved) = base.join(next) {
                    if best.as_ref().map(|(b, _)| bandwidth > *b).unwrap_or(true) {
                        best = Some((bandwidth, resolved.to_string()));
                    }
                }
                break;
            }
        }
    }
    best.map(|(_, u)| u)
}

/// Parses a media playlist into ordered segments plus the key that applies to
/// each sequence number.
pub fn parse_media(text: &str, base: &url::Url) -> Result<(Vec<Segment>, std::collections::HashMap<u64, Key>)> {
    if !text.contains("#EXTM3U") {
        return Err(VdmError::Hls("not an M3U8 playlist".into()));
    }
    let live = !text.contains("#EXT-X-ENDLIST");
    if live {
        return Err(VdmError::Hls("live playlists are not supported (no EXT-X-ENDLIST)".into()));
    }
    let media_sequence = text
        .lines()
        .find_map(|l| l.strip_prefix("#EXT-X-MEDIA-SEQUENCE:").and_then(|v| v.trim().parse::<u64>().ok()))
        .unwrap_or(0);

    let mut segments = Vec::new();
    let mut keys = std::collections::HashMap::new();
    let mut current_key: Option<Key> = None;
    let mut seq = media_sequence;

    for line in text.lines() {
        let line = line.trim();
        if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            let key = parse_key_attrs(attrs)?;
            current_key = Some(key);
            continue;
        }
        if line.starts_with("#EXTINF") {
            // The following non-comment line is the segment URI.
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let url = base.join(line).map_err(|e| VdmError::Hls(format!("bad segment url: {}", e)))?;
        if let Some(key) = &current_key {
            keys.entry(seq).or_insert_with(|| key.clone());
        }
        segments.push(Segment { url: url.to_string(), seq });
        seq += 1;
    }
    if segments.is_empty() {
        return Err(VdmError::Hls("playlist contains no segments".into()));
    }
    Ok((segments, keys))
}

fn parse_key_attrs(attrs: &str) -> Result<Key> {
    let mut method = String::from("NONE");
    let mut uri = None;
    let mut iv = None;
    for attr in attrs.split(',') {
        let mut parts = attr.splitn(2, '=');
        let name = parts.next().unwrap_or("").trim().to_ascii_uppercase();
        let value = parts.next().unwrap_or("").trim().trim_matches('"').to_string();
        match name.as_str() {
            "METHOD" => method = value,
            "URI" => uri = Some(value),
            "IV" => {
                let hex_part = value.trim_start_matches("0x").trim_start_matches("0X");
                let bytes = hex::decode(hex_part).map_err(|e| VdmError::Hls(format!("bad IV: {}", e)))?;
                if bytes.len() != 16 {
                    return Err(VdmError::Hls("IV must be 16 bytes".into()));
                }
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes);
                iv = Some(arr);
            }
            _ => {}
        }
    }
    Ok(Key { method, uri, iv })
}

fn sequence_iv(seq: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&seq.to_be_bytes());
    iv
}

/// AES-128-CBC decrypt with PKCS7 padding removed (per HLS segment).
#[cfg(feature = "hls-aes")]
fn decrypt_aes128_cbc(cipher: &[u8], key: &[u8], iv: &[u8; 16]) -> std::result::Result<Vec<u8>, String> {
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
    if cipher.is_empty() {
        return Ok(Vec::new());
    }
    if cipher.len() % 16 != 0 {
        return Err(format!("ciphertext length {} is not a multiple of 16", cipher.len()));
    }
    Aes128CbcDec::new(key.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(cipher)
        .map_err(|e| format!("padding error: {}", e))
}

#[cfg(not(feature = "hls-aes"))]
fn decrypt_aes128_cbc(_cipher: &[u8], _key: &[u8], _iv: &[u8; 16]) -> std::result::Result<Vec<u8>, String> {
    Err("encrypted HLS requires the 'hls-aes' feature".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_highest_bandwidth_variant() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=640x360\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=4128000,RESOLUTION=1280x720\nhigh.m3u8\n";
        let picked = pick_variant(master, "https://cdn.example.com/v/master.m3u8").unwrap();
        assert_eq!(picked, "https://cdn.example.com/v/high.m3u8");
    }

    #[test]
    fn parses_media_playlist_with_key() {
        let base = url::Url::parse("https://cdn.example.com/v/media.m3u8").unwrap();
        let media = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x000102030405060708090a0b0c0d0e0f\n#EXTINF:9.0,\nseg0.ts\n#EXTINF:9.0,\nseg1.ts\n#EXT-X-ENDLIST\n";
        let (segments, keys) = parse_media(media, &base).unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].seq, 7);
        assert_eq!(segments[0].url, "https://cdn.example.com/v/seg0.ts");
        assert_eq!(segments[1].seq, 8);
        let key = keys.get(&7).unwrap();
        assert_eq!(key.method, "AES-128");
        assert_eq!(key.uri.as_deref(), Some("key.bin"));
        assert_eq!(key.iv.unwrap()[15], 0x0f);
    }

    #[test]
    fn rejects_live_playlist() {
        let base = url::Url::parse("https://cdn.example.com/live.m3u8").unwrap();
        let live = "#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXTINF:9.0,\nseg.ts\n";
        assert!(parse_media(live, &base).is_err());
        assert!(parse_media("<html>", &base).is_err());
    }

    #[test]
    fn sequence_iv_encodes_big_endian() {
        let iv = sequence_iv(1);
        assert_eq!(&iv[8..], &[0, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[cfg(feature = "hls-aes")]
    #[test]
    fn aes_decrypt_roundtrip_with_pkcs7() {
        use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
        let key = [7u8; 16];
        let iv = [3u8; 16];
        let plain = b"hello hls segment payload!"; // 26 bytes
        let cipher = Aes128CbcEnc::new(&key.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain);
        assert_eq!(cipher.len(), 32);
        let out = decrypt_aes128_cbc(&cipher, &key, &iv).unwrap();
        assert_eq!(out, plain);
    }
}
