//! Magnet URI parsing/building (BEP-9 workflow entry point, Motrix/Gopeed feature).

use crate::bt::bencode;
use crate::error::{Error, Result};
use crate::util::percent_decode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Magnet {
    pub info_hash: [u8; 20],
    pub name: Option<String>,
    pub trackers: Vec<String>,
    pub length: Option<u64>,
}

impl Magnet {
    pub fn parse(uri: &str) -> Result<Magnet> {
        let rest = uri
            .strip_prefix("magnet:?")
            .or_else(|| uri.strip_prefix("magnet:"))
            .ok_or_else(|| Error::Torrent("not a magnet URI".into()))?;
        let mut info_hash: Option<[u8; 20]> = None;
        let mut name = None;
        let mut trackers = Vec::new();
        let mut length = None;
        for pair in rest.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = match pair.split_once('=') {
                Some((k, v)) => (k, percent_decode(v)),
                None => (pair, String::new()),
            };
            match k {
                "xt" => {
                    let v = v.trim();
                    let hash = v
                        .strip_prefix("urn:btih:")
                        .ok_or_else(|| Error::Torrent(format!("unsupported xt '{}'", v)))?;
                    info_hash = Some(decode_info_hash(hash)?);
                }
                "dn" => name = Some(v),
                "tr" => trackers.push(v),
                "xl" => length = v.parse::<u64>().ok(),
                _ => {}
            }
        }
        let info_hash = info_hash.ok_or_else(|| Error::Torrent("magnet has no xt info hash".into()))?;
        if trackers.is_empty() {
            return Err(Error::Torrent(
                "magnet has no trackers (DHT is not enabled; add &tr=...)".into(),
            ));
        }
        Ok(Magnet {
            info_hash,
            name,
            trackers,
            length,
        })
    }

    pub fn to_uri(&self) -> String {
        let mut s = String::from("magnet:?xt=urn:btih:");
        s.push_str(&hex::encode(self.info_hash));
        if let Some(n) = &self.name {
            s.push_str(&format!("&dn={}", crate::util::percent_encode_path(n)));
        }
        if let Some(l) = self.length {
            s.push_str(&format!("&xl={}", l));
        }
        for t in &self.trackers {
            s.push_str(&format!("&tr={}", crate::util::percent_encode_path(t)));
        }
        s
    }

    /// Build a magnet from a metainfo (info hash + name + trackers).
    pub fn from_parts(info_hash: [u8; 20], name: Option<String>, trackers: Vec<String>) -> Magnet {
        Magnet {
            info_hash,
            name,
            trackers,
            length: None,
        }
    }
}

fn decode_info_hash(s: &str) -> Result<[u8; 20]> {
    let s = s.trim();
    if s.len() == 40 {
        let bytes = hex::decode(s.to_ascii_lowercase())
            .map_err(|_| Error::Torrent("invalid hex info hash".into()))?;
        let mut out = [0u8; 20];
        out.copy_from_slice(&bytes);
        Ok(out)
    } else if s.len() == 32 {
        // base32 (some clients emit it)
        let bytes = base32_decode(s.to_ascii_uppercase().as_bytes())
            .ok_or_else(|| Error::Torrent("invalid base32 info hash".into()))?;
        if bytes.len() != 20 {
            return Err(Error::Torrent("base32 info hash wrong length".into()));
        }
        let mut out = [0u8; 20];
        out.copy_from_slice(&bytes);
        Ok(out)
    } else {
        Err(Error::Torrent(format!("info hash '{}' has wrong length", s)))
    }
}

fn base32_decode(data: &[u8]) -> Option<Vec<u8>> {
    let mut bits: u32 = 0;
    let mut nbits: u32 = 0;
    let mut out = Vec::new();
    for &c in data {
        if c == b'=' {
            continue;
        }
        let v = match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'2'..=b'7' => (c - b'2' + 26) as u32,
            _ => return None,
        };
        bits = (bits << 5) | v;
        nbits += 5;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
        }
    }
    Some(out)
}

/// RFC 4648 base32 encode (used in tests to build valid magnet hashes).
#[cfg(test)]
fn base32_encode(data: &[u8]) -> String {
    const ALPHA: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut bits: u32 = 0;
    let mut nbits: u32 = 0;
    for &b in data {
        bits = (bits << 8) | b as u32;
        nbits += 8;
        while nbits >= 5 {
            nbits -= 5;
            out.push(ALPHA[((bits >> nbits) & 31) as usize] as char);
        }
    }
    if nbits > 0 {
        out.push(ALPHA[((bits << (5 - nbits)) & 31) as usize] as char);
    }
    while out.len() % 8 != 0 {
        out.push('=');
    }
    out
}

/// Convenience: build a `.torrent`-style announce list from a magnet.
pub fn trackers_from_announce_list(v: &bencode::Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(tiers) = v.as_list() {
        for tier in tiers {
            if let Some(urls) = tier.as_list() {
                for u in urls {
                    if let Some(s) = u.as_str_lossy() {
                        if !out.contains(&s) {
                            out.push(s);
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH_HEX: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn parse_full() {
        let m = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{}&dn=Test%20File&tr=http%3A%2F%2Ft1%2Fann&tr=udp%3A%2F%2Ft2%3A80&xl=1234",
            HASH_HEX
        ))
        .unwrap();
        assert_eq!(hex::encode(m.info_hash), HASH_HEX);
        assert_eq!(m.name.as_deref(), Some("Test File"));
        assert_eq!(m.trackers, vec!["http://t1/ann", "udp://t2:80"]);
        assert_eq!(m.length, Some(1234));
    }

    #[test]
    fn roundtrip_uri() {
        let m = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{}&dn=Big%20File&tr=http%3A%2F%2Ftracker%2Fannounce",
            HASH_HEX
        ))
        .unwrap();
        let uri = m.to_uri();
        let back = Magnet::parse(&uri).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn base32_hash() {
        // RFC 4648 known vector
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI======");
        let hash_bytes: Vec<u8> = (0u8..20).collect();
        assert_eq!(hash_bytes.len(), 20);
        let b32 = base32_encode(&hash_bytes);
        assert_eq!(b32.len(), 32);
        let m = Magnet::parse(&format!("magnet:?xt=urn:btih:{}&tr=http://t/", b32)).unwrap();
        let want: [u8; 20] = hash_bytes.try_into().unwrap();
        assert_eq!(m.info_hash, want);
    }

    #[test]
    fn errors() {
        assert!(Magnet::parse("http://x").is_err());
        assert!(Magnet::parse("magnet:?dn=x").is_err());
        assert!(Magnet::parse(&format!("magnet:?xt=urn:btih:{}", "z".repeat(40))).is_err());
        assert!(Magnet::parse(&format!("magnet:?xt=urn:btih:{}", HASH_HEX)).is_err()); // no trackers
        assert!(Magnet::parse("magnet:?xt=urn:sha1:abc&tr=http://t/").is_err());
    }
}
