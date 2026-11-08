//! Seeding server: serves pieces and metadata (BEP-9) to leechers.
//! Used by `velocitydm seed`, by the integration test harness, and doubles as
//! the reference peer implementation.

use crate::bt::metainfo::Metainfo;
use crate::bt::peer::{
    build_ext_handshake, build_metadata_data, handshake, parse_ext_handshake, parse_metadata_msg,
    read_peer_msg, write_peer_msg, PeerMsg,
};

use crate::error::{Error, Result};
use std::io::{Read, Seek, SeekFrom};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Our advertised ut_metadata extension id.
const OUR_UT_METADATA: u8 = 3;
const METADATA_PIECE_LEN: usize = 16 * 1024;

pub struct Seeder {
    pub addr: SocketAddr,
    meta: Arc<Metainfo>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Seeder {
    /// Start seeding `meta` whose files live under `root`
    /// (root/<file.path components>).
    pub fn start(meta: Arc<Metainfo>, root: &Path, bind: &str) -> Result<Seeder> {
        let mut files = Vec::with_capacity(meta.files.len());
        for f in &meta.files {
            let mut p = root.to_path_buf();
            for c in &f.path {
                p.push(c);
            }
            let fh = std::fs::OpenOptions::new()
                .read(true)
                .open(&p)
                .map_err(|e| Error::Torrent(format!("seed input {}: {}", p.display(), e)))?;
            files.push(Arc::new(Mutex::new(fh)));
        }
        let listener = TcpListener::bind(bind)
            .map_err(|e| Error::Torrent(format!("seeder bind {}: {}", bind, e)))?;
        let addr = listener
            .local_addr()
            .map_err(|e| Error::Torrent(format!("seeder local_addr: {}", e)))?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let meta_c = meta.clone();
        let handle = std::thread::spawn(move || {
            listener.set_nonblocking(true).ok();
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let meta = meta_c.clone();
                        let files = files.clone();
                        std::thread::spawn(move || {
                            if let Err(e) = serve_conn(stream, meta, files) {
                                // surface peer-connection failures (tests and
                                // `RUST_LOG`-style debugging); never fatal.
                                eprintln!("vdm-seeder: peer connection ended: {e:?}");
                            }
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Seeder {
            addr,
            meta,
            stop,
            handle: Some(handle),
        })
    }

    /// Convenience: seed a single torrent file on disk; returns (torrent bytes, seeder).
    pub fn seed_file(path: &Path, announce: &str) -> Result<(Seeder, Vec<u8>, Arc<Metainfo>)> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "download".to_string());
        let (meta, torrent) = crate::bt::metainfo::create_torrent(
            &name,
            &[crate::bt::metainfo::CreateInput {
                path: vec![name.clone()],
                fs_path: path.to_path_buf(),
            }],
            crate::bt::metainfo::suggest_piece_len(std::fs::metadata(path)?.len()),
            Some(announce),
            &[],
            false,
        )?;
        let meta = Arc::new(meta);
        let seeder = Seeder::start(
            meta.clone(),
            path.parent().unwrap_or(Path::new(".")),
            "127.0.0.1:0",
        )?;
        Ok((seeder, torrent, meta))
    }

    pub fn info_hash(&self) -> [u8; 20] {
        self.meta.info_hash
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn serve_conn(
    mut stream: TcpStream,
    meta: Arc<Metainfo>,
    files: Vec<Arc<Mutex<std::fs::File>>>,
) -> Result<()> {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let our_id = crate::util::new_peer_id();
    handshake(&mut stream, &meta.info_hash, &our_id, Some(&meta.info_hash))?;
    // full bitfield
    write_peer_msg(
        &mut stream,
        &PeerMsg::Bitfield(vec![true; meta.piece_count()]),
    )?;
    let mut their_ut_metadata: Option<u8> = None;
    loop {
        let msg = read_peer_msg(&mut stream)?;
        match msg {
            PeerMsg::Interested => {
                write_peer_msg(&mut stream, &PeerMsg::Unchoke)?;
            }
            PeerMsg::Request { index, begin, len } => {
                let piece_len = meta.piece_len_at(index);
                if (begin as u64) + (len as u64) > piece_len
                    || len as usize > crate::bt::peer::MAX_MSG_LEN
                {
                    return Err(Error::Torrent("invalid request span".into()));
                }
                let data = read_span(&meta, &files, index, begin as u64, len as u64)?;
                write_peer_msg(&mut stream, &PeerMsg::Piece { index, begin, data })?;
            }
            PeerMsg::Extended { ext_id: 0, payload } => {
                // record their advertised ut_metadata id and answer with ours
                if let Ok(eh) = parse_ext_handshake(&payload) {
                    their_ut_metadata = eh.ut_metadata;
                }
                write_peer_msg(
                    &mut stream,
                    &PeerMsg::Extended {
                        ext_id: 0,
                        payload: build_ext_handshake(OUR_UT_METADATA, meta.info_bytes.len() as u64),
                    },
                )?;
            }
            PeerMsg::Extended { ext_id, payload } if ext_id == OUR_UT_METADATA => {
                if let Ok(MetadataMsgReq::Request { piece }) = parse_metadata_req(&payload) {
                    let start = piece as usize * METADATA_PIECE_LEN;
                    if start >= meta.info_bytes.len() {
                        continue;
                    }
                    let end = (start + METADATA_PIECE_LEN).min(meta.info_bytes.len());
                    let chunk = meta.info_bytes[start..end].to_vec();
                    // reply using THEIR advertised id (BEP-10 direction rule)
                    let reply_id = their_ut_metadata.unwrap_or(1);
                    write_peer_msg(
                        &mut stream,
                        &PeerMsg::Extended {
                            ext_id: reply_id,
                            payload: build_metadata_data(
                                piece,
                                meta.info_bytes.len() as u64,
                                &chunk,
                            ),
                        },
                    )?;
                }
            }
            PeerMsg::KeepAlive
            | PeerMsg::Choke
            | PeerMsg::Unchoke
            | PeerMsg::NotInterested
            | PeerMsg::Have(_)
            | PeerMsg::Bitfield(_)
            | PeerMsg::Port(_) => {}
            PeerMsg::Extended { .. }
            | PeerMsg::Cancel { .. }
            | PeerMsg::Piece { .. }
            | PeerMsg::Unknown { .. } => {}
        }
    }
}

enum MetadataMsgReq {
    Request { piece: u32 },
    Other,
}

fn parse_metadata_req(payload: &[u8]) -> Result<MetadataMsgReq> {
    match parse_metadata_msg(payload)? {
        crate::bt::peer::MetadataMsg::Request { piece } => Ok(MetadataMsgReq::Request { piece }),
        _ => Ok(MetadataMsgReq::Other),
    }
}

/// Read `len` bytes at (piece, begin) across the file layout.
pub fn read_span(
    meta: &Metainfo,
    files: &[Arc<Mutex<std::fs::File>>],
    index: u32,
    begin: u64,
    len: u64,
) -> Result<Vec<u8>> {
    let mut out = vec![0u8; len as usize];
    let mut filled = 0u64;
    for (fi, file_off, n) in meta.map_piece_span(index, begin, len) {
        let mut f = files[fi].lock().unwrap();
        f.seek(SeekFrom::Start(file_off))?;
        let mut remaining = n as usize;
        while remaining > 0 {
            let pos = filled as usize + (n as usize - remaining);
            let got = f.read(&mut out[pos..pos + remaining])?;
            if got == 0 {
                return Err(Error::Torrent("seed file truncated".into()));
            }
            remaining -= got;
        }
        filled += n;
    }
    Ok(out)
}

/// Read whole piece (used by resume verification).
pub fn read_piece(
    meta: &Metainfo,
    files: &[Arc<Mutex<std::fs::File>>],
    index: u32,
) -> Result<Vec<u8>> {
    let len = meta.piece_len_at(index);
    read_span(meta, files, index, 0, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeder_serves_pieces_and_metadata() {
        crate::bt::tracker::tests::flaky_loopback(|| {
            let dir = tempfile::tempdir().unwrap();
            let data: Vec<u8> = (0..50_000u32).map(|i| (i % 249) as u8).collect();
            let p = dir.path().join("blob.bin");
            std::fs::write(&p, &data).unwrap();
            let tracker = crate::bt::tracker::TrackerServer::start("127.0.0.1:0").unwrap();
            let (seeder, _torrent, meta) = Seeder::seed_file(&p, &tracker.url()).unwrap();
            tracker.register(meta.info_hash, seeder.addr);

            // raw client: handshake, bitfield, interested, request two blocks
            use crate::bt::peer::{read_peer_msg, write_peer_msg, PeerMsg};
            let mut c = TcpStream::connect(seeder.addr).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let our_id = crate::util::new_peer_id();
            let hs = handshake(&mut c, &meta.info_hash, &our_id, Some(&meta.info_hash)).unwrap();
            assert!(hs.supports_extensions());
            let bf = match read_peer_msg(&mut c).unwrap() {
                PeerMsg::Bitfield(b) => b,
                other => panic!("expected bitfield, got {:?}", other),
            };
            // bitfield is byte-padded; the first `piece_count` bits must be set
            assert!(bf.len() >= meta.piece_count());
            assert!(bf[..meta.piece_count()].iter().all(|&b| b));
            write_peer_msg(&mut c, &PeerMsg::Interested).unwrap();
            assert!(matches!(read_peer_msg(&mut c).unwrap(), PeerMsg::Unchoke));
            // request piece 0 block 0 (16 KiB)
            write_peer_msg(
                &mut c,
                &PeerMsg::Request {
                    index: 0,
                    begin: 0,
                    len: 16384,
                },
            )
            .unwrap();
            match read_peer_msg(&mut c).unwrap() {
                PeerMsg::Piece {
                    index,
                    begin,
                    data: got,
                } => {
                    assert_eq!((index, begin), (0, 0));
                    assert_eq!(got, &data[..16384]);
                }
                other => panic!("{:?}", other),
            }
            // request a tail block
            let last_piece = (meta.piece_count() - 1) as u32;
            let tail_len = meta.piece_len_at(last_piece);
            write_peer_msg(
                &mut c,
                &PeerMsg::Request {
                    index: last_piece,
                    begin: 0,
                    len: tail_len as u32,
                },
            )
            .unwrap();
            match read_peer_msg(&mut c).unwrap() {
                PeerMsg::Piece { data: got, .. } => {
                    assert_eq!(got.len() as u64, tail_len);
                    assert_eq!(got.as_slice(), &data[data.len() - tail_len as usize..]);
                }
                other => panic!("{:?}", other),
            }
            // metadata exchange: ext handshake then request piece 0
            write_peer_msg(
                &mut c,
                &PeerMsg::Extended {
                    ext_id: 0,
                    payload: crate::bt::peer::build_ext_handshake(1, 0),
                },
            )
            .unwrap();
            match read_peer_msg(&mut c).unwrap() {
                PeerMsg::Extended { ext_id: 0, payload } => {
                    let eh = parse_ext_handshake(&payload).unwrap();
                    assert_eq!(eh.ut_metadata, Some(3));
                    assert_eq!(eh.metadata_size, Some(meta.info_bytes.len() as u64));
                }
                other => panic!("{:?}", other),
            }
            write_peer_msg(
                &mut c,
                &PeerMsg::Extended {
                    ext_id: 3,
                    payload: crate::bt::peer::build_metadata_request(0),
                },
            )
            .unwrap();
            match read_peer_msg(&mut c).unwrap() {
                PeerMsg::Extended { ext_id: 1, payload } => {
                    match parse_metadata_msg_ext(&payload) {
                        crate::bt::peer::MetadataMsg::Data { data, .. } => {
                            let expect = &meta.info_bytes[..(16 * 1024).min(meta.info_bytes.len())];
                            assert_eq!(data, expect.to_vec());
                        }
                        m => panic!("{:?}", m),
                    }
                }
                other => panic!("{:?}", other),
            }
            seeder.stop();
            tracker.stop();
        });
    }

    fn parse_metadata_msg_ext(payload: &[u8]) -> crate::bt::peer::MetadataMsg {
        crate::bt::peer::parse_metadata_msg(payload).unwrap()
    }
}
