//! Torrent metainfo: .torrent parsing, magnet info-dict parsing, torrent
//! creation, and piece-to-file mapping.

use crate::bt::bencode::{self, Value};
use crate::error::{Error, Result};
use sha1::{Digest, Sha1};


pub fn sha1_20(data: &[u8]) -> [u8; 20] {
    let d: [u8; 20] = Sha1::digest(data).into();
    d
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TorrentFile {
    pub length: u64,
    /// Path components below the torrent root (multi-file) or single name.
    pub path: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Metainfo {
    pub info_hash: [u8; 20],
    /// Raw bencoded info dict (magnet metadata exchange payload).
    pub info_bytes: Vec<u8>,
    pub piece_len: u64,
    pub pieces: Vec<[u8; 20]>,
    pub name: String,
    pub files: Vec<TorrentFile>,
    /// Cumulative start offsets per file.
    pub file_offsets: Vec<u64>,
    pub total_len: u64,
    pub announce: Option<String>,
    /// All trackers flattened from announce-list (+ announce), unique.
    pub trackers: Vec<String>,
}

impl Metainfo {
    pub fn from_torrent_bytes(data: &[u8]) -> Result<Metainfo> {
        let root = bencode::decode(data)?;
        if root.as_dict().is_none() {
            return Err(Error::Torrent("torrent root is not a dict".into()));
        }
        let info_bytes = bencode::extract_info_bytes(data)?.to_vec();
        let announce = root
            .dict_get(b"announce")
            .and_then(|v| v.as_str_lossy())
            .filter(|s| !s.is_empty());
        let mut trackers = Vec::new();
        if let Some(a) = &announce {
            trackers.push(a.clone());
        }
        if let Some(list) = root.dict_get(b"announce-list").and_then(|v| v.as_list()) {
            for tier in list {
                if let Some(urls) = tier.as_list() {
                    for u in urls {
                        if let Some(s) = u.as_str_lossy() {
                            if !s.is_empty() && !trackers.contains(&s) {
                                trackers.push(s);
                            }
                        }
                    }
                }
            }
        }
        let info = Self::from_info_bytes(info_bytes, &trackers)?;
        Ok(info)
    }

    pub fn from_info_bytes(info_bytes: Vec<u8>, trackers: &[String]) -> Result<Metainfo> {
        let info = bencode::decode(&info_bytes)?;
        let info_hash = sha1_20(&info_bytes);
        let piece_len_i = info
            .dict_get(b"piece length")
            .and_then(|v| v.as_int())
            .ok_or_else(|| Error::Torrent("info: missing piece length".into()))?;
        if piece_len_i <= 0 {
            return Err(Error::Torrent("info: bad piece length".into()));
        }
        let piece_len = piece_len_i as u64;
        let pieces_raw = info
            .dict_get(b"pieces")
            .and_then(|v| v.as_bytes())
            .ok_or_else(|| Error::Torrent("info: missing pieces".into()))?
            .to_vec();
        if pieces_raw.is_empty() || pieces_raw.len() % 20 != 0 {
            return Err(Error::Torrent("info: pieces length not a multiple of 20".into()));
        }
        let pieces: Vec<[u8; 20]> = pieces_raw
            .chunks(20)
            .map(|c| {
                let mut h = [0u8; 20];
                h.copy_from_slice(c);
                h
            })
            .collect();
        let name = info
            .dict_get(b"name")
            .and_then(|v| v.as_str_lossy())
            .ok_or_else(|| Error::Torrent("info: missing name".into()))?;
        let mut files = Vec::new();
        let mut total_len = 0u64;
        if let Some(flist) = info.dict_get(b"files").and_then(|v| v.as_list()) {
            for f in flist {
                let length = f
                    .dict_get(b"length")
                    .and_then(|v| v.as_int())
                    .ok_or_else(|| Error::Torrent("file entry: missing length".into()))?;
                if length < 0 {
                    return Err(Error::Torrent("file entry: negative length".into()));
                }
                let path = f
                    .dict_get(b"path")
                    .and_then(|v| v.as_list())
                    .ok_or_else(|| Error::Torrent("file entry: missing path".into()))?
                    .iter()
                    .map(|p| p.as_str_lossy().unwrap_or_default())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<String>>();
                if path.is_empty() {
                    return Err(Error::Torrent("file entry: empty path".into()));
                }
                total_len += length as u64;
                files.push(TorrentFile {
                    length: length as u64,
                    path,
                });
            }
        } else {
            let length = info
                .dict_get(b"length")
                .and_then(|v| v.as_int())
                .ok_or_else(|| Error::Torrent("info: neither files nor length".into()))?;
            if length < 0 {
                return Err(Error::Torrent("info: negative length".into()));
            }
            total_len = length as u64;
            files.push(TorrentFile {
                length: total_len,
                path: vec![name.clone()],
            });
        }
        let expected_pieces = (total_len + piece_len - 1) / piece_len;
        if expected_pieces != pieces.len() as u64 {
            return Err(Error::Torrent(format!(
                "piece count mismatch: {} hashes for {} expected pieces",
                pieces.len(),
                expected_pieces
            )));
        }
        let file_offsets = {
            let mut offs = Vec::with_capacity(files.len());
            let mut acc = 0u64;
            for f in &files {
                offs.push(acc);
                acc += f.length;
            }
            offs
        };
        Ok(Metainfo {
            info_hash,
            info_bytes,
            piece_len,
            pieces,
            name,
            files,
            file_offsets,
            total_len,
            announce: trackers.first().cloned(),
            trackers: trackers.to_vec(),
        })
    }

    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    pub fn piece_len_at(&self, index: u32) -> u64 {
        let start = index as u64 * self.piece_len;
        self.piece_len.min(self.total_len.saturating_sub(start))
    }

    pub fn piece_offset(&self, index: u32) -> u64 {
        index as u64 * self.piece_len
    }

    /// Map a byte span within piece `index` to (file_index, offset_in_file, len) triples.
    pub fn map_piece_span(&self, index: u32, off: u64, len: u64) -> Vec<(usize, u64, u64)> {
        let mut out = Vec::new();
        if len == 0 {
            return out;
        }
        let mut pos = self.piece_offset(index) + off;
        let mut remaining = len.min(self.total_len.saturating_sub(pos));
        while remaining > 0 {
            // find file containing pos (files sorted by offset)
            let fi = match self.file_offsets.binary_search(&pos) {
                Ok(i) => i,
                Err(i) => i.saturating_sub(1),
            };
            let fstart = self.file_offsets[fi];
            let fend = fstart + self.files[fi].length;
            if pos >= fend {
                break; // beyond last file (padding)
            }
            let in_file = pos - fstart;
            let n = (fend - pos).min(remaining);
            out.push((fi, in_file, n));
            pos += n;
            remaining -= n;
        }
        out
    }

    /// Which files overlap a piece (for priority filtering).
    pub fn files_overlapping_piece(&self, index: u32) -> Vec<usize> {
        self.map_piece_span(index, 0, self.piece_len_at(index))
            .into_iter()
            .map(|(fi, _, _)| fi)
            .collect()
    }

    /// Piece indices that overlap the given file.
    pub fn pieces_overlapping_file(&self, file_idx: usize) -> Vec<u32> {
        let start = self.file_offsets[file_idx];
        let end = start + self.files[file_idx].length;
        let first = (start / self.piece_len) as u32;
        let last = ((end.saturating_sub(1)) / self.piece_len) as u32;
        (first..=last).collect()
    }
}

/// Input for torrent creation.
pub struct CreateInput {
    /// Path components below the torrent root.
    pub path: Vec<String>,
    pub fs_path: std::path::PathBuf,
}

/// Create a .torrent from on-disk files (Motrix/Gopeed-style "share" feature).
/// Returns (metainfo, full .torrent bytes).
pub fn create_torrent(
    name: &str,
    inputs: &[CreateInput],
    piece_len: u64,
    announce: Option<&str>,
    extra_trackers: &[String],
    private: bool,
) -> Result<(Metainfo, Vec<u8>)> {
    if inputs.is_empty() {
        return Err(Error::Torrent("no input files".into()));
    }
    if piece_len == 0 {
        return Err(Error::Torrent("piece length must be > 0".into()));
    }
    // sort inputs by path for determinism
    let mut inputs: Vec<&CreateInput> = inputs.iter().collect();
    inputs.sort_by(|a, b| a.path.cmp(&b.path));

    let mut hasher = Sha1::new();
    let mut pieces: Vec<[u8; 20]> = Vec::new();
    let mut buffered = 0u64;
    let mut files_value = Vec::new();
    let mut total_len = 0u64;
    let mut single_file: Option<&CreateInput> = None;

    for input in &inputs {
        let len = std::fs::metadata(&input.fs_path)
            .map_err(|e| Error::Torrent(format!("stat {}: {}", input.fs_path.display(), e)))?
            .len();
        total_len += len;
        if inputs.len() == 1 {
            single_file = Some(*input);
        }
        files_value.push(bencode::dict(vec![
            (b"length".to_vec(), Value::Int(len as i64)),
            (
                b"path".to_vec(),
                Value::List(
                    input
                        .path
                        .iter()
                        .map(|c| bencode::str(c))
                        .collect(),
                ),
            ),
        ]));
        let mut f = std::fs::File::open(&input.fs_path)
            .map_err(|e| Error::Torrent(format!("open {}: {}", input.fs_path.display(), e)))?;
        let mut remaining = len;
        let mut buf = vec![0u8; 1024 * 1024];
        while remaining > 0 {
            let want = (remaining as usize).min(buf.len());
            let n = std::io::Read::read(&mut f, &mut buf[..want])
                .map_err(|e| Error::Torrent(format!("read {}: {}", input.fs_path.display(), e)))?;
            if n == 0 {
                return Err(Error::Torrent(format!(
                    "unexpected EOF reading {}",
                    input.fs_path.display()
                )));
            }
            remaining -= n as u64;
            let mut consumed = 0usize;
            while consumed < n {
                let space = (piece_len - buffered) as usize;
                let take = (n - consumed).min(space);
                hasher.update(&buf[consumed..consumed + take]);
                buffered += take as u64;
                consumed += take;
                if buffered == piece_len {
                    let d: [u8; 20] = hasher.clone().finalize().into();
                    pieces.push(d);
                    hasher.reset();
                    buffered = 0;
                }
            }
        }
    }
    if buffered > 0 {
        let d: [u8; 20] = hasher.finalize().into();
        pieces.push(d);
    }

    let mut info_pairs: Vec<(Vec<u8>, Value)> = Vec::new();
    if single_file.is_some() {
        info_pairs.push((b"length".to_vec(), Value::Int(total_len as i64)));
        info_pairs.push((b"name".to_vec(), bencode::str(name)));
    } else {
        info_pairs.push((b"files".to_vec(), Value::List(files_value)));
        info_pairs.push((b"name".to_vec(), bencode::str(name)));
    }
    info_pairs.push((b"piece length".to_vec(), Value::Int(piece_len as i64)));
    let mut pieces_flat = Vec::with_capacity(pieces.len() * 20);
    for p in &pieces {
        pieces_flat.extend_from_slice(p);
    }
    info_pairs.push((b"pieces".to_vec(), Value::Str(pieces_flat)));
    if private {
        info_pairs.push((b"private".to_vec(), Value::Int(1)));
    }
    let info_value = bencode::dict(info_pairs);

    let mut root = vec![
        (
            b"created by".to_vec(),
            bencode::str("VelocityDM/1.0"),
        ),
    ];
    if let Some(a) = announce {
        if !a.is_empty() {
            root.push((b"announce".to_vec(), bencode::str(a)));
            let mut all: Vec<String> = vec![a.to_string()];
            for t in extra_trackers {
                if !all.contains(t) {
                    all.push(t.clone());
                }
            }
            if all.len() > 1 {
                root.push((
                    b"announce-list".to_vec(),
                    Value::List(all.into_iter().map(|t| vec![bencode::str(&t)]).map(Value::List).collect()),
                ));
            }
        }
    } else if !extra_trackers.is_empty() {
        root.push((b"announce".to_vec(), bencode::str(&extra_trackers[0])));
    }
    root.push((b"info".to_vec(), info_value));
    let torrent_bytes = bencode::encode(&bencode::dict(root));

    // Re-parse for consistency (also validates piece count math).
    let meta = Metainfo::from_torrent_bytes(&torrent_bytes)?;
    Ok((meta, torrent_bytes))
}

/// Suggested piece length for a total size (progressive like mainstream clients).
pub fn suggest_piece_len(total: u64) -> u64 {
    let mut len = 16 * 1024u64;
    while len * 1000 < total && len < 16 * 1024 * 1024 {
        len *= 2;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_test_torrent(dir: &Path) -> (Vec<u8>, Vec<u8>) {
        let data: Vec<u8> = (0..40_000u32).map(|i| (i % 251) as u8).collect();
        let p = dir.join("data.bin");
        std::fs::write(&p, &data).unwrap();
        let (_meta, bytes) = create_torrent(
            "data.bin",
            &[CreateInput {
                path: vec!["data.bin".to_string()],
                fs_path: p.clone(),
            }],
            16 * 1024,
            Some("http://127.0.0.1:9/announce"),
            &[],
            false,
        )
        .unwrap();
        (bytes, data)
    }

    #[test]
    fn create_and_parse_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let (bytes, data) = write_test_torrent(dir.path());
        let meta = Metainfo::from_torrent_bytes(&bytes).unwrap();
        assert_eq!(meta.name, "data.bin");
        assert_eq!(meta.total_len, 40_000);
        assert_eq!(meta.piece_len, 16 * 1024);
        assert_eq!(meta.piece_count(), 3);
        assert_eq!(meta.trackers, vec!["http://127.0.0.1:9/announce"]);
        // verify piece hashes against a direct computation
        assert_eq!(sha1_20(&data[..16384]), meta.pieces[0]);
        assert_eq!(sha1_20(&data[16384..32768]), meta.pieces[1]);
        assert_eq!(sha1_20(&data[32768..]), meta.pieces[2]);
        // info hash stable across re-parse
        let meta2 = Metainfo::from_torrent_bytes(&bytes).unwrap();
        assert_eq!(meta.info_hash, meta2.info_hash);
    }

    #[test]
    fn multi_file_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("pkg");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.bin"), vec![7u8; 10_000]).unwrap();
        std::fs::write(root.join("sub").join("b.bin"), vec![9u8; 5_000]).unwrap();
        let (_m, bytes) = create_torrent(
            "pkg",
            &[
                CreateInput {
                    path: vec!["a.bin".into()],
                    fs_path: root.join("a.bin"),
                },
                CreateInput {
                    path: vec!["sub".into(), "b.bin".into()],
                    fs_path: root.join("sub").join("b.bin"),
                },
            ],
            8 * 1024,
            Some("http://t/ann"),
            &[],
            false,
        )
        .unwrap();
        let meta = Metainfo::from_torrent_bytes(&bytes).unwrap();
        assert_eq!(meta.files.len(), 2);
        assert_eq!(meta.files[1].path, vec!["sub".to_string(), "b.bin".to_string()]);
        assert_eq!(meta.total_len, 15_000);
        assert_eq!(meta.piece_count(), 2);
        // piece 1 (8192..15000) spans a.bin [8192..10000) then b.bin [0..5000)
        let spans = meta.map_piece_span(1, 0, 6808);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0], (0, 8192, 1808));
        assert_eq!(spans[1], (1, 0, 5000));
        assert_eq!(meta.files_overlapping_piece(1), vec![0, 1]);
        assert_eq!(meta.pieces_overlapping_file(1), vec![1]);
        assert_eq!(meta.piece_len_at(1), 6808);
        assert_eq!(meta.piece_len_at(0), 8192);
    }

    #[test]
    fn piece_len_last() {
        let dir = tempfile::tempdir().unwrap();
        let (bytes, _) = write_test_torrent(dir.path());
        let meta = Metainfo::from_torrent_bytes(&bytes).unwrap();
        assert_eq!(meta.piece_len_at(2), 40_000 - 32_768);
        assert_eq!(meta.piece_len_at(0), 16_384);
    }

    #[test]
    fn suggest_piece_len_progression() {
        assert_eq!(suggest_piece_len(1024), 16 * 1024);
        assert!(suggest_piece_len(50 * 1024 * 1024 * 1024) >= 1024 * 1024);
    }

    #[test]
    fn bad_torrents_rejected() {
        assert!(Metainfo::from_torrent_bytes(b"le").is_err());
        assert!(Metainfo::from_torrent_bytes(b"d4:infod4:name5:f.bine").is_err());
        // piece count mismatch: pieces 20 bytes but length implies 2 pieces
        let bad = b"d4:infod6:lengthi32768e4:name5:f.bin12:piece lengthi16384e6:pieces20:00000000000000000000e";
        assert!(Metainfo::from_torrent_bytes(bad).is_err());
    }
}
