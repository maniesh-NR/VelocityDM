//! Bencode codec (BEP-3) with raw-span extraction for `info_hash` computation.

use crate::error::{Error, Result};
use std::fmt;

/// Bencode value. Dictionaries keep insertion order (encoding sorts keys).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Str(Vec<u8>),
    List(Vec<Value>),
    Dict(Vec<(Vec<u8>, Value)>),
}

impl Value {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_str_lossy(&self) -> Option<String> {
        self.as_bytes().map(|b| String::from_utf8_lossy(b).into_owned())
    }
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }
    pub fn as_dict(&self) -> Option<&[(Vec<u8>, Value)]> {
        match self {
            Value::Dict(d) => Some(d),
            _ => None,
        }
    }
    pub fn dict_get(&self, key: &[u8]) -> Option<&Value> {
        self.as_dict()
            .and_then(|d| d.iter().find(|(k, _)| k == key).map(|(_, v)| v))
    }
    /// Human-readable rendering for logs/errors.
    pub fn render(&self) -> String {
        match self {
            Value::Int(i) => format!("{}", i),
            Value::Str(s) => {
                let text = String::from_utf8_lossy(s);
                if text.chars().all(|c| !c.is_control()) {
                    format!("\"{}\"", text)
                } else {
                    format!("b{}", hex::encode(s))
                }
            }
            Value::List(l) => {
                let items: Vec<String> = l.iter().map(|v| v.render()).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Dict(d) => {
                let items: Vec<String> = d
                    .iter()
                    .map(|(k, v)| format!("{}: {}", String::from_utf8_lossy(k), v.render()))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// Decode a complete bencoded document.
pub fn decode(data: &[u8]) -> Result<Value> {
    let mut dec = Decoder { data, pos: 0 };
    let v = dec.decode_value()?;
    if dec.pos != data.len() {
        return Err(Error::Bencode(format!(
            "trailing {} bytes after document",
            data.len() - dec.pos
        )));
    }
    Ok(v)
}

/// Decode one bencoded value from the front of `data`; returns the value and
/// the number of bytes consumed (ut_metadata appends raw bytes after the dict).
pub fn decode_prefix(data: &[u8]) -> Result<(Value, usize)> {
    let mut dec = Decoder { data, pos: 0 };
    let v = dec.decode_value()?;
    Ok((v, dec.pos))
}

struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    fn decode_value(&mut self) -> Result<Value> {
        if self.pos >= self.data.len() {
            return Err(Error::Bencode("unexpected end of input".into()));
        }
        match self.data[self.pos] {
            b'i' => {
                self.pos += 1;
                let end = self.find(b'e').ok_or_else(|| Error::Bencode("unterminated integer".into()))?;
                let text = std::str::from_utf8(&self.data[self.pos..end])
                    .map_err(|_| Error::Bencode("non-utf8 integer".into()))?;
                let n: i64 = text
                    .parse()
                    .map_err(|_| Error::Bencode(format!("bad integer '{}'", text)))?;
                self.pos = end + 1;
                Ok(Value::Int(n))
            }
            b'l' => {
                self.pos += 1;
                let mut items = Vec::new();
                while self.peek() != Some(b'e') {
                    items.push(self.decode_value()?);
                }
                self.pos += 1; // consume 'e'
                Ok(Value::List(items))
            }
            b'd' => {
                self.pos += 1;
                let mut items = Vec::new();
                while self.peek() != Some(b'e') {
                    let key = self.decode_value()?;
                    let key = match key {
                        Value::Str(s) => s,
                        _ => return Err(Error::Bencode("non-string dict key".into())),
                    };
                    let val = self.decode_value()?;
                    items.push((key, val));
                }
                self.pos += 1;
                Ok(Value::Dict(items))
            }
            b'0'..=b'9' => {
                let colon = self
                    .find(b':')
                    .ok_or_else(|| Error::Bencode("unterminated string".into()))?;
                let text = std::str::from_utf8(&self.data[self.pos..colon])
                    .map_err(|_| Error::Bencode("non-utf8 string length".into()))?;
                let len: usize = text
                    .parse()
                    .map_err(|_| Error::Bencode(format!("bad string length '{}'", text)))?;
                if text.len() > 1 && text.starts_with('0') {
                    return Err(Error::Bencode("leading zero in string length".into()));
                }
                let start = colon + 1;
                let end = start
                    .checked_add(len)
                    .ok_or_else(|| Error::Bencode("string length overflow".into()))?;
                if end > self.data.len() {
                    return Err(Error::Bencode("string exceeds input".into()));
                }
                self.pos = end;
                Ok(Value::Str(self.data[start..end].to_vec()))
            }
            other => Err(Error::Bencode(format!("unexpected type byte 0x{:02x}", other))),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn find(&self, needle: u8) -> Option<usize> {
        self.data[self.pos..]
            .iter()
            .position(|&b| b == needle)
            .map(|p| self.pos + p)
    }
}

/// Encode a value (dict keys sorted canonically).
pub fn encode(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    encode_into(&mut out, v);
    out
}

fn encode_into(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Int(i) => {
            out.push(b'i');
            out.extend_from_slice(i.to_string().as_bytes());
            out.push(b'e');
        }
        Value::Str(s) => {
            out.extend_from_slice(s.len().to_string().as_bytes());
            out.push(b':');
            out.extend_from_slice(s);
        }
        Value::List(l) => {
            out.push(b'l');
            for item in l {
                encode_into(out, item);
            }
            out.push(b'e');
        }
        Value::Dict(d) => {
            out.push(b'd');
            let mut sorted: Vec<&(Vec<u8>, Value)> = d.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in sorted {
                encode_into(out, &Value::Str(k.clone()));
                encode_into(out, v);
            }
            out.push(b'e');
        }
    }
}

/// Build a dict value from ordered pairs (order preserved for decode fidelity;
/// `encode` sorts keys).
pub fn dict(pairs: Vec<(Vec<u8>, Value)>) -> Value {
    Value::Dict(pairs)
}

pub fn str(s: &str) -> Value {
    Value::Str(s.as_bytes().to_vec())
}

/// Extract the raw bencoded bytes of the top-level `info` dict from a .torrent
/// document (needed for the exact `SHA-1(info)` info-hash).
pub fn extract_info_bytes(data: &[u8]) -> Result<&[u8]> {
    let mut dec = Decoder { data, pos: 0 };
    if dec.peek() != Some(b'd') {
        return Err(Error::Bencode("torrent is not a dict".into()));
    }
    dec.pos += 1;
    while dec.peek() != Some(b'e') {
        if dec.pos >= data.len() {
            return Err(Error::Bencode("unterminated torrent dict".into()));
        }
        let key = match dec.decode_value() {
            Ok(Value::Str(s)) => s,
            Ok(_) => return Err(Error::Bencode("non-string dict key".into())),
            Err(e) => return Err(e),
        };
        if key == b"info" {
            let start = dec.pos;
            let _ = dec.decode_value()?;
            let end = dec.pos;
            return Ok(&data[start..end]);
        }
        let _ = dec.decode_value()?;
    }
    Err(Error::Bencode("no info dict in torrent".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_basics() {
        assert_eq!(decode(b"i42e").unwrap(), Value::Int(42));
        assert_eq!(decode(b"i-7e").unwrap(), Value::Int(-7));
        assert_eq!(decode(b"4:spam").unwrap(), Value::Str(b"spam".to_vec()));
        assert_eq!(decode(b"0:").unwrap(), Value::Str(vec![]));
        let l = decode(b"li1ei2ee").unwrap();
        assert_eq!(l.as_list().unwrap().len(), 2);
        let d = decode(b"d3:bar4:spam3:fooi42ee").unwrap();
        assert_eq!(d.dict_get(b"foo").unwrap().as_int(), Some(42));
        assert_eq!(d.dict_get(b"bar").unwrap().as_bytes(), Some(&b"spam"[..]));
    }

    #[test]
    fn decode_errors() {
        assert!(decode(b"").is_err());
        assert!(decode(b"02:x").is_err());
        assert!(decode(b"4:spa").is_err());
        assert!(decode(b"li1eeX").is_err());
        assert!(decode(b"d3:foo3:bar3:baz").is_err());
    }

    #[test]
    fn encode_roundtrip() {
        let v = dict(vec![
            (b"announce".to_vec(), str("http://t.test/ann")),
            (
                b"info".to_vec(),
                dict(vec![
                    (b"length".to_vec(), Value::Int(5)),
                    (b"name".to_vec(), str("f.bin")),
                    (b"piece length".to_vec(), Value::Int(16384)),
                    (b"pieces".to_vec(), Value::Str(vec![0u8; 40])),
                ]),
            ),
        ]);
        let enc = encode(&v);
        let back = decode(&enc).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn encode_sorts_dict_keys() {
        let v = dict(vec![
            (b"b".to_vec(), Value::Int(1)),
            (b"a".to_vec(), Value::Int(2)),
        ]);
        assert_eq!(encode(&v), b"d1:ai2e1:bi1ee");
    }

    #[test]
    fn info_span_extraction_exact() {
        // dict keys NOT in canonical order; the span must reproduce the
        // original bytes exactly so SHA-1(info) matches other clients.
        let raw = b"d8:announce9:http://x/4:infod6:lengthi5e4:name5:f.bin6:pieces40:0123456789012345678901234567890123456789ee";
        let info = extract_info_bytes(raw).unwrap();
        assert_eq!(
            info,
            b"d6:lengthi5e4:name5:f.bin6:pieces40:0123456789012345678901234567890123456789e"
        );
        let full = decode(raw).unwrap();
        assert_eq!(
            full.dict_get(b"announce").unwrap().as_str_lossy().unwrap(),
            "http://x/"
        );
    }

    #[test]
    fn info_span_missing() {
        assert!(extract_info_bytes(b"d4:infod4:name5:f.bine").is_ok());
        assert!(extract_info_bytes(b"d1:ai1ee").is_err());
        assert!(extract_info_bytes(b"le").is_err());
    }

    #[test]
    fn render_helpers() {
        let v = decode(b"d1:al2:ab2:cdee").unwrap();
        assert!(v.render().contains("[\"ab\", \"cd\"]"));
        assert!(Value::Str(vec![0xff, 0x00]).render().starts_with("b"));
    }
}
