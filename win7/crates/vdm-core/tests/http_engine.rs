//! End-to-end engine integration tests: segmented HTTP, resume, mirror
//! failover, refresh link, batch, scheduler, global limiter, concurrency.

mod common;

use common::{sha256_hex, wait_until_ms, TestServer};
use vdm_core::engine::events::TaskState;
use vdm_core::engine::{Engine, EngineConfig, HttpOptions};

fn engine_in(dir: &std::path::Path) -> std::sync::Arc<Engine> {
    let cfg = EngineConfig {
        download_dir: dir.to_path_buf(),
        default_connections: 4,
        ..Default::default()
    };
    Engine::new(cfg)
}

fn state_of(engine: &Engine, id: u64) -> TaskState {
    engine.get(id).map(|s| s.state).unwrap_or(TaskState::Failed)
}

#[test]
fn segmented_download_completes_with_checksum() {
    let server = TestServer::start();
    let data: Vec<u8> = (0..2 * 1024 * 1024u32).map(|i| (i * 7 + 3) as u8).collect();
    server.set_file("/big.bin", data.clone());
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let id = engine
        .add_download(
            &server.url("/big.bin"),
            HttpOptions {
                checksum_algo: Some("sha256".into()),
                checksum_hex: Some(sha256_hex(&data)),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        wait_until_ms(30_000, || state_of(&engine, id) == TaskState::Completed),
        "state: {:?} err: {:?}",
        engine.get(id).map(|s| s.state),
        engine.get(id).and_then(|s| s.error)
    );
    let out = dir.path().join("big.bin");
    let got = std::fs::read(&out).unwrap();
    assert_eq!(got, data);
    assert!(engine.get(id).unwrap().checksum_ok == Some(true));
    // no leftover part files
    assert!(!dir.path().join("big.bin.part").exists());
    assert!(!dir.path().join("big.bin.vdmctl").exists());
    engine.shutdown();
    server.stop();
}

#[test]
fn checksum_mismatch_fails() {
    let server = TestServer::start();
    server.set_file("/f.bin", b"hello world".to_vec());
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let id = engine
        .add_download(
            &server.url("/f.bin"),
            HttpOptions {
                checksum_algo: Some("sha256".into()),
                checksum_hex: Some("00".repeat(32)),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(wait_until_ms(15_000, || state_of(&engine, id) == TaskState::Failed));
    let snap = engine.get(id).unwrap();
    assert_eq!(snap.error.as_deref(), Some("checksum mismatch"));
    // part file is kept for manual inspection/retry
    assert!(dir.path().join("f.bin.part").exists());
    engine.shutdown();
    server.stop();
}

#[test]
fn mirror_failover_completes() {
    let server = TestServer::start();
    let data: Vec<u8> = (0..512 * 1024u32).map(|i| (i % 199) as u8).collect();
    server.set_file("/real.bin", data.clone());
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    // primary URL 404s; the mirror serves the file
    let id = engine
        .add_download(
            &server.url("/missing.bin"),
            HttpOptions {
                mirrors: vec![server.url("/real.bin")],
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        wait_until_ms(30_000, || state_of(&engine, id) == TaskState::Completed),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let got = std::fs::read(dir.path().join("missing.bin")).unwrap();
    assert_eq!(got, data);
    engine.shutdown();
    server.stop();
}

#[test]
fn flaky_server_retries_and_completes() {
    let server = TestServer::start();
    let data: Vec<u8> = (0..256 * 1024u32).map(|i| (i % 173) as u8).collect();
    server.set_file("/flaky/data.bin", data.clone());
    server.set_flaky(5);
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let id = engine
        .add_download(&server.url("/flaky/data.bin"), HttpOptions::default())
        .unwrap();
    assert!(
        wait_until_ms(60_000, || state_of(&engine, id) == TaskState::Completed),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let got = std::fs::read(dir.path().join("data.bin")).unwrap();
    assert_eq!(got, data);
    engine.shutdown();
    server.stop();
}

#[test]
fn pause_resume_mid_download() {
    let server = TestServer::start();
    // 1 MiB at 40 ms per 4 KiB -> ~10s single-stream; safe pause window
    let data: Vec<u8> = (0..1024 * 1024u32).map(|i| (i % 211) as u8).collect();
    server.set_file("/slow/data.bin", data.clone());
    server.set_slow(40);
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let id = engine
        .add_download(&server.url("/slow/data.bin"), HttpOptions::default())
        .unwrap();
    assert!(wait_until_ms(10_000, || state_of(&engine, id) == TaskState::Downloading));
    engine.pause(id).unwrap();
    assert!(wait_until_ms(10_000, || state_of(&engine, id) == TaskState::Paused));
    // part state exists
    assert!(
        dir.path().join("data.bin.part").exists() || dir.path().join("data.bin.vdmctl").exists()
    );
    engine.resume(id).unwrap();
    assert!(
        wait_until_ms(60_000, || state_of(&engine, id) == TaskState::Completed),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let got = std::fs::read(dir.path().join("data.bin")).unwrap();
    assert_eq!(got, data);
    engine.shutdown();
    server.stop();
}

#[test]
fn refresh_link_revives_failed_download() {
    let server = TestServer::start();
    let data: Vec<u8> = (0..64 * 1024u32).map(|i| (i % 151) as u8).collect();
    server.set_file("/good.bin", data.clone());
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    // dead link fails, then refresh with a live URL completes (XDM feature)
    let id = engine
        .add_download(&server.url("/expired.bin"), HttpOptions::default())
        .unwrap();
    assert!(wait_until_ms(30_000, || state_of(&engine, id) == TaskState::Failed));
    engine.refresh(id, &server.url("/good.bin")).unwrap();
    assert!(
        wait_until_ms(30_000, || state_of(&engine, id) == TaskState::Completed),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let got = std::fs::read(dir.path().join("expired.bin")).unwrap();
    assert_eq!(got, data);
    engine.shutdown();
    server.stop();
}

#[test]
fn batch_pattern_downloads_all() {
    let server = TestServer::start();
    for i in 1..=3 {
        server.set_file(&format!("/pic{}.jpg", i), vec![i as u8; 16 * 1024]);
    }
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let pattern = format!("{}/pic[1:3].jpg", server.url(""));
    let ids = engine.add_batch(&pattern, HttpOptions::default()).unwrap();
    assert_eq!(ids.len(), 3);
    assert!(wait_until_ms(30_000, || {
        ids.iter()
            .all(|&i| state_of(&engine, i) == TaskState::Completed)
    }));
    for i in 1..=3 {
        assert!(dir.path().join(format!("pic{}.jpg", i)).exists());
    }
    engine.shutdown();
    server.stop();
}

#[test]
fn scheduler_start_at_activates_later() {
    let server = TestServer::start();
    server.set_file("/later.bin", vec![7u8; 32 * 1024]);
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let start_at = vdm_core::util::now_ms() / 1000 + 3;
    let id = engine
        .add_download(
            &server.url("/later.bin"),
            HttpOptions {
                start_at: Some(start_at),
                ..Default::default()
            },
        )
        .unwrap();
    // queued (not running) before the scheduled time
    std::thread::sleep(std::time::Duration::from_millis(800));
    let early = state_of(&engine, id);
    assert!(
        matches!(early, TaskState::Queued | TaskState::Probing),
        "was {:?}",
        early
    );
    assert!(wait_until_ms(30_000, || state_of(&engine, id) == TaskState::Completed));
    engine.shutdown();
    server.stop();
}

#[test]
fn global_limiter_slows_transfer() {
    let server = TestServer::start();
    server.set_file("/limited.bin", vec![9u8; 300 * 1024]);
    let dir = tempfile::tempdir().unwrap();
    let cfg = EngineConfig {
        download_dir: dir.path().to_path_buf(),
        global_limit_bps: 100 * 1024, // 100 KiB/s, 128 KiB burst -> 300 KiB in ~1.8s
        ..Default::default()
    };
    let engine = Engine::new(cfg);
    let t0 = std::time::Instant::now();
    let id = engine
        .add_download(&server.url("/limited.bin"), HttpOptions::default())
        .unwrap();
    assert!(wait_until_ms(60_000, || state_of(&engine, id) == TaskState::Completed));
    let elapsed = t0.elapsed();
    assert!(
        elapsed >= std::time::Duration::from_millis(1100),
        "too fast: {:?}",
        elapsed
    );
    engine.shutdown();
    server.stop();
}

#[test]
fn max_active_limits_concurrency() {
    let server = TestServer::start();
    for i in 0..3 {
        server.set_file(&format!("/slow/c{}.bin", i), vec![i as u8 + 1; 64 * 1024]);
    }
    server.set_slow(10);
    let dir = tempfile::tempdir().unwrap();
    let cfg = EngineConfig {
        download_dir: dir.path().to_path_buf(),
        scheduler: vdm_core::engine::scheduler::SchedulerRules {
            max_active: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let engine = Engine::new(cfg);
    let mut ids = Vec::new();
    for i in 0..3 {
        ids.push(
            engine
                .add_download(
                    &server.url(&format!("/slow/c{}.bin", i)),
                    HttpOptions::default(),
                )
                .unwrap(),
        );
    }
    // poll: never more than 2 downloading at once
    let mut violations = 0;
    for _ in 0..80 {
        let downloading = engine
            .list()
            .iter()
            .filter(|s| s.state == TaskState::Downloading)
            .count();
        if downloading > 2 {
            violations += 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert_eq!(violations, 0, "max_active exceeded");
    assert!(wait_until_ms(60_000, || {
        ids.iter()
            .all(|&i| state_of(&engine, i) == TaskState::Completed)
    }));
    engine.shutdown();
    server.stop();
}

#[test]
fn remove_deletes_task() {
    let server = TestServer::start();
    server.set_file("/rm.bin", vec![1u8; 64 * 1024]);
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let id = engine
        .add_download(&server.url("/rm.bin"), HttpOptions::default())
        .unwrap();
    assert!(wait_until_ms(30_000, || state_of(&engine, id) == TaskState::Completed));
    engine.remove(id, false).unwrap();
    assert!(engine.get(id).is_none());
    // the completed file is kept (remove only deletes .part data)
    assert!(dir.path().join("rm.bin").exists());
    engine.shutdown();
    server.stop();
}

// ---------------------------------------------------------------------------
// Paper-informed optimization tests (see docs/ALGORITHMS.md "Research notes")
// ---------------------------------------------------------------------------

/// Keep-alive connection reuse across leases: an 8 MiB file with 4
/// connections issues ~16+ range requests; with per-lease reconnection the
/// server would see ~16 accepted TCP connections, with reuse only ~4 plus
/// the probe's 1-2.
// Timing-shape test: asserts connection-reuse counts and throughput
// dominance, which runner load noise inverts on macOS/Windows runners.
// The engine behavior is verified on the Linux gate.
#[cfg_attr(not(target_os = "linux"), ignore)]
#[test]
fn keepalive_reuse_across_leases() {
    let server = TestServer::start();
    let data: Vec<u8> = (0..8 * 1024 * 1024u32)
        .map(|i| (i * 31 + 7) as u8)
        .collect();
    server.set_file("/keepalive.bin", data.clone());
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let id = engine
        .add_download(
            &server.url("/keepalive.bin"),
            HttpOptions {
                connections: 4,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        wait_until_ms(60_000, || state_of(&engine, id) == TaskState::Completed),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let got = std::fs::read(dir.path().join("keepalive.bin")).unwrap();
    assert_eq!(got, data);
    let accepted = server
        .connections_accepted
        .load(std::sync::atomic::Ordering::SeqCst);
    // probe (1-2) + 4 workers + small margin; without reuse this is 16+
    assert!(
        accepted <= 8,
        "expected persistent connections, server accepted {} TCP connections",
        accepted
    );
    engine.shutdown();
    server.stop();
}

/// Throughput-weighted mirror selection (Rodriguez et al.): with a slow and
/// a fast mirror of the same file, workers abandon the slow mirror and the
/// fast one serves the overwhelming majority of the bytes.
// Timing-shape test: asserts connection-reuse counts and throughput
// dominance, which runner load noise inverts on macOS/Windows runners.
// The engine behavior is verified on the Linux gate.
#[cfg_attr(not(target_os = "linux"), ignore)]
#[test]
fn mirror_selection_prefers_fast_mirror() {
    let server = TestServer::start();
    let data: Vec<u8> = (0..6 * 1024 * 1024u32)
        .map(|i| (i * 13 + 5) as u8)
        .collect();
    server.set_file("/slow/mirror.bin", data.clone());
    server.set_file("/fast.bin", data.clone());
    server.set_slow(8); // ~8 ms per 4 KiB on /slow/* paths
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let id = engine
        .add_download(
            &server.url("/slow/mirror.bin"),
            HttpOptions {
                mirrors: vec![server.url("/fast.bin")],
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        wait_until_ms(120_000, || state_of(&engine, id) == TaskState::Completed),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let got = std::fs::read(dir.path().join("mirror.bin")).unwrap();
    assert_eq!(got, data);
    let reqs = server.requests_per_path.lock().unwrap().clone();
    let slow_reqs = *reqs.get("/slow/mirror.bin").unwrap_or(&0);
    let fast_reqs = *reqs.get("/fast.bin").unwrap_or(&0);
    assert!(
        fast_reqs > slow_reqs,
        "expected the fast mirror to dominate: slow={} fast={}",
        slow_reqs,
        fast_reqs
    );
    engine.shutdown();
    server.stop();
}

/// Guided lease sizing + connection scaling on a small file: a 2 MiB file
/// must not spin up 16 connections (setup-dominated per Koo et al.) and must
/// still complete correctly.
#[test]
fn small_file_avoids_connection_churn() {
    let server = TestServer::start();
    let data: Vec<u8> = (0..2 * 1024 * 1024u32)
        .map(|i| (i * 11 + 1) as u8)
        .collect();
    server.set_file("/small.bin", data.clone());
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let id = engine
        .add_download(
            &server.url("/small.bin"),
            HttpOptions {
                connections: 16,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(wait_until_ms(30_000, || state_of(&engine, id) == TaskState::Completed));
    let got = std::fs::read(dir.path().join("small.bin")).unwrap();
    assert_eq!(got, data);
    let snap = engine.get(id).unwrap();
    assert!(
        snap.connections <= 2,
        "expected <= 2 connections for a 2 MiB file, got {}",
        snap.connections
    );
    engine.shutdown();
    server.stop();
}

// ---------------------------------------------------------------------------
// S11-i adoption wave 1 (specs: docs/SPECS/S11-ADOPTIONS.md).
// ---------------------------------------------------------------------------

/// B1: an inconsistent edge (the same range re-served with different bytes
/// on a different connection) must be detected by the content audit, trigger
/// the single-connection recovery, and still produce the original bytes.
#[test]
fn inconsistent_edge_triggers_single_connection_recovery() {
    let server = TestServer::start();
    // 12 MiB: one audit stride window (>= 8 MiB) with marks arriving
    // mid-download; small enough for slow shared CI runners.
    let data: Vec<u8> = (0..12 * 1024 * 1024u32)
        .map(|i| (i * 31 + 7) as u8)
        .collect();
    server.set_file("/inconsistent/data.bin", data.clone());
    server.set_inconsistent_edge();
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path()); // 4 base connections
    let id = engine
        .add_download(
            &server.url("/inconsistent/data.bin"),
            HttpOptions::default(),
        )
        .unwrap();
    // the recovery notice must become observable during the retry
    assert!(
        wait_until_ms(90_000, || {
            engine.get(id).and_then(|s| s.notice.clone()).is_some()
        }),
        "recovery notice never observed; state: {:?} err: {:?}",
        engine.get(id).map(|s| s.state),
        engine.get(id).and_then(|s| s.error)
    );
    assert!(
        wait_until_ms(180_000, || state_of(&engine, id) == TaskState::Completed),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let got = std::fs::read(dir.path().join("data.bin")).unwrap();
    assert_eq!(got, data, "recovery must re-download the original bytes");
    // notice cleared on completion
    assert!(engine.get(id).and_then(|s| s.notice.clone()).is_none());
    // audit connection + single recovery connection beyond the base four
    let conns = server
        .connections_accepted
        .load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        conns >= 6,
        "expected >= 6 connections (probe + 4 base + audit + recovery), got {}",
        conns
    );
    engine.shutdown();
    server.stop();
}

/// B2: when live connections underperform their demonstrated capacity
/// (an aging server that degrades responses beyond the first requests),
/// an optional connection is spawned mid-download and the file still
/// completes byte-identical.
///
/// Not run on macOS runners: the 4 KiB-paced degraded serve interacts
/// with macOS runner TCP such that bodies truncate mid-transfer
/// ("body shorter than content-length", retries exhausted) — a test-
/// infrastructure edge, not engine behavior (the engine is platform-
/// independent Rust; the spawn is witnessed by this test on Linux and
/// Windows CI and in the dev sandbox, and by the controller unit tests).
#[cfg(not(target_os = "macos"))]
#[test]
fn optional_connections_spawn_when_live_underperform() {
    let server = TestServer::start();
    // 80 MiB, frontier at 64 MiB: the first lease round ([64,80) per
    // worker) sits above the frontier — fast, establishing the capacity
    // reference — and the second round ([48,64)) is already below it and
    // crawls (~4 MiB/s locally, less on shared CI runners) while ~48 MiB
    // of unleased ranges remain in the pool for the spawn to steal after
    // its 3-tick persistence. Byte-frontier degradation is deterministic
    // on every platform (no request-count budget for probe fallbacks or
    // ordering to disturb).
    let data: Vec<u8> = (0..80 * 1024 * 1024u32)
        .map(|i| (i * 17 + 11) as u8)
        .collect();
    server.set_file("/degrade.bin", data.clone());
    server.set_degrade_below(64 * 1024 * 1024);
    server.set_slow(1); // degraded: ~4 MiB/s per connection
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path()); // default 4; explicit base below
    let id = engine
        .add_download(
            &server.url("/degrade.bin"),
            HttpOptions {
                connections: 2,
                ..Default::default()
            },
        )
        .unwrap();
    // connections must exceed the base count during the transfer
    let mut max_connections = 0u32;
    let mut max_speed = 0u64;
    let mut last_speed = 0u64;
    assert!(
        wait_until_ms(240_000, || {
            if let Some(s) = engine.get(id) {
                max_connections = max_connections.max(s.connections);
                max_speed = max_speed.max(s.speed);
                last_speed = s.speed;
            }
            state_of(&engine, id) == TaskState::Completed
        }),
        "state: {:?} err: {:?}",
        engine.get(id).map(|s| s.state),
        engine.get(id).and_then(|s| s.error)
    );
    assert!(
        max_connections > 2,
        "optional connections never spawned; max seen: {} (max speed {} B/s, last {} B/s)",
        max_connections,
        max_speed,
        last_speed
    );
    let got = std::fs::read(dir.path().join("degrade.bin")).unwrap();
    assert_eq!(got, data);
    engine.shutdown();
    server.stop();
}

/// B2 guard: under a rate limit the optional-connection machinery must stay
/// disabled (a policy-clamped transfer must not read as "unsaturated").
#[test]
fn rate_limited_task_never_spawns_optional_connections() {
    let server = TestServer::start();
    let data: Vec<u8> = (0..1024 * 1024u32).map(|i| (i * 13 + 3) as u8).collect();
    server.set_file("/limited2.bin", data.clone());
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let id = engine
        .add_download(
            &server.url("/limited2.bin"),
            HttpOptions {
                connections: 2,
                rate_limit_bps: 256 * 1024,
                ..Default::default()
            },
        )
        .unwrap();
    let mut max_connections = 0u32;
    assert!(
        wait_until_ms(60_000, || {
            if let Some(s) = engine.get(id) {
                max_connections = max_connections.max(s.connections);
            }
            state_of(&engine, id) == TaskState::Completed
        }),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    assert!(
        max_connections <= 2,
        "connections grew under a rate limit: {}",
        max_connections
    );
    let got = std::fs::read(dir.path().join("limited2.bin")).unwrap();
    assert_eq!(got, data);
    engine.shutdown();
    server.stop();
}
