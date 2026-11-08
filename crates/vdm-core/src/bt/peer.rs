//! Peer wire protocol (BEP-3) + extension protocol (BEP-10) + ut_metadata (BEP-9).

use crate::bt::bencode::{self, Value};
use crate::error::{Error, Result};
use std::io::{Read, Write};
use std::time::Duration;

pub const PROTOCOL_STR: &[u8] = b"BitTorrent protocol";
/// Reserved bits: extension protocol (BEP-10) enabled.
pub const RESERVED_EXT: [u8; 8] = [0, 0, 0, 0, 0, 0x10, 0, 0];
/// Extension message id for the handshake itself.
pub const EXT_HANDSHAKE_ID: u8 = 0;

/// Cap on message body (metadata pieces are up to 16 KiB + dict overhead).
pub const MAX_MSG_LEN: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerMsg {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<bool>),
    Request {
        index: u32,
        begin: u32,
        len: u32,
    },
    Cancel {
        index: u32,
        begin: u32,
        len: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        data: Vec<u8>,
    },
    /// BEP-10 extension message: (extension id, payload).
    Extended {
        ext_id: u8,
        payload: Vec<u8>,
    },
    /// DHT port (BEP-5); ignored.
    Port(u16),
    Unknown {
        id: u8,
        data: Vec<u8>,
    },
}

impl PeerMsg {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            PeerMsg::KeepAlive => vec![0, 0, 0, 0],
            PeerMsg::Choke => frame(0, &[]),
            PeerMsg::Unchoke => frame(1, &[]),
            PeerMsg::Interested => frame(2, &[]),
            PeerMsg::NotInterested => frame(3, &[]),
            PeerMsg::Have(i) => frame(4, &i.to_be_bytes()),
            PeerMsg::Bitfield(bits) => frame(5, &crate::bt::pieces::bitfield_encode(bits)),
            PeerMsg::Request { index, begin, len } => frame(6, &concat_be(*index, *begin, *len)),
            PeerMsg::Cancel { index, begin, len } => frame(8, &concat_be(*index, *begin, *len)),
            PeerMsg::Piece { index, begin, data } => {
                let mut body = Vec::with_capacity(9 + data.len());
                body.extend_from_slice(&concat_be(*index, *begin, 0));
                body.truncate(8);
                body.extend_from_slice(data);
                frame(7, &body)
            }
            PeerMsg::Extended { ext_id, payload } => {
                let mut body = Vec::with_capacity(1 + payload.len());
                body.push(*ext_id);
                body.extend_from_slice(payload);
                frame(20, &body)
            }
            PeerMsg::Port(p) => frame(9, &p.to_be_bytes()),
            PeerMsg::Unknown { id, data } => frame(*id, data),
        }
    }
}

fn concat_be(a: u32, b: u32, c: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(12);
    v.extend_from_slice(&a.to_be_bytes());
    v.extend_from_slice(&b.to_be_bytes());
    v.extend_from_slice(&c.to_be_bytes());
    v
}

fn frame(id: u8, body: &[u8]) -> Vec<u8> {
    let len = (1 + body.len()) as u32;
    let mut out = Vec::with_capacity(4 + 1 + body.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.push(id);
    out.extend_from_slice(body);
    out
}

/// Read one message from a stream. Errors on EOF or malformed frames.
pub fn read_peer_msg(r: &mut impl Read) -> Result<PeerMsg> {
    let mut len_buf = [0u8; 4];
    read_exact(r, &mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MSG_LEN {
        return Err(Error::Torrent(format!(
            "peer message too large ({} bytes)",
            len
        )));
    }
    if len == 0 {
        return Ok(PeerMsg::KeepAlive);
    }
    let mut body = vec![0u8; len];
    read_exact(r, &mut body)?;
    let id = body[0];
    let data = &body[1..];
    Ok(match id {
        0 => PeerMsg::Choke,
        1 => PeerMsg::Unchoke,
        2 => PeerMsg::Interested,
        3 => PeerMsg::NotInterested,
        4 => {
            if data.len() != 4 {
                return Err(Error::Torrent("bad have message".into()));
            }
            PeerMsg::Have(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
        }
        5 => {
            let mut bits = Vec::with_capacity(data.len() * 8);
            for &b in data {
                for bit in (0..8).rev() {
                    bits.push((b >> bit) & 1 == 1);
                }
            }
            PeerMsg::Bitfield(bits)
        }
        6 => PeerMsg::Request {
            index: be32(data, 0),
            begin: be32(data, 4),
            len: be32(data, 8),
        },
        7 => PeerMsg::Piece {
            index: be32(data, 0),
            begin: be32(data, 4),
            data: data[8..].to_vec(),
        },
        8 => PeerMsg::Cancel {
            index: be32(data, 0),
            begin: be32(data, 4),
            len: be32(data, 8),
        },
        9 => PeerMsg::Port(u16::from_be_bytes([data[0], data[1]])),
        20 => PeerMsg::Extended {
            ext_id: data[0],
            payload: data[1..].to_vec(),
        },
        other => PeerMsg::Unknown {
            id: other,
            data: data.to_vec(),
        },
    })
}

fn be32(data: &[u8], off: usize) -> u32 {
    if data.len() < off + 4 {
        return 0;
    }
    u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_exact(r: &mut impl Read, buf: &mut [u8]) -> Result<()> {
    let mut filled = 0;
    // Windows can surface WSAEWOULDBLOCK on blocking sockets as a spurious
    // transient error (observed on CI runners): retry briefly instead of
    // dropping the peer connection.
    let mut would_block = 0u32;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => return Err(Error::Torrent("peer closed connection".into())),
            Ok(n) => {
                filled += n;
                would_block = 0;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                would_block += 1;
                if would_block > 5_000 {
                    return Err(Error::Io(e));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }
    Ok(())
}

pub fn write_peer_msg(w: &mut impl Write, msg: &PeerMsg) -> Result<()> {
    let bytes = msg.encode();
    w.write_all(&bytes)
        .and_then(|_| w.flush())
        .map_err(Error::Io)
}

// ---------------------------------------------------------------------------
// Handshake
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Handshake {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub reserved: [u8; 8],
}

impl Handshake {
    pub fn supports_extensions(&self) -> bool {
        self.reserved[5] & 0x10 != 0
    }
    pub fn encode(info_hash: &[u8; 20], peer_id: &[u8; 20]) -> Vec<u8> {
        let mut out = Vec::with_capacity(68);
        out.push(19);
        out.extend_from_slice(PROTOCOL_STR);
        out.extend_from_slice(&RESERVED_EXT);
        out.extend_from_slice(info_hash);
        out.extend_from_slice(peer_id);
        out
    }
}

/// Send + read a handshake, verifying the info hash when `expected` is given.
pub fn handshake(
    stream: &mut (impl Read + Write),
    info_hash: &[u8; 20],
    our_peer_id: &[u8; 20],
    expected: Option<&[u8; 20]>,
) -> Result<Handshake> {
    let bytes = Handshake::encode(info_hash, our_peer_id);
    stream
        .write_all(&bytes)
        .and_then(|_| stream.flush())
        .map_err(Error::Io)?;
    let mut buf = [0u8; 68];
    read_exact(stream, &mut buf)?;
    if buf[0] != 19 || &buf[1..20] != PROTOCOL_STR {
        return Err(Error::Torrent("not a BitTorrent handshake".into()));
    }
    let mut reserved = [0u8; 8];
    reserved.copy_from_slice(&buf[20..28]);
    let mut ih = [0u8; 20];
    ih.copy_from_slice(&buf[28..48]);
    let mut pid = [0u8; 20];
    pid.copy_from_slice(&buf[48..68]);
    if let Some(exp) = expected {
        if &ih != exp {
            return Err(Error::Torrent("handshake info hash mismatch".into()));
        }
    }
    Ok(Handshake {
        info_hash: ih,
        peer_id: pid,
        reserved,
    })
}

// ---------------------------------------------------------------------------
// Extension protocol helpers (BEP-10 / BEP-9)
// ---------------------------------------------------------------------------

/// Build the extension handshake payload advertising `ut_metadata` and the
/// metadata size (needed for magnet downloads).
pub fn build_ext_handshake(ut_metadata_id: u8, metadata_size: u64) -> Vec<u8> {
    bencode::encode(&Value::Dict(vec![
        (
            b"m".to_vec(),
            Value::Dict(vec![(
                b"ut_metadata".to_vec(),
                Value::Int(ut_metadata_id as i64),
            )]),
        ),
        (b"metadata_size".to_vec(), Value::Int(metadata_size as i64)),
        (b"v".to_vec(), bencode::str("VelocityDM 1.0")),
    ]))
}

#[derive(Debug, Clone, Default)]
pub struct ExtHandshake {
    pub ut_metadata: Option<u8>,
    pub metadata_size: Option<u64>,
}

pub fn parse_ext_handshake(payload: &[u8]) -> Result<ExtHandshake> {
    let (v, _) = bencode::decode_prefix(payload)?;
    let m = v
        .dict_get(b"m")
        .ok_or_else(|| Error::Torrent("ext handshake: no m dict".into()))?;
    let ut = m
        .dict_get(b"ut_metadata")
        .and_then(|u| u.as_int())
        .filter(|&u| u > 0)
        .map(|u| u as u8);
    let size = v
        .dict_get(b"metadata_size")
        .and_then(|s| s.as_int())
        .map(|s| s as u64);
    Ok(ExtHandshake {
        ut_metadata: ut,
        metadata_size: size,
    })
}

/// ut_metadata request payload (msg_type 0).
pub fn build_metadata_request(piece: u32) -> Vec<u8> {
    bencode::encode(&Value::Dict(vec![
        (b"msg_type".to_vec(), Value::Int(0)),
        (b"piece".to_vec(), Value::Int(piece as i64)),
    ]))
}

/// ut_metadata data response payload (msg_type 1) + raw chunk appended.
pub fn build_metadata_data(piece: u32, total_size: u64, chunk: &[u8]) -> Vec<u8> {
    let mut out = bencode::encode(&Value::Dict(vec![
        (b"msg_type".to_vec(), Value::Int(1)),
        (b"piece".to_vec(), Value::Int(piece as i64)),
        (b"total_size".to_vec(), Value::Int(total_size as i64)),
    ]));
    out.extend_from_slice(chunk);
    out
}

#[derive(Debug, Clone)]
pub enum MetadataMsg {
    Request {
        piece: u32,
    },
    Data {
        piece: u32,
        total_size: u64,
        data: Vec<u8>,
    },
    Reject {
        piece: u32,
    },
}

pub fn parse_metadata_msg(payload: &[u8]) -> Result<MetadataMsg> {
    let (v, consumed) = bencode::decode_prefix(payload)?;
    let msg_type = v
        .dict_get(b"msg_type")
        .and_then(|t| t.as_int())
        .ok_or_else(|| Error::Torrent("metadata msg: no msg_type".into()))?;
    let piece = v.dict_get(b"piece").and_then(|p| p.as_int()).unwrap_or(0) as u32;
    match msg_type {
        0 => Ok(MetadataMsg::Request { piece }),
        1 => {
            let total = v
                .dict_get(b"total_size")
                .and_then(|t| t.as_int())
                .unwrap_or(0) as u64;
            Ok(MetadataMsg::Data {
                piece,
                total_size: total,
                data: payload[consumed..].to_vec(),
            })
        }
        2 => Ok(MetadataMsg::Reject { piece }),
        other => Err(Error::Torrent(format!(
            "metadata msg: bad msg_type {}",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg_roundtrip(msg: PeerMsg) {
        let enc = msg.encode();
        let mut cur = std::io::Cursor::new(enc.clone());
        let back = read_peer_msg(&mut cur).unwrap();
        match (&back, &msg) {
            // bitfields decode padded to full bytes; receivers trim to piece count
            (PeerMsg::Bitfield(got), PeerMsg::Bitfield(want)) => {
                assert_eq!(got.len(), want.len().div_ceil(8) * 8);
                assert_eq!(&got[..want.len()], want.as_slice());
            }
            _ => assert_eq!(back, msg),
        }
    }

    #[test]
    fn wire_roundtrips() {
        msg_roundtrip(PeerMsg::KeepAlive);
        msg_roundtrip(PeerMsg::Choke);
        msg_roundtrip(PeerMsg::Unchoke);
        msg_roundtrip(PeerMsg::Interested);
        msg_roundtrip(PeerMsg::NotInterested);
        msg_roundtrip(PeerMsg::Have(42));
        msg_roundtrip(PeerMsg::Bitfield(vec![true, false, true]));
        msg_roundtrip(PeerMsg::Request {
            index: 1,
            begin: 2,
            len: 16384,
        });
        msg_roundtrip(PeerMsg::Cancel {
            index: 1,
            begin: 2,
            len: 3,
        });
        msg_roundtrip(PeerMsg::Piece {
            index: 3,
            begin: 16384,
            data: vec![9u8; 100],
        });
        msg_roundtrip(PeerMsg::Extended {
            ext_id: 1,
            payload: vec![1, 2, 3],
        });
        msg_roundtrip(PeerMsg::Port(6881));
    }

    #[test]
    fn oversized_message_rejected() {
        let mut cur = std::io::Cursor::new((MAX_MSG_LEN as u32 + 1).to_be_bytes().to_vec());
        assert!(read_peer_msg(&mut cur).is_err());
    }

    #[test]
    fn handshake_roundtrip_and_validation() {
        // duplex stream pair (handshake writes before reading)
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut a = std::net::TcpStream::connect(addr).unwrap();
        let (mut b, _) = listener.accept().unwrap();
        let ih = [1u8; 20];
        let pid = crate::util::new_peer_id();
        let bytes = Handshake::encode(&ih, &pid);
        use std::io::Write;
        a.write_all(&bytes).unwrap();
        a.flush().unwrap();
        let hs = handshake(&mut b, &[2u8; 20], &[3u8; 20], Some(&ih)).unwrap();
        assert_eq!(hs.info_hash, ih);
        assert_eq!(hs.peer_id, pid);
        assert!(hs.supports_extensions());
        // b wrote its own handshake back; read it from a
        use std::io::Read;
        let mut got = [0u8; 68];
        a.read_exact(&mut got).unwrap();
        assert_eq!(&got[28..48], &[2u8; 20]);
        assert_eq!(&got[48..68], &[3u8; 20]);
        // mismatch is rejected
        let listener2 = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr2 = listener2.local_addr().unwrap();
        let mut c = std::net::TcpStream::connect(addr2).unwrap();
        let (mut d, _) = listener2.accept().unwrap();
        c.write_all(&bytes).unwrap();
        c.flush().unwrap();
        assert!(handshake(&mut d, &[2u8; 20], &[3u8; 20], Some(&[9u8; 20])).is_err());
    }

    #[test]
    fn ext_protocol_helpers() {
        let hs = build_ext_handshake(3, 40_000);
        let parsed = parse_ext_handshake(&hs).unwrap();
        assert_eq!(parsed.ut_metadata, Some(3));
        assert_eq!(parsed.metadata_size, Some(40_000));

        let req = build_metadata_request(2);
        match parse_metadata_msg(&req).unwrap() {
            MetadataMsg::Request { piece } => assert_eq!(piece, 2),
            other => panic!("{:?}", other),
        }

        let chunk = vec![0xABu8; 300];
        let data = build_metadata_data(2, 40_000, &chunk);
        match parse_metadata_msg(&data).unwrap() {
            MetadataMsg::Data {
                piece,
                total_size,
                data,
            } => {
                assert_eq!((piece, total_size), (2, 40_000));
                assert_eq!(data, chunk);
            }
            other => panic!("{:?}", other),
        }
    }
}
