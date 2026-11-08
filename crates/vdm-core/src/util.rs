//! Small utilities: base64 (proxy auth), percent-encoding (tracker queries),
//! byte-size parsing/formatting (CLI limits), filename sanitizing, randomness.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (used for HTTP Basic proxy authentication).
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn b64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        if c == b'=' || c == b'\r' || c == b'\n' {
            continue;
        }
        let v = b64_val(c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Percent-encode for a URI path segment (keeps `/` and common safe chars).
pub fn percent_encode_path(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Percent-encode arbitrary binary data for a query string value (tracker `info_hash`).
/// Every byte that is not an RFC 3986 unreserved character is escaped.
pub fn percent_encode_query_binary(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 3);
    for &b in data {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Parse human byte sizes: `500`, `2MB`, `1.5MiB`, `512K`, `10m`.
pub fn parse_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, mult): (f64, u64) = {
        let lower = s.to_ascii_lowercase();
        let suffixes = [
            ("kib", 1024u64),
            ("mib", 1024 * 1024),
            ("gib", 1024 * 1024 * 1024),
            ("tib", 1024u64 * 1024 * 1024 * 1024),
            ("kb", 1000),
            ("mb", 1000 * 1000),
            ("gb", 1000 * 1000 * 1000),
            ("tb", 1000u64 * 1000 * 1000 * 1000),
            ("k", 1024),
            ("m", 1024 * 1024),
            ("g", 1024 * 1024 * 1024),
            ("t", 1024u64 * 1024 * 1024 * 1024),
            ("b", 1),
        ];
        let mut found = None;
        for (suf, m) in suffixes {
            if let Some(stripped) = lower.strip_suffix(suf) {
                found = Some((stripped.to_string(), m));
                break;
            }
        }
        match found {
            Some((n, m)) => (n.parse::<f64>().ok()?, m),
            None => (lower.parse::<f64>().ok()?, 1),
        }
    };
    if num < 0.0 {
        return None;
    }
    Some((num * mult as f64) as u64)
}

/// Format bytes for CLI display: `1.4 MiB`.
pub fn format_bytes(b: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if b < 1024 {
        return format!("{} B", b);
    }
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < 5 {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.1} {}", v, UNITS[i])
}

/// Sanitize a filename across platforms (strip separators, control chars, `..`).
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "download".to_string()
    } else {
        trimmed
    }
}

/// Extract a filename from a URL path (percent-decoded, sanitized).
pub fn filename_from_url(url: &str) -> Option<String> {
    let no_scheme = match url.split_once("://") {
        Some((_, rest)) => rest,
        None => url,
    };
    let path = match no_scheme.split_once('/') {
        Some((_, path)) => path,
        None => "",
    };
    let path = path.split(['?', '#']).next()?;
    let path = path.trim_end_matches('/');
    let base = path.rsplit('/').next()?;
    if base.is_empty() {
        return None;
    }
    let decoded = percent_decode(base);
    let name = sanitize_filename(&decoded);
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub fn percent_decode(s: &str) -> String {
    String::from_utf8_lossy(&percent_decode_bytes(s)).into_owned()
}

/// Percent-decode to raw bytes (needed for binary values like `info_hash`).
pub fn percent_decode_bytes(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Random bytes (peer ids, boundary strings). Tries the OS entropy source,
/// falls back to a time/pid-seeded xorshift.
pub fn random_bytes(n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(&mut buf).is_ok() {
            return buf;
        }
    }
    // fallback: xorshift seeded from clock + pid
    let mut state = now_ms()
        ^ (std::process::id() as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ (&buf as *const _ as u64);
    for b in buf.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *b = (state >> 24) as u8;
    }
    buf
}

/// A 20-byte BitTorrent peer id: `-VD1000-` + 12 random bytes.
pub fn new_peer_id() -> [u8; 20] {
    let r = random_bytes(12);
    let mut id = [0u8; 20];
    id[..8].copy_from_slice(b"-VD1000-");
    id[8..].copy_from_slice(&r);
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            let enc = base64_encode(s.as_bytes());
            assert_eq!(base64_decode(&enc).unwrap(), s.as_bytes());
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
    }

    #[test]
    fn percent_encoding() {
        assert_eq!(percent_encode_query_binary(&[0x41, 0x00, 0xff]), "A%00%FF");
        assert_eq!(percent_encode_path("a b/c"), "a%20b/c");
    }

    #[test]
    fn parse_bytes_cases() {
        assert_eq!(parse_bytes("500"), Some(500));
        assert_eq!(parse_bytes("2MB"), Some(2_000_000));
        assert_eq!(parse_bytes("1.5MiB"), Some(1_572_864));
        assert_eq!(parse_bytes("512K"), Some(512 * 1024));
        assert_eq!(parse_bytes("10m"), Some(10 * 1024 * 1024));
        assert_eq!(parse_bytes("abc"), None);
    }

    #[test]
    fn format_bytes_cases() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MiB");
    }

    #[test]
    fn filename_from_url_cases() {
        assert_eq!(
            filename_from_url("https://x.test/a/b/file.zip?x=1"),
            Some("file.zip".to_string())
        );
        assert_eq!(
            filename_from_url("https://x.test/my%20file.bin"),
            Some("my file.bin".to_string())
        );
        assert_eq!(filename_from_url("https://x.test/"), None);
    }

    #[test]
    fn sanitize_cases() {
        assert_eq!(sanitize_filename("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_filename(".."), "download");
    }

    #[test]
    fn random_and_peer_id() {
        let a = random_bytes(32);
        assert_eq!(a.len(), 32);
        let id = new_peer_id();
        assert_eq!(&id[..8], b"-VD1000-");
        assert!(id[8..].iter().any(|&b| b != 0));
    }
}
