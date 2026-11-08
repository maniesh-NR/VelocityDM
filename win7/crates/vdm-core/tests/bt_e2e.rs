//! BitTorrent end-to-end: torrent download, magnet metadata exchange,
//! per-file selection, resume, seeding.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use vdm_core::bt::magnet::Magnet;
use vdm_core::bt::metainfo::{create_torrent, CreateInput, Metainfo};
use vdm_core::bt::seeder::Seeder;
use vdm_core::bt::tracker::TrackerServer;
use vdm_core::bt::{download_blocking, BtEvent, BtOptions, BtSpec};
use vdm_core::engine::events::TaskState;
use vdm_core::engine::{Engine, EngineConfig};

fn wait_until_ms(timeout_ms: u64, pred: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if pred() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    pred()
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}

fn make_data(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u16).wrapping_mul(251).wrapping_add(seed as u16) as u8)
        .collect()
}

fn seed_harness(
    dir: &Path,
    size: usize,
    piece_len: u64,
) -> (TrackerServer, Seeder, Vec<u8>, Vec<u8>, Arc<Metainfo>) {
    let data = make_data(size, 7);
    let path = dir.join("payload.bin");
    std::fs::write(&path, &data).unwrap();
    let tracker = TrackerServer::start("127.0.0.1:0").unwrap();
    let (meta, torrent) = create_torrent(
        "payload.bin",
        &[CreateInput {
            path: vec!["payload.bin".to_string()],
            fs_path: path.clone(),
        }],
        piece_len,
        Some(&tracker.url()),
        &[],
        false,
    )
    .unwrap();
    let meta = Arc::new(meta);
    let seeder = Seeder::start(meta.clone(), dir, "127.0.0.1:0").unwrap();
    tracker.register(meta.info_hash, seeder.addr);
    (tracker, seeder, data, torrent, meta)
}

#[test]
fn torrent_download_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    let (tracker, seeder, data, torrent, meta) = seed_harness(&root, 2 * 1024 * 1024, 64 * 1024);
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut events = 0usize;
    let mut completed = false;
    let result = download_blocking(
        BtSpec::TorrentBytes(torrent),
        &BtOptions {
            out_dir: out.clone(),
            max_peers: 4,
            connect_timeout: std::time::Duration::from_secs(5),
            io_timeout: std::time::Duration::from_secs(10),
            ..Default::default()
        },
        &mut |evt: BtEvent| {
            events += 1;
            if matches!(evt, BtEvent::Completed { .. }) {
                completed = true;
            }
            if let BtEvent::Metadata(m) = &evt {
                assert_eq!(m.info_hash, meta.info_hash);
            }
        },
    )
    .unwrap();
    assert!(completed);
    assert!(events >= 2);
    assert_eq!(result.files.len(), 1);
    let got = std::fs::read(&result.files[0]).unwrap();
    assert_eq!(sha256_hex(&got), sha256_hex(&data));
    assert_eq!(result.total_len, data.len() as u64);
    // state file removed on completion
    assert!(!out.join("payload.bin.vdmctl").exists());
    seeder.stop();
    tracker.stop();
}

#[test]
fn magnet_download_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    let (tracker, seeder, data, _torrent, meta) = seed_harness(&root, 1024 * 1024, 64 * 1024);
    let magnet = Magnet::from_parts(
        meta.info_hash,
        Some("payload.bin".into()),
        vec![tracker.url()],
    );
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut got_metadata = false;
    let result = download_blocking(
        BtSpec::Magnet(magnet),
        &BtOptions {
            out_dir: out.clone(),
            max_peers: 4,
            connect_timeout: std::time::Duration::from_secs(5),
            io_timeout: std::time::Duration::from_secs(10),
            ..Default::default()
        },
        &mut |evt: BtEvent| {
            if let BtEvent::Metadata(m) = &evt {
                got_metadata = true;
                assert_eq!(m.total_len, data.len() as u64);
            }
        },
    )
    .unwrap();
    assert!(got_metadata, "metadata event missing");
    let got = std::fs::read(&result.files[0]).unwrap();
    assert_eq!(sha256_hex(&got), sha256_hex(&data));
    seeder.stop();
    tracker.stop();
}

#[test]
fn multi_file_priority_selection() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("bundle");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let a = make_data(300 * 1024, 1);
    let b = make_data(200 * 1024, 2);
    std::fs::write(root.join("a.bin"), &a).unwrap();
    std::fs::write(root.join("sub").join("b.bin"), &b).unwrap();
    let tracker = TrackerServer::start("127.0.0.1:0").unwrap();
    let (meta, torrent) = create_torrent(
        "bundle",
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
        64 * 1024,
        Some(&tracker.url()),
        &[],
        false,
    )
    .unwrap();
    let meta = Arc::new(meta);
    let seeder = Seeder::start(meta.clone(), &root, "127.0.0.1:0").unwrap();
    tracker.register(meta.info_hash, seeder.addr);
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    // skip b.bin entirely (priority 0)
    let result = download_blocking(
        BtSpec::TorrentBytes(torrent),
        &BtOptions {
            out_dir: out.clone(),
            file_priorities: vec![1, 0],
            max_peers: 4,
            connect_timeout: std::time::Duration::from_secs(5),
            io_timeout: std::time::Duration::from_secs(10),
            ..Default::default()
        },
        &mut |_evt| {},
    )
    .unwrap();
    assert_eq!(result.files.len(), 1);
    let got_a = std::fs::read(out.join("bundle").join("a.bin")).unwrap();
    assert_eq!(sha256_hex(&got_a), sha256_hex(&a));
    // skipped file exists only as far as the shared boundary piece reaches:
    // a.bin is 307200 bytes; 4 pieces * 65536 = 262144, so piece 4 spans
    // a.bin [262144..307200] and b.bin [0..20480] -- those 20480 bytes are
    // required because piece 4 is needed for a.bin.
    let b_path = out.join("bundle").join("sub").join("b.bin");
    assert!(b_path.exists());
    assert_eq!(std::fs::metadata(&b_path).unwrap().len(), 20480);
    let got_b = std::fs::read(&b_path).unwrap();
    assert_eq!(got_b, &b[..20480]);
    seeder.stop();
    tracker.stop();
}

#[test]
fn bt_resume_from_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    // 4 pieces of 64 KiB
    let (tracker, seeder, data, torrent, meta) = seed_harness(&root, 4 * 64 * 1024, 64 * 1024);
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    // simulate a partial previous run: piece 0 already present + state file
    let out_file = out.join("payload.bin");
    let mut partial = vec![0u8; data.len()];
    partial[..64 * 1024].copy_from_slice(&data[..64 * 1024]);
    std::fs::write(&out_file, &partial).unwrap();
    let state = serde_json::json!({
        "info_hash": hex::encode(meta.info_hash),
        "have": [0u32],
        "priorities": [1u8],
    });
    std::fs::write(out.join("payload.bin.vdmctl"), state.to_string()).unwrap();
    let result = download_blocking(
        BtSpec::TorrentBytes(torrent),
        &BtOptions {
            out_dir: out.clone(),
            max_peers: 2,
            connect_timeout: std::time::Duration::from_secs(5),
            io_timeout: std::time::Duration::from_secs(10),
            ..Default::default()
        },
        &mut |_evt| {},
    )
    .unwrap();
    let got = std::fs::read(&result.files[0]).unwrap();
    assert_eq!(sha256_hex(&got), sha256_hex(&data));
    seeder.stop();
    tracker.stop();
}

#[test]
fn seeder_rejects_wrong_info_hash() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    let (tracker, seeder, _data, torrent, _meta) = seed_harness(&root, 64 * 1024, 64 * 1024);
    // corrupt the info hash -> every peer handshake must fail
    let bad = torrent.clone();
    // flip one byte inside the info dict (piece length integer)
    let idx = bad.windows(4).position(|w| w == b"6:len").unwrap_or(0);
    let _ = idx;
    let mut corrupted = BtSpec::TorrentBytes(bad);
    if let BtSpec::TorrentBytes(bytes) = &mut corrupted {
        // flip a digit of the length value inside the info dict
        let pos = bytes
            .windows(9)
            .position(|w| w == b"6:lengthi")
            .expect("length marker");
        bytes[pos + 9] = b'0' + (bytes[pos + 9] - b'0' + 1) % 10;
    }
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let result = download_blocking(
        corrupted,
        &BtOptions {
            out_dir: out.clone(),
            max_peers: 2,
            connect_timeout: std::time::Duration::from_secs(3),
            io_timeout: std::time::Duration::from_secs(5),
            ..Default::default()
        },
        &mut |_evt| {},
    );
    assert!(
        result.is_err(),
        "expected handshake info-hash mismatch to fail"
    );
    seeder.stop();
    tracker.stop();
}

#[test]
fn engine_torrent_task_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    let (tracker, seeder, data, torrent, _meta) = seed_harness(&root, 512 * 1024, 64 * 1024);
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let engine = Engine::new(EngineConfig {
        download_dir: out.clone(),
        ..Default::default()
    });
    let id = engine
        .add_torrent(
            BtSpec::TorrentBytes(torrent),
            Some(out.to_string_lossy().to_string()),
            vec![],
            0,
        )
        .unwrap();
    assert!(wait_until_ms(60_000, || {
        engine.get(id).map(|s| s.state) == Some(TaskState::Completed)
    }));
    let got = std::fs::read(out.join("payload.bin")).unwrap();
    assert_eq!(sha256_hex(&got), sha256_hex(&data));
    let snap = engine.get(id).unwrap();
    assert_eq!(snap.kind, "torrent");
    assert_eq!(snap.size, data.len() as u64);
    engine.shutdown();
    seeder.stop();
    tracker.stop();
}

#[allow(dead_code)]
fn unused(_: SocketAddr) {}
