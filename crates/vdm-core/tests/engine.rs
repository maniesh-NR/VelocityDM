// SPDX-License-Identifier: GPL-3.0-or-later
//! Engine integration suite: exercises the full download pipeline through
//! the public Manager API against in-process misbehaving servers.

mod common;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::{TestServer, TestServerConfig};
use sha2::{Digest, Sha256};
use vdm_core::manager::Manager;
use vdm_core::settings::Settings;
use vdm_core::types::{TaskSpec, TaskState};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vdm-engine-{}-{}-{}",
        tag,
        std::process::id(),
        uniquifier()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn uniquifier() -> u64 {
    use std::sync::atomic::AtomicU64;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn manager_for(dir: &std::path::Path) -> std::sync::Arc<Manager> {
    let mut settings = Settings::default();
    settings.download_dir = dir.to_path_buf();
    settings.refresh_hz = 12;
    settings.max_concurrent_tasks = 4;
    Manager::new(&settings)
}

fn spec(url: &str) -> TaskSpec {
    TaskSpec { url: url.to_string(), resume: false, ..TaskSpec::default() }
}

async fn wait_for_state(manager: &Manager, id: u64, want: TaskState, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(snap) = manager.get(id) {
            if snap.state == want {
                return true;
            }
            if snap.state.is_terminal() && snap.state != want {
                return false;
            }
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    false
}

async fn wait_downloaded(manager: &Manager, id: u64, at_least: u64, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(snap) = manager.get(id) {
            if snap.downloaded >= at_least {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    false
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn test_payload(mib: usize) -> Vec<u8> {
    // Deterministic pseudo-random payload (xorshift), no external entropy.
    let len = mib * 1024 * 1024;
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.extend_from_slice(&state.to_le_bytes());
    }
    out.truncate(len);
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_stream_download_matches_sha256() {
    let dir = temp_dir("single");
    let payload = test_payload(1);
    let server = TestServer::start(TestServerConfig {
        body: payload.clone(),
        accepts_ranges: false,
        ..Default::default()
    })
    .await
    .unwrap();
    let manager = manager_for(&dir);
    let id = manager.add(spec(&server.url("/file.bin"))).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(30)).await);
    let snap = manager.get(id).unwrap();
    assert_eq!(snap.total, Some(payload.len() as u64));
    let final_path = snap.path.unwrap();
    assert_eq!(sha256_hex(&std::fs::read(&final_path).unwrap()), sha256_hex(&payload));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_download_uses_parallel_connections() {
    let dir = temp_dir("segmented");
    let payload = test_payload(4);
    let server = TestServer::start(TestServerConfig { body: payload.clone(), ..Default::default() })
        .await
        .unwrap();
    let manager = manager_for(&dir);
    let mut s = spec(&server.url("/big.bin"));
    s.segments = 8;
    let id = manager.add(s).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(30)).await);
    // Parallelism actually happened and stayed within the cap.
    assert!(server.max_active() >= 2, "expected parallel connections, max was {}", server.max_active());
    assert!(server.max_active() <= 8);
    assert!(server.range_requests.load(Ordering::Relaxed) > 0, "expected ranged requests");
    let final_path = manager.get(id).unwrap().path.unwrap();
    assert_eq!(sha256_hex(&std::fs::read(&final_path).unwrap()), sha256_hex(&payload));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_resume_mid_download() {
    let dir = temp_dir("pause");
    let payload = test_payload(8);
    let server = TestServer::start(TestServerConfig {
        body: payload.clone(),
        throttle_bps: 3 * 1024 * 1024,
        ..Default::default()
    })
    .await
    .unwrap();
    let manager = manager_for(&dir);
    let mut s = spec(&server.url("/video.bin"));
    s.segments = 4;
    let id = manager.add(s).await.unwrap();

    assert!(wait_downloaded(&manager, id, 2 * 1024 * 1024, Duration::from_secs(20)).await);
    manager.pause(id).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Paused, Duration::from_secs(15)).await);

    // Resume state persisted next to the partial file.
    let part = dir.join("video.bin.part");
    assert!(part.exists(), "partial file must exist while paused");
    assert!(part.with_extension("part.vdmctl").exists(), "vdmctl must exist while paused");
    let ctl_bytes = std::fs::read(part.with_extension("part.vdmctl")).unwrap();
    assert!(serde_json::from_slice::<vdm_core::range_pool::RangeCtl>(&ctl_bytes).is_ok());

    manager.resume(id).unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(60)).await);
    let final_path = manager.get(id).unwrap().path.unwrap();
    assert_eq!(sha256_hex(&std::fs::read(&final_path).unwrap()), sha256_hex(&payload));
    assert!(!part.with_extension("part.vdmctl").exists(), "ctl cleaned up on completion");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manager_restart_resumes_from_ctl() {
    let dir = temp_dir("restart");
    let payload = test_payload(8);
    let server = TestServer::start(TestServerConfig {
        body: payload.clone(),
        throttle_bps: 3 * 1024 * 1024,
        ..Default::default()
    })
    .await
    .unwrap();
    let manager = manager_for(&dir);
    let mut s = spec(&server.url("/resume.bin"));
    s.segments = 4;
    let id = manager.add(s).await.unwrap();
    assert!(wait_downloaded(&manager, id, 2 * 1024 * 1024, Duration::from_secs(20)).await);

    // Simulate a daemon restart: pause (persists ctl), drop the manager.
    manager.shutdown().await;
    assert!(wait_for_state(&manager, id, TaskState::Paused, Duration::from_secs(15)).await);
    drop(manager);

    let manager2 = manager_for(&dir);
    let recovered = manager2.scan_and_resume().await;
    assert_eq!(recovered, 1, "restart scan must recover the paused download");
    assert!(wait_for_state(&manager2, 1, TaskState::Completed, Duration::from_secs(60)).await);
    let final_path = manager2.get(1).unwrap().path.unwrap();
    assert_eq!(sha256_hex(&std::fs::read(&final_path).unwrap()), sha256_hex(&payload));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_survive_mid_stream_kills() {
    let dir = temp_dir("kill");
    let payload = test_payload(2);
    let server = TestServer::start(TestServerConfig {
        body: payload.clone(),
        kill_after_bytes: 192 * 1024,
        ..Default::default()
    })
    .await
    .unwrap();
    let manager = manager_for(&dir);
    let mut s = spec(&server.url("/fragile.bin"));
    s.segments = 3;
    let id = manager.add(s).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(60)).await);
    let final_path = manager.get(id).unwrap().path.unwrap();
    assert_eq!(sha256_hex(&std::fs::read(&final_path).unwrap()), sha256_hex(&payload));
    // Retries actually happened: more requests than would suffice once.
    assert!(server.request_count() > 1, "expected retried requests");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mirror_failover_completes_download() {
    let dir = temp_dir("mirror");
    let payload = test_payload(1);
    let dead = TestServer::start(TestServerConfig {
        body: payload.clone(),
        always_kill: true,
        ..Default::default()
    })
    .await
    .unwrap();
    let good = TestServer::start(TestServerConfig { body: payload.clone(), ..Default::default() })
        .await
        .unwrap();
    let manager = manager_for(&dir);
    let mut s = spec(&dead.url("/dead.bin"));
    s.mirrors = vec![good.url("/dead.bin")];
    s.segments = 2;
    let id = manager.add(s).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(60)).await);
    let final_path = manager.get(id).unwrap().path.unwrap();
    assert_eq!(sha256_hex(&std::fs::read(&final_path).unwrap()), sha256_hex(&payload));
    assert!(good.request_count() > 0, "mirror must have served the download");
    assert!(dead.request_count() > 0, "primary must have been attempted");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rate_limit_is_respected() {
    let dir = temp_dir("rate");
    let payload = test_payload(3);
    let server = TestServer::start(TestServerConfig { body: payload.clone(), ..Default::default() })
        .await
        .unwrap();
    let manager = manager_for(&dir);
    let mut s = spec(&server.url("/paced.bin"));
    s.rate_limit_bps = 1024 * 1024; // 1 MiB/s
    s.segments = 1;
    let id = manager.add(s).await.unwrap();
    let start = Instant::now();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(60)).await);
    let elapsed = start.elapsed();
    // 3 MiB at 1 MiB/s minus the ~250 KB seed burst => ~2.7 s floor.
    assert!(
        elapsed >= Duration::from_millis(2200),
        "rate limit not respected: finished in {:?}",
        elapsed
    );
    assert!(elapsed < Duration::from_secs(20), "excessively slow: {:?}", elapsed);
    let final_path = manager.get(id).unwrap().path.unwrap();
    assert_eq!(sha256_hex(&std::fs::read(&final_path).unwrap()), sha256_hex(&payload));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checksum_mismatch_fails_the_task() {
    let dir = temp_dir("checksum");
    let payload = test_payload(1);
    let server = TestServer::start(TestServerConfig { body: payload.clone(), ..Default::default() })
        .await
        .unwrap();
    let manager = manager_for(&dir);
    let mut s = spec(&server.url("/checked.bin"));
    s.checksum = Some(vdm_core::types::ChecksumSpec {
        algorithm: vdm_core::checksum::Algorithm::Sha256,
        expected: "0000000000000000000000000000000000000000000000000000000000000000".into(),
    });
    let id = manager.add(s).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Failed, Duration::from_secs(30)).await);
    let snap = manager.get(id).unwrap();
    assert!(snap.error.unwrap_or_default().contains("checksum mismatch"));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn content_disposition_names_the_file() {
    let dir = temp_dir("disposition");
    let payload = test_payload(1);
    let server = TestServer::start(TestServerConfig {
        body: payload.clone(),
        disposition: Some("attachment; filename=\"release bundle.zip\"".into()),
        ..Default::default()
    })
    .await
    .unwrap();
    let manager = manager_for(&dir);
    let id = manager.add(spec(&server.url("/d/12345"))).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(30)).await);
    let snap = manager.get(id).unwrap();
    assert_eq!(snap.filename, "release bundle.zip");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redirect_is_followed() {
    let dir = temp_dir("redirect");
    let payload = test_payload(1);
    let server = TestServer::start(TestServerConfig {
        body: payload.clone(),
        redirect_from: Some("/moved".into()),
        redirect_to: String::from("/final.bin"),
        ..Default::default()
    })
    .await
    .unwrap();
    let manager = manager_for(&dir);
    let id = manager.add(spec(&server.url("/moved"))).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(30)).await);
    let final_path = manager.get(id).unwrap().path.unwrap();
    assert_eq!(sha256_hex(&std::fs::read(&final_path).unwrap()), sha256_hex(&payload));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_length_download_completes() {
    let dir = temp_dir("open");
    let payload = test_payload(1);
    let server = TestServer::start(TestServerConfig {
        body: payload.clone(),
        accepts_ranges: false,
        suppress_length: true,
        ..Default::default()
    })
    .await
    .unwrap();
    let manager = manager_for(&dir);
    let id = manager.add(spec(&server.url("/stream.bin"))).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(30)).await);
    let final_path = manager.get(id).unwrap().path.unwrap();
    assert_eq!(sha256_hex(&std::fs::read(&final_path).unwrap()), sha256_hex(&payload));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_events_are_coalesced_to_refresh_rate() {
    let dir = temp_dir("events");
    let payload = test_payload(3);
    let server = TestServer::start(TestServerConfig {
        body: payload.clone(),
        throttle_bps: 2 * 1024 * 1024,
        ..Default::default()
    })
    .await
    .unwrap();
    let manager = manager_for(&dir);
    let mut events = manager.subscribe();
    let mut s = spec(&server.url("/coalesced.bin"));
    s.segments = 2;
    let id = manager.add(s).await.unwrap();
    let start = Instant::now();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(30)).await);
    let elapsed = start.elapsed().as_secs_f64();

    let mut progress_events = 0u32;
    while let Ok(event) = events.try_recv() {
        if let vdm_core::Event::Progress(snap) = event {
            if snap.id == id {
                progress_events += 1;
            }
        }
    }
    let hz = manager.refresh_hz() as f64;
    let max_allowed = (hz * elapsed).ceil() as u32 + 10;
    assert!(
        progress_events <= max_allowed,
        "{} progress events in {:.2}s exceeds the {} Hz cap (max {})",
        progress_events,
        elapsed,
        hz,
        max_allowed
    );
    assert!(progress_events >= 3, "expected at least a few snapshots, got {}", progress_events);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_aes128_playlist_downloads_and_decrypts() {
    use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

    let dir = temp_dir("hls");
    // Four plaintext segments; the reference output is their concatenation.
    let segments: Vec<Vec<u8>> = (0..4)
        .map(|i| {
            let mut state: u64 = 0x1000 + i;
            let mut out = Vec::with_capacity(48 * 1024);
            while out.len() < 48 * 1024 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                out.extend_from_slice(&state.to_le_bytes());
            }
            out.truncate(48 * 1024);
            out
        })
        .collect();
    let key = [0x42u8; 16];
    let iv_hex = "000102030405060708090a0b0c0d0e0f";
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&hex::decode(iv_hex).unwrap());

    let mut routes = std::collections::HashMap::new();
    let mut reference = Vec::new();
    let mut media = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:0\n");
    media.push_str(&format!("#EXT-X-KEY:METHOD=AES-128,URI=\"/hls/key.bin\",IV=0x{}\n", iv_hex));
    for (i, seg) in segments.iter().enumerate() {
        let cipher = Aes128CbcEnc::new((&key).into(), (&iv).into()).encrypt_padded_vec_mut::<Pkcs7>(seg);
        routes.insert(format!("/hls/seg{}.ts", i), (cipher, "video/mp2t".to_string()));
        media.push_str(&format!("#EXTINF:9.0,\n/hls/seg{}.ts\n", i));
        reference.extend_from_slice(seg);
    }
    media.push_str("#EXT-X-ENDLIST\n");
    routes.insert("/hls/key.bin".into(), (key.to_vec(), "application/octet-stream".into()));
    routes.insert("/hls/media.m3u8".into(), (media.into_bytes(), "application/vnd.apple.mpegurl".into()));

    let server = TestServer::start(TestServerConfig {
        routes,
        accepts_ranges: false,
        ..Default::default()
    })
    .await
    .unwrap();
    let manager = manager_for(&dir);
    let id = manager.add(spec(&server.url("/hls/media.m3u8"))).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(60)).await);
    let final_path = manager.get(id).unwrap().path.unwrap();
    let produced = std::fs::read(&final_path).unwrap();
    assert_eq!(produced, reference, "decrypted HLS output must match the plaintext reference");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unreachable_mirror_fails_fast() {
    let dir = temp_dir("404");
    let manager = manager_for(&dir);
    let id = manager.add(spec("http://127.0.0.1:1/nope.bin")).await.unwrap();
    assert!(wait_for_state(&manager, id, TaskState::Failed, Duration::from_secs(20)).await);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_tasks_share_pool_and_slots() {
    let dir = temp_dir("concurrent");
    let payload = test_payload(2);
    let server = TestServer::start(TestServerConfig { body: payload.clone(), ..Default::default() })
        .await
        .unwrap();
    let manager = manager_for(&dir);
    let mut ids = Vec::new();
    for i in 0..3 {
        let mut s = spec(&server.url(format!("/conc{}.bin", i).as_str()));
        s.segments = 4;
        s.filename = Some(format!("conc{}.bin", i));
        ids.push(manager.add(s).await.unwrap());
    }
    for id in ids {
        assert!(wait_for_state(&manager, id, TaskState::Completed, Duration::from_secs(60)).await);
    }
    let (connects, reuses) = manager.env().client.pool().stats();
    assert!(
        reuses > 0,
        "connection pool must be reused across workers (connects={}, reuses={})",
        connects,
        reuses
    );
    for id in 1..=3 {
        let snap = manager.get(id).unwrap();
        assert_eq!(sha256_hex(&std::fs::read(snap.path.unwrap()).unwrap()), sha256_hex(&payload));
    }
    std::fs::remove_dir_all(&dir).ok();
}
