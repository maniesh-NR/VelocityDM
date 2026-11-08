//! HLS end-to-end: master playlist selection, clear + AES-128 streams.

mod common;

use common::{wait_until_ms, TestServer};
use vdm_core::engine::{Engine, EngineConfig, HttpOptions};
use vdm_core::engine::events::TaskState;
use vdm_core::engine::hls::aes128_cbc_decrypt;
use vdm_core::http::{ClientCfg, Url};

fn aes128_cbc_encrypt(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    use aes::cipher::{BlockEncrypt, KeyInit};
    let cipher = aes::Aes128::new(key.into());
    let mut padded = data.to_vec();
    let pad = 16 - padded.len() % 16;
    padded.extend(std::iter::repeat(pad as u8).take(pad));
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);
    let mut enc = padded;
    for chunk in enc.chunks_mut(16) {
        for i in 0..16 {
            chunk[i] ^= prev[i];
        }
        let mut block: aes::cipher::generic_array::GenericArray<u8, aes::cipher::typenum::U16> =
            aes::cipher::generic_array::GenericArray::clone_from_slice(chunk);
        cipher.encrypt_block(&mut block);
        chunk.copy_from_slice(&block);
        prev.copy_from_slice(chunk);
    }
    enc
}

fn make_segment(seed: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

fn setup_hls(server: &TestServer, encrypt: bool) -> (Vec<u8>, String) {
    // 6 segments of ~50 KB each; two variants (big/small)
    let key: [u8; 16] = *b"VelocityDM-HLS!!";
    let _iv: [u8; 16] = [0u8; 16];
    if encrypt {
        server.set_file("/key.bin", key.to_vec());
    }
    let mut stream_hi = Vec::new();
    let mut stream_lo = Vec::new();
    for q in ["hi", "lo"] {
        let mut playlist = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n");
        let big = q == "hi";
        for i in 0..6u32 {
            let seg = make_segment((i % 8) as u8 + 1, if big { 51 * 1024 } else { 17 * 1024 });
            let stored = if encrypt {
                let iv = {
                    let mut v = [0u8; 16];
                    v[8..].copy_from_slice(&(i as u64).to_be_bytes());
                    v
                };
                aes128_cbc_encrypt(&key, &iv, &seg)
            } else {
                seg.clone()
            };
            server.set_file(&format!("/{}/seg{}.ts", q, i), stored);
            playlist.push_str(&format!(
                "#EXTINF:5.0,\n/{}/seg{}.ts\n",
                q,
                i
            ));
            if big {
                stream_hi.extend_from_slice(&seg);
            } else {
                stream_lo.extend_from_slice(&seg);
            }
        }
        playlist.push_str("#EXT-X-ENDLIST\n");
        if encrypt {
            playlist = format!(
                "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-KEY:METHOD=AES-128,URI=\"/key.bin\"\n{}",
                playlist.trim_start_matches("#EXTM3U\n")
            );
        }
        server.set_file(&format!("/{}.m3u8", q), playlist.into_bytes());
    }
    let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=900000\n/hi.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=300000\n/lo.m3u8\n";
    server.set_file("/master.m3u8", master.as_bytes().to_vec());
    (stream_hi, stream_lo.to_vec().is_empty().then(|| String::new()).unwrap_or_default())
}

#[test]
fn hls_clear_best_quality() {
    let server = TestServer::start();
    let (hi, _) = setup_hls(&server, false);
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig {
        download_dir: dir.path().to_path_buf(),
        ..Default::default()
    });
    let id = engine
        .add_download(&server.url("/master.m3u8"), HttpOptions::default())
        .unwrap();
    assert!(
        wait_until_ms(30_000, || engine.get(id).map(|s| s.state) == Some(TaskState::Completed)),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let out = dir.path().join("master.ts");
    assert!(out.exists(), "missing {:?}", out);
    let got = std::fs::read(&out).unwrap();
    assert_eq!(got.len(), hi.len());
    assert_eq!(got, hi);
    engine.shutdown();
    server.stop();
}

#[test]
fn hls_aes128_worst_quality() {
    let server = TestServer::start();
    let (_, _) = setup_hls(&server, true);
    // rebuild expected LO stream (setup returns hi only; recompute here)
    let mut stream_lo = Vec::new();
    for i in 0..6u32 {
        stream_lo.extend_from_slice(&make_segment((i % 8) as u8 + 1, 17 * 1024));
    }
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig {
        download_dir: dir.path().to_path_buf(),
        ..Default::default()
    });
    let id = engine
        .add_download(
            &server.url("/master.m3u8"),
            HttpOptions {
                quality: "worst".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        wait_until_ms(30_000, || engine.get(id).map(|s| s.state) == Some(TaskState::Completed)),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let out = dir.path().join("master.ts");
    let got = std::fs::read(&out).unwrap();
    assert_eq!(got.len(), stream_lo.len());
    assert_eq!(got, stream_lo);
    engine.shutdown();
    server.stop();
}

#[test]
fn hls_live_playlist_rejected() {
    let server = TestServer::start();
    let live = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:5.0,\n/seg0.ts\n";
    server.set_file("/live.m3u8", live.as_bytes().to_vec());
    server.set_file("/seg0.ts", vec![0u8; 1024]);
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig {
        download_dir: dir.path().to_path_buf(),
        ..Default::default()
    });
    let id = engine
        .add_download(&server.url("/live.m3u8"), HttpOptions::default())
        .unwrap();
    assert!(wait_until_ms(15_000, || {
        engine.get(id).map(|s| s.state) == Some(TaskState::Failed)
    }));
    let err = engine.get(id).unwrap().error.unwrap();
    assert!(err.contains("live"), "unexpected error: {}", err);
    engine.shutdown();
    server.stop();
}

#[test]
fn aes_cbc_roundtrip_via_public_api() {
    let key: [u8; 16] = *b"0123456789abcdef";
    let iv = [9u8; 16];
    let plain = b"segment-payload!!".to_vec(); // 17 bytes -> pads to 32
    let enc = aes128_cbc_encrypt(&key, &iv, &plain);
    let mut got = enc.clone();
    aes128_cbc_decrypt(&key, &iv, &mut got).unwrap();
    assert_eq!(got, plain);
}

#[test]
fn playlist_resolution_over_http() {
    let server = TestServer::start();
    server.set_file("/v.m3u8", b"#EXTM3U\n#EXTINF:5.0,\nseg0.ts\n#EXT-X-ENDLIST\n".to_vec());
    server.set_file("/seg0.ts", vec![5u8; 4096]);
    let cfg = ClientCfg::default();
    let url = Url::parse(&server.url("/v.m3u8")).unwrap();
    let (final_url, media) = vdm_core::engine::hls::resolve_playlist(&cfg, &url, "best").unwrap();
    assert_eq!(final_url.to_string_full(), server.url("/v.m3u8"));
    assert_eq!(media.segments.len(), 1);
    assert_eq!(media.segments[0].uri, server.url("/seg0.ts"));
    assert!(media.endlist);
    server.stop();
}
