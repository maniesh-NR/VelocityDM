//! DHT (BEP-5) end-to-end: two in-process nodes over loopback UDP — the
//! local mini-DHT harness only; the public DHT is never touched by tests.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vdm_core::bt::dht::{self, Dht};
use vdm_core::bt::magnet::Magnet;
use vdm_core::bt::metainfo::{create_torrent, CreateInput};
use vdm_core::bt::seeder::Seeder;
use vdm_core::bt::tracker::TrackerServer;
use vdm_core::bt::{download_blocking, BtEvent, BtOptions, BtSpec};
use vdm_core::engine::{Engine, EngineConfig};

/// The tests observe the process-global ACTIVE_SOCKETS counter, so the
/// three tests in this file serialize (test binaries run sequentially
/// anyway; other binaries cannot hold DHT sockets of this process).
static SERIAL: Mutex<()> = Mutex::new(());

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

/// Two mini-DHT nodes over loopback: node A holds the peers for the info
/// hash; node B knows only node A as its bootstrap and resolves via
/// get_peers.
#[test]
fn dht_two_node_lookup_e2e() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let a_dir = dir.path().join("a");
    let b_dir = dir.path().join("b");
    std::fs::create_dir_all(&a_dir).unwrap();
    std::fs::create_dir_all(&b_dir).unwrap();
    // node A: no bootstrap of its own, never touches the public network
    let node_a = Dht::start_with_bootstrap(&a_dir, "127.0.0.1:0", Some(Vec::new())).unwrap();
    let hash = [9u8; 20];
    let peer: SocketAddr = "127.0.0.1:55555".parse().unwrap();
    node_a.register(hash, peer);
    // node B: bootstraps from node A only
    let node_b =
        Dht::start_with_bootstrap(&b_dir, "127.0.0.1:0", Some(vec![node_a.local_addr()])).unwrap();
    let found = node_b.lookup_peers(hash, 0, Duration::from_secs(20));
    assert_eq!(found, vec![peer]);
    // the lookup populated B's routing table with node A
    assert_eq!(node_b.table_len(), 1);
    // both nodes learned a persisted identity
    assert!(dht::node_id_path(&a_dir).exists());
    assert!(dht::node_id_path(&b_dir).exists());
    drop(node_b);
    drop(node_a);
    assert!(wait_until_ms(2_000, || dht::ACTIVE_SOCKETS
        .load(Ordering::Relaxed)
        == 0));
    assert_eq!(dht::ACTIVE_SOCKETS.load(Ordering::Relaxed), 0);
}

/// Trackerless magnet: the engine resolves peers AND metadata through the
/// mini-DHT (node A holds the seeder), then completes the torrent — output
/// byte-identical to the source payload.
#[test]
fn trackerless_magnet_download_e2e() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let data = make_data(512 * 1024, 7);
    std::fs::write(src.join("payload.bin"), &data).unwrap();
    // a torrent with NO tracker
    let (meta, _torrent) = create_torrent(
        "payload.bin",
        &[CreateInput {
            path: vec!["payload.bin".to_string()],
            fs_path: src.join("payload.bin"),
        }],
        64 * 1024,
        None,
        &[],
        false,
    )
    .unwrap();
    let meta = Arc::new(meta);
    let seeder = Seeder::start(meta.clone(), &src, "127.0.0.1:0").unwrap();
    // the mini-DHT node A holds the seeder for this info hash
    let a_dir = dir.path().join("node-a");
    std::fs::create_dir_all(&a_dir).unwrap();
    let node_a = Dht::start_with_bootstrap(&a_dir, "127.0.0.1:0", Some(Vec::new())).unwrap();
    node_a.register(meta.info_hash, seeder.addr);
    // trackerless magnet: valid since S4, resolved entirely via DHT
    let magnet = Magnet::from_parts(meta.info_hash, Some("payload.bin".into()), Vec::new());
    assert!(Magnet::parse(&magnet.to_uri()).is_ok());
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut got_metadata = false;
    let result = download_blocking(
        BtSpec::Magnet(magnet),
        &BtOptions {
            out_dir: out.clone(),
            max_peers: 4,
            connect_timeout: Duration::from_secs(5),
            io_timeout: Duration::from_secs(10),
            dht_bootstrap: Some(vec![node_a.local_addr()]),
            ..Default::default()
        },
        &mut |evt: BtEvent| {
            if let BtEvent::Metadata(m) = &evt {
                got_metadata = true;
                assert_eq!(m.info_hash, meta.info_hash);
                assert_eq!(m.total_len, data.len() as u64);
            }
        },
    )
    .unwrap();
    assert!(got_metadata, "metadata event missing");
    let got = std::fs::read(&result.files[0]).unwrap();
    assert_eq!(sha256_hex(&got), sha256_hex(&data));
    // the engine's DHT persisted its node id under out/.vdm/
    assert!(dht::node_id_path(&out).exists());
    seeder.stop();
    drop(node_a);
    assert!(wait_until_ms(2_000, || dht::ACTIVE_SOCKETS
        .load(Ordering::Relaxed)
        == 0));
}

/// The idle no-op promise: an engine with no BT task binds zero DHT sockets,
/// and a torrent-file download (trackers, no magnet) never activates the DHT.
#[test]
fn dht_idle_no_socket_activity() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let before = dht::ACTIVE_SOCKETS.load(Ordering::Relaxed);
    assert_eq!(before, 0);
    // an engine with no BT task at all
    {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            download_dir: dir.path().to_path_buf(),
            ..Default::default()
        });
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(dht::ACTIVE_SOCKETS.load(Ordering::Relaxed), 0);
        engine.shutdown();
        assert_eq!(dht::ACTIVE_SOCKETS.load(Ordering::Relaxed), 0);
    }
    // and a full tracker-based torrent download: DHT never engages
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    let data = make_data(128 * 1024, 3);
    std::fs::write(root.join("payload.bin"), &data).unwrap();
    let tracker = TrackerServer::start("127.0.0.1:0").unwrap();
    let (meta, torrent) = create_torrent(
        "payload.bin",
        &[CreateInput {
            path: vec!["payload.bin".to_string()],
            fs_path: root.join("payload.bin"),
        }],
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
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    let handle = std::thread::spawn(move || {
        let result = download_blocking(
            BtSpec::TorrentBytes(torrent),
            &BtOptions {
                out_dir: out,
                max_peers: 2,
                connect_timeout: Duration::from_secs(5),
                io_timeout: Duration::from_secs(10),
                ..Default::default()
            },
            &mut |_evt| {},
        );
        let _ = result.unwrap();
        flag.store(true, Ordering::Relaxed);
    });
    while !done.load(Ordering::Relaxed) && handle.is_finished() {
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut stayed_idle = true;
    while !done.load(Ordering::Relaxed) {
        if dht::ACTIVE_SOCKETS.load(Ordering::Relaxed) != 0 {
            stayed_idle = false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = handle.join();
    assert!(
        stayed_idle,
        "DHT socket appeared during a torrent-file task"
    );
    assert_eq!(dht::ACTIVE_SOCKETS.load(Ordering::Relaxed), 0);
    seeder.stop();
    tracker.stop();
}

#[allow(dead_code)]
fn unused(_: &Path) {}
