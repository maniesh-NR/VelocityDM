//! BitTorrent download orchestration: torrent/magnet resolution, peer
//! coordination, resume state, progress events.

use crate::bt::magnet::Magnet;
use crate::bt::metainfo::Metainfo;
use crate::bt::peer::{
    build_ext_handshake, build_metadata_request, handshake, parse_ext_handshake,
    parse_metadata_msg, read_peer_msg, write_peer_msg, MetadataMsg, PeerMsg,
};
use crate::bt::pieces::{bitfield_encode, AddOutcome, BlockPick, PieceStore};
use crate::bt::tracker::{announce_all, TrackerEvent};
use crate::error::{Error, Result};
use crate::token_bucket::TokenBucket;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const METADATA_PIECE_LEN: usize = 16 * 1024;
/// Blocks requested in parallel per peer.
const MAX_PIPELINE: usize = 8;

#[derive(Debug, Clone)]
pub enum BtSpec {
    TorrentFile(PathBuf),
    TorrentBytes(Vec<u8>),
    Magnet(Magnet),
}

#[derive(Clone)]
pub struct BtOptions {
    /// Directory that receives the torrent content.
    pub out_dir: PathBuf,
    /// Per-file priorities (0 = skip); empty = include all.
    pub file_priorities: Vec<u8>,
    pub max_peers: usize,
    pub sequential: bool,
    /// Port we announce to trackers (0 = unspecified client).
    pub announce_port: u16,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    /// External cancel/pause flag.
    pub stop: Option<Arc<AtomicBool>>,
    /// Optional shared rate limiter.
    pub rate_limit: Option<Arc<TokenBucket>>,
}

impl Default for BtOptions {
    fn default() -> Self {
        BtOptions {
            out_dir: std::env::temp_dir(),
            file_priorities: Vec::new(),
            max_peers: 16,
            sequential: false,
            announce_port: 0,
            connect_timeout: Duration::from_secs(10),
            io_timeout: Duration::from_secs(30),
            stop: None,
            rate_limit: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum BtEvent {
    Metadata(Arc<Metainfo>),
    Progress {
        done: u64,
        total: u64,
        speed_bps: u64,
        peers: u32,
        seeds: u32,
    },
    Completed {
        files: Vec<PathBuf>,
        total_len: u64,
    },
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct DownloadedTorrent {
    pub info_hash: [u8; 20],
    pub name: String,
    pub files: Vec<PathBuf>,
    pub total_len: u64,
    pub downloaded: u64,
}

#[derive(Serialize, Deserialize)]
struct BtResumeState {
    info_hash: String,
    have: Vec<u32>,
    priorities: Vec<u8>,
}

struct Shared {
    store: Mutex<PieceStore>,
    writers: Vec<Arc<Mutex<std::fs::File>>>,
    done: AtomicBool,
    failed: Mutex<Option<String>>,
    peers: AtomicU32,
    seeds: AtomicU32,
    downloaded: AtomicU64,
    corrupted: AtomicU64,
    rate: Option<Arc<TokenBucket>>,
}

/// Blocking download entry point used by the task engine / CLI.
pub fn download_blocking(
    spec: BtSpec,
    opts: &BtOptions,
    on_event: &mut dyn FnMut(BtEvent),
) -> Result<DownloadedTorrent> {
    // 1) resolve metainfo
    let (meta, trackers): (Arc<Metainfo>, Vec<String>) = match spec {
        BtSpec::TorrentFile(p) => {
            let bytes = std::fs::read(&p)
                .map_err(|e| Error::Torrent(format!("read {}: {}", p.display(), e)))?;
            let m = Metainfo::from_torrent_bytes(&bytes)?;
            let t = m.trackers.clone();
            (Arc::new(m), t)
        }
        BtSpec::TorrentBytes(b) => {
            let m = Metainfo::from_torrent_bytes(&b)?;
            let t = m.trackers.clone();
            (Arc::new(m), t)
        }
        BtSpec::Magnet(mag) => {
            let info_bytes = fetch_metadata(&mag, opts)?;
            let m = Metainfo::from_info_bytes(info_bytes, &mag.trackers)?;
            (Arc::new(m), mag.trackers.clone())
        }
    };
    on_event(BtEvent::Metadata(meta.clone()));
    if trackers.is_empty() {
        return Err(Error::Torrent(
            "no HTTP trackers available (UDP/DHT not supported)".into(),
        ));
    }

    // 2) output layout
    let priorities: Vec<u8> = if opts.file_priorities.is_empty() {
        vec![1; meta.files.len()]
    } else {
        if opts.file_priorities.len() != meta.files.len() {
            return Err(Error::Torrent(format!(
                "{} priorities for {} files",
                opts.file_priorities.len(),
                meta.files.len()
            )));
        }
        opts.file_priorities.clone()
    };
    let (out_paths, state_path) = layout(&opts.out_dir, &meta)?;
    let mut store = PieceStore::new(meta.clone(), priorities.clone(), opts.sequential)?;

    // 3) resume
    let state_path = state_path;
    if let Ok(text) = std::fs::read_to_string(&state_path) {
        if let Ok(st) = serde_json::from_str::<BtResumeState>(&text) {
            if st.info_hash == hex::encode(meta.info_hash) {
                store.restore_have(&st.have);
            }
        }
    } else if out_paths.iter().all(|p| p.exists())
        && out_paths.iter().enumerate().all(|(i, p)| {
            std::fs::metadata(p)
                .map(|m| m.len() >= meta.files[i].length)
                .unwrap_or(false)
        })
    {
        // files exist without state: full hash verification
        let mut prog = |done: u64, total: u64| {
            on_event(BtEvent::Progress {
                done,
                total,
                speed_bps: 0,
                peers: 0,
                seeds: 0,
            });
        };
        let have = verify_existing(&meta, &out_paths, &mut prog);
        store.restore_have(&have);
    }

    // 4) open writers for selected files
    let mut writers = Vec::with_capacity(meta.files.len());
    for (i, f) in meta.files.iter().enumerate() {
        if let Some(parent) = out_paths[i].parent() {
            std::fs::create_dir_all(parent)?;
        }
        if priorities[i] == 0 {
            writers.push(Arc::new(Mutex::new(
                std::fs::OpenOptions::new()
                    .write(true)
                    .read(true)
                    .create(true)
                    .open(&out_paths[i])?,
            )));
            continue;
        }
        let fh = std::fs::OpenOptions::new()
            .write(true)
            .read(true)
            .create(true)
            .open(&out_paths[i])?;
        if fh.metadata()?.len() != f.length {
            fh.set_len(f.length)?;
        }
        writers.push(Arc::new(Mutex::new(fh)));
    }

    let total_needed = store.total_needed_bytes();
    let _ = total_needed;
    let shared = Arc::new(Shared {
        store: Mutex::new(store),
        writers,
        done: AtomicBool::new(false),
        failed: Mutex::new(None),
        peers: AtomicU32::new(0),
        seeds: AtomicU32::new(0),
        downloaded: AtomicU64::new(0),
        corrupted: AtomicU64::new(0),
        rate: opts.rate_limit.clone(),
    });

    let peer_id = crate::util::new_peer_id();

    // 5) announce + spawn peers
    let left = {
        let st = shared.store.lock().unwrap();
        st.needed_bytes()
    };
    let (initial_peers, _ok, failures) = announce_all(
        &trackers,
        &meta.info_hash,
        &peer_id,
        opts.announce_port,
        0,
        0,
        left,
        TrackerEvent::Started,
    );
    if initial_peers.is_empty() {
        let msg = if failures.is_empty() {
            "tracker returned no peers".to_string()
        } else {
            failures.join("; ")
        };
        return Err(Error::Torrent(format!("no peers: {}", msg)));
    }
    let mut attempted: HashSet<std::net::SocketAddr> = HashSet::new();
    let mut spawned = 0usize;
    for addr in initial_peers {
        if spawned >= opts.max_peers {
            break;
        }
        attempted.insert(addr);
        spawn_peer(shared.clone(), meta.clone(), addr, peer_id, opts);
        spawned += 1;
    }

    // 6) monitor loop
    let start = Instant::now();
    let mut last_progress = Instant::now() - Duration::from_secs(1);
    let mut last_speed_sample = (Instant::now(), 0u64);
    let mut speed_ema = 0u64;
    let mut last_reannounce = Instant::now();
    let mut last_state_save = Instant::now();
    let mut last_peer_activity = Instant::now();
    let result = loop {
        if let Some(stop) = &opts.stop {
            if stop.load(Ordering::Relaxed) {
                save_state(&shared, &state_path, &meta, &priorities);
                announce_all(
                    &trackers,
                    &meta.info_hash,
                    &peer_id,
                    opts.announce_port,
                    0,
                    shared.downloaded.load(Ordering::Relaxed),
                    0,
                    TrackerEvent::Stopped,
                );
                break Err(Error::Task("cancelled".into()));
            }
        }
        {
            let st = shared.store.lock().unwrap();
            if st.all_needed_have() {
                break Ok(());
            }
            if let Some(err) = shared.failed.lock().unwrap().clone() {
                break Err(Error::Task(err));
            }
        }
        if shared.peers.load(Ordering::Relaxed) > 0 {
            last_peer_activity = Instant::now();
        }
        // give up if no peers for a long time
        if last_peer_activity.elapsed() > Duration::from_secs(120)
            && start.elapsed() > Duration::from_secs(120)
        {
            save_state(&shared, &state_path, &meta, &priorities);
            break Err(Error::Torrent("no reachable peers".into()));
        }
        // periodic re-announce to pick up new peers
        if last_reannounce.elapsed() > Duration::from_secs(30)
            && shared.peers.load(Ordering::Relaxed) < opts.max_peers as u32
        {
            last_reannounce = Instant::now();
            let left = {
                let st = shared.store.lock().unwrap();
                st.needed_bytes()
            };
            let dl = shared.downloaded.load(Ordering::Relaxed);
            let (peers, _, _) = announce_all(
                &trackers,
                &meta.info_hash,
                &peer_id,
                opts.announce_port,
                0,
                dl,
                left,
                TrackerEvent::Periodic,
            );
            for addr in peers {
                if !attempted.contains(&addr)
                    && shared.peers.load(Ordering::Relaxed) < opts.max_peers as u32
                {
                    attempted.insert(addr);
                    spawn_peer(shared.clone(), meta.clone(), addr, peer_id, opts);
                }
            }
        }
        // periodic state save
        if last_state_save.elapsed() > Duration::from_secs(5) {
            last_state_save = Instant::now();
            save_state(&shared, &state_path, &meta, &priorities);
        }
        // progress events (~4 Hz)
        if last_progress.elapsed() >= Duration::from_millis(250) {
            last_progress = Instant::now();
            let (done, total) = {
                let st = shared.store.lock().unwrap();
                (
                    st.total_needed_bytes() - st.needed_bytes(),
                    st.total_needed_bytes(),
                )
            };
            let now = Instant::now();
            let (pt, pv) = last_speed_sample;
            let inst = if now > pt {
                ((done - pv) as f64 / now.duration_since(pt).as_secs_f64()) as u64
            } else {
                0
            };
            last_speed_sample = (now, done);
            speed_ema = if speed_ema == 0 {
                inst
            } else {
                (speed_ema * 7 + inst) / 8
            };
            on_event(BtEvent::Progress {
                done,
                total,
                speed_bps: speed_ema,
                peers: shared.peers.load(Ordering::Relaxed),
                seeds: shared.seeds.load(Ordering::Relaxed),
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    shared.done.store(true, Ordering::Relaxed);
    match result {
        Ok(()) => {
            let _ = std::fs::remove_file(&state_path);
            let dl = shared.downloaded.load(Ordering::Relaxed);
            announce_all(
                &trackers,
                &meta.info_hash,
                &peer_id,
                opts.announce_port,
                0,
                dl,
                0,
                TrackerEvent::Completed,
            );
            announce_all(
                &trackers,
                &meta.info_hash,
                &peer_id,
                opts.announce_port,
                0,
                dl,
                0,
                TrackerEvent::Stopped,
            );
            let selected: Vec<PathBuf> = out_paths
                .iter()
                .enumerate()
                .filter(|(i, _)| priorities[*i] > 0)
                .map(|(_, p)| p.clone())
                .collect();
            on_event(BtEvent::Completed {
                files: selected.clone(),
                total_len: meta.total_len,
            });
            Ok(DownloadedTorrent {
                info_hash: meta.info_hash,
                name: meta.name.clone(),
                files: selected,
                total_len: meta.total_len,
                downloaded: dl,
            })
        }
        Err(e) => {
            save_state(&shared, &state_path, &meta, &priorities);
            announce_all(
                &trackers,
                &meta.info_hash,
                &peer_id,
                opts.announce_port,
                0,
                shared.downloaded.load(Ordering::Relaxed),
                0,
                TrackerEvent::Stopped,
            );
            Err(e)
        }
    }
}

fn layout(out_dir: &Path, meta: &Metainfo) -> Result<(Vec<PathBuf>, PathBuf)> {
    std::fs::create_dir_all(out_dir)?;
    let name = crate::util::sanitize_filename(&meta.name);
    let mut paths = Vec::with_capacity(meta.files.len());
    if meta.files.len() == 1 && meta.files[0].path.len() == 1 {
        let p = out_dir.join(name.clone());
        paths.push(p.clone());
        let state = out_dir.join(format!("{}.vdmctl", name));
        Ok((paths, state))
    } else {
        let root = out_dir.join(name.clone());
        for f in &meta.files {
            let mut p = root.clone();
            for c in &f.path {
                p.push(crate::util::sanitize_filename(c));
            }
            paths.push(p);
        }
        let state = root.with_file_name(format!("{}.vdmctl", name));
        Ok((paths, state))
    }
}

fn save_state(shared: &Arc<Shared>, path: &Path, meta: &Metainfo, priorities: &[u8]) {
    let have: Vec<u32> = {
        let st = shared.store.lock().unwrap();
        (0..st.have.len() as u32)
            .filter(|&i| st.have[i as usize])
            .collect()
    };
    let state = BtResumeState {
        info_hash: hex::encode(meta.info_hash),
        have,
        priorities: priorities.to_vec(),
    };
    if let Ok(text) = serde_json::to_string(&state) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }
}

/// Hash-verify existing output files to rebuild the have-set on resume.
pub fn verify_existing(
    meta: &Metainfo,
    out_paths: &[PathBuf],
    progress: &mut dyn FnMut(u64, u64),
) -> Vec<u32> {
    let mut handles = Vec::with_capacity(meta.files.len());
    for p in out_paths {
        handles.push(std::fs::OpenOptions::new().read(true).open(p).ok());
    }
    let files: Vec<Arc<Mutex<std::fs::File>>> = handles
        .into_iter()
        .map(|h| {
            Arc::new(Mutex::new(h.unwrap_or_else(|| {
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(std::env::temp_dir().join(".vdm-missing"))
                    .expect("open fallback")
            })))
        })
        .collect();
    let mut have = Vec::new();
    let total = meta.piece_count() as u64;
    for i in 0..meta.piece_count() as u32 {
        if let Ok(data) = crate::bt::seeder::read_piece(meta, &files, i) {
            if crate::bt::metainfo::sha1_20(&data) == meta.pieces[i as usize] {
                have.push(i);
            }
        }
        progress(i as u64 + 1, total);
    }
    have
}

fn spawn_peer(
    shared: Arc<Shared>,
    meta: Arc<Metainfo>,
    addr: std::net::SocketAddr,
    peer_id: [u8; 20],
    opts: &BtOptions,
) {
    let io_timeout = opts.io_timeout;
    let connect_timeout = opts.connect_timeout;
    std::thread::spawn(move || {
        let _ = peer_thread(shared, meta, addr, peer_id, io_timeout, connect_timeout);
    });
}

fn peer_thread(
    shared: Arc<Shared>,
    meta: Arc<Metainfo>,
    addr: std::net::SocketAddr,
    our_id: [u8; 20],
    io_timeout: Duration,
    connect_timeout: Duration,
) -> Result<()> {
    let sockaddr = std::net::SocketAddr::from(addr);
    let stream = TcpStream::connect_timeout(&sockaddr, connect_timeout)?;
    let _ = stream.set_nodelay(true);
    stream.set_read_timeout(Some(io_timeout))?;
    stream.set_write_timeout(Some(io_timeout))?;
    let mut stream = stream;
    handshake(&mut stream, &meta.info_hash, &our_id, Some(&meta.info_hash))?;
    write_peer_msg(&mut stream, &PeerMsg::Interested)?;
    shared.peers.fetch_add(1, Ordering::Relaxed);

    let piece_count = meta.piece_count();
    let mut peer_bits: Vec<bool> = vec![false; piece_count];
    let mut choked = true;
    let mut outstanding: VecDeque<(u32, u32, u32)> = VecDeque::new();

    let res = peer_loop(
        &mut stream,
        &shared,
        &meta,
        piece_count,
        &mut peer_bits,
        &mut choked,
        &mut outstanding,
    );

    shared.peers.fetch_sub(1, Ordering::Relaxed);
    if !peer_bits.is_empty() && peer_bits.iter().all(|&b| b) {
        shared.seeds.fetch_sub(1, Ordering::Relaxed);
    }
    // remove this peer's availability
    let bf = bitfield_encode(&peer_bits);
    shared.store.lock().unwrap().peer_remove(Some(&bf));
    res
}

fn peer_loop(
    stream: &mut TcpStream,
    shared: &Arc<Shared>,
    meta: &Arc<Metainfo>,
    piece_count: usize,
    peer_bits: &mut Vec<bool>,
    choked: &mut bool,
    outstanding: &mut VecDeque<(u32, u32, u32)>,
) -> Result<()> {
    loop {
        if shared.done.load(Ordering::Relaxed) {
            return Ok(());
        }
        {
            let st = shared.store.lock().unwrap();
            if st.all_needed_have() {
                return Ok(());
            }
        }
        // pipeline fill
        if !*choked {
            while outstanding.len() < MAX_PIPELINE {
                let pick = {
                    let bits = peer_bits.clone();
                    let mut st = shared.store.lock().unwrap();
                    st.next_block(&|p: u32| bits.get(p as usize).copied().unwrap_or(false))
                };
                match pick {
                    BlockPick::Request { piece, offset, len } => {
                        write_peer_msg(
                            stream,
                            &PeerMsg::Request {
                                index: piece,
                                begin: offset,
                                len,
                            },
                        )?;
                        outstanding.push_back((piece, offset, len));
                    }
                    BlockPick::None => break,
                }
            }
        }
        let msg = match read_peer_msg(stream) {
            Ok(m) => m,
            Err(Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if outstanding.is_empty() {
                    write_peer_msg(stream, &PeerMsg::KeepAlive)?;
                    continue;
                }
                return Err(Error::Torrent("peer timed out with pending blocks".into()));
            }
            Err(e) => return Err(e),
        };
        match msg {
            PeerMsg::Bitfield(bits) => {
                if bits.len() == piece_count {
                    *peer_bits = bits;
                } else if bits.len() > piece_count {
                    *peer_bits = bits[..piece_count].to_vec();
                } else {
                    for (i, b) in bits.iter().enumerate() {
                        peer_bits[i] = *b;
                    }
                }
                let bf = bitfield_encode(peer_bits);
                shared.store.lock().unwrap().peer_add(Some(&bf));
                if !peer_bits.is_empty() && peer_bits.iter().all(|&b| b) {
                    shared.seeds.fetch_add(1, Ordering::Relaxed);
                }
            }
            PeerMsg::Have(i) => {
                if (i as usize) < piece_count {
                    peer_bits[i as usize] = true;
                    shared.store.lock().unwrap().peer_have(i);
                }
            }
            PeerMsg::Choke => *choked = true,
            PeerMsg::Unchoke => *choked = false,
            PeerMsg::Piece { index, begin, data } => {
                outstanding.retain(|&(pi, off, len)| {
                    !(pi == index && off == begin && len as u32 == data.len() as u32)
                });
                if let Some(bucket) = &shared.rate {
                    bucket.wait_for(data.len() as u64);
                }
                let outcome = {
                    let mut st = shared.store.lock().unwrap();
                    st.add_block(index, begin, &data)
                };
                match outcome {
                    AddOutcome::Complete(piece_data) => {
                        if let Err(e) = write_piece(shared, meta, index, &piece_data) {
                            *shared.failed.lock().unwrap() = Some(format!("write failed: {}", e));
                            return Err(e);
                        }
                        let mut st = shared.store.lock().unwrap();
                        st.mark_have(index);
                        shared
                            .downloaded
                            .fetch_add(piece_data.len() as u64, Ordering::Relaxed);
                    }
                    AddOutcome::Corrupted => {
                        shared.corrupted.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            PeerMsg::KeepAlive => {}
            PeerMsg::Extended { ext_id: 0, payload } => {
                // polite reply advertising no extensions we serve
                let _ = payload;
                let _ = write_peer_msg(
                    stream,
                    &PeerMsg::Extended {
                        ext_id: 0,
                        payload: build_ext_handshake(0, 0),
                    },
                );
            }
            _ => {}
        }
    }
}

fn write_piece(shared: &Arc<Shared>, meta: &Arc<Metainfo>, index: u32, data: &[u8]) -> Result<()> {
    let mut data_off = 0u64;
    for (fi, file_off, n) in meta.map_piece_span(index, 0, data.len() as u64) {
        let w = &shared.writers[fi];
        let mut f = w.lock().unwrap();
        f.seek(SeekFrom::Start(file_off))?;
        f.write_all(&data[data_off as usize..(data_off + n) as usize])?;
        data_off += n;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Magnet metadata fetch (BEP-9)
// ---------------------------------------------------------------------------

/// Our advertised ut_metadata id (we can also answer requests, but we only
/// request metadata while downloading).
const OUR_UT_METADATA: u8 = 1;

fn fetch_metadata(magnet: &Magnet, opts: &BtOptions) -> Result<Vec<u8>> {
    let peer_id = crate::util::new_peer_id();
    let (peers, _ok, failures) = announce_all(
        &magnet.trackers,
        &magnet.info_hash,
        &peer_id,
        opts.announce_port,
        0,
        0,
        0,
        TrackerEvent::Started,
    );
    let candidates: Vec<std::net::SocketAddr> = peers.into_iter().take(8).collect();
    if candidates.is_empty() {
        return Err(Error::Torrent(format!(
            "magnet: no peers from trackers{}",
            if failures.is_empty() {
                String::new()
            } else {
                format!(" ({})", failures.join("; "))
            }
        )));
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    for addr in &candidates {
        if Instant::now() > deadline {
            break;
        }
        if let Ok(info) = try_metadata_from_peer(*addr, &magnet.info_hash, &peer_id, opts) {
            if crate::bt::metainfo::sha1_20(&info) == magnet.info_hash {
                return Ok(info);
            }
        }
    }
    Err(Error::Torrent(
        "magnet: no peer supplied valid metadata".into(),
    ))
}

#[allow(unused_assignments)]
fn try_metadata_from_peer(
    addr: std::net::SocketAddr,
    info_hash: &[u8; 20],
    our_id: &[u8; 20],
    opts: &BtOptions,
) -> Result<Vec<u8>> {
    let stream = TcpStream::connect_timeout(&addr, opts.connect_timeout)?;
    let _ = stream.set_nodelay(true);
    let mut stream = stream;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    handshake(&mut stream, info_hash, our_id, Some(info_hash))?;
    // advertise our extension mapping
    write_peer_msg(
        &mut stream,
        &PeerMsg::Extended {
            ext_id: 0,
            payload: build_ext_handshake(OUR_UT_METADATA, 0),
        },
    )?;
    write_peer_msg(&mut stream, &PeerMsg::Interested)?;
    let mut their_ut_metadata: Option<u8> = None;
    let mut metadata_size: Option<u64> = None;
    let mut chunks: HashMap<u32, Vec<u8>> = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if Instant::now() > deadline {
            return Err(Error::Torrent("metadata exchange timeout".into()));
        }
        let msg = read_peer_msg(&mut stream)?;
        match msg {
            PeerMsg::Extended { ext_id: 0, payload } => {
                let eh = parse_ext_handshake(&payload)?;
                their_ut_metadata = eh.ut_metadata;
                metadata_size = eh.metadata_size;
                if let (Some(id), Some(size)) = (their_ut_metadata, metadata_size) {
                    let pieces = (size as usize + METADATA_PIECE_LEN - 1) / METADATA_PIECE_LEN;
                    for p in 0..pieces {
                        write_peer_msg(
                            &mut stream,
                            &PeerMsg::Extended {
                                ext_id: id,
                                payload: build_metadata_request(p as u32),
                            },
                        )?;
                    }
                }
            }
            PeerMsg::Extended { ext_id, payload } if ext_id == OUR_UT_METADATA => {
                if let Ok(MetadataMsg::Data {
                    piece,
                    total_size,
                    data,
                }) = parse_metadata_msg(&payload)
                {
                    metadata_size = Some(total_size);
                    chunks.insert(piece, data);
                    if let Some(size) = metadata_size {
                        let pieces = (size as usize + METADATA_PIECE_LEN - 1) / METADATA_PIECE_LEN;
                        if (0..pieces as u32).all(|p| chunks.contains_key(&p)) {
                            let mut info = Vec::with_capacity(size as usize);
                            for p in 0..pieces as u32 {
                                info.extend_from_slice(&chunks[&p]);
                            }
                            info.truncate(size as usize);
                            return Ok(info);
                        }
                    }
                }
            }
            PeerMsg::Bitfield(_)
            | PeerMsg::Have(_)
            | PeerMsg::Unchoke
            | PeerMsg::Choke
            | PeerMsg::KeepAlive => {}
            _ => {}
        }
    }
}

/// Progress info helper for UIs.
pub fn spec_label(spec: &BtSpec) -> String {
    match spec {
        BtSpec::TorrentFile(p) => format!("torrent:{}", p.display()),
        BtSpec::TorrentBytes(_) => "torrent:bytes".to_string(),
        BtSpec::Magnet(m) => format!("magnet:{}", hex::encode(m.info_hash)),
    }
}
