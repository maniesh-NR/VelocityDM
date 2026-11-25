//! DASH end-to-end (S6): static manifest + init + media segments served
//! from a loopback test server; produced bytes == init + seg1 + seg2;
//! `done == size`; ≥ 1 intermediate `done` observation (no per-segment
//! event promise — the throttled snapshot path only).

mod common;

use common::{wait_until_ms, TestServer};
use vdm_core::engine::events::TaskState;
use vdm_core::engine::{Engine, EngineConfig, HttpOptions};

fn make_segment(seed: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// Static template manifest: init + two media segments, template-numbered
/// (startNumber 1, duration 2 s, period 4 s). The engine's naming rules
/// strip `.mpd` and force `.mp4`.
#[test]
fn dash_template_manifest_assembles_identical() {
    let server = TestServer::start();
    // segments large enough (and the server slow enough) that the download
    // spans multiple 100 ms throttle windows — the intermediate-progress
    // assertion below is then deterministic, not a race
    server.set_slow(1);
    let init = make_segment(1, 4096);
    let seg1 = make_segment(2, 1_048_576);
    let seg2 = make_segment(3, 1_048_576);
    server.set_file("/v0/init-1000000.mp4", init.clone());
    server.set_file("/slow/v0/seg-1.m4s", seg1.clone());
    server.set_file("/slow/v0/seg-2.m4s", seg2.clone());
    let manifest = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" type=\"static\" mediaPresentationDuration=\"PT4S\" minBufferTime=\"PT2S\">\n\
<Period id=\"p0\" duration=\"PT4S\">\n\
<AdaptationSet contentType=\"video\" mimeType=\"video/mp4\">\n\
<SegmentTemplate initialization=\"v0/init-$Bandwidth$.mp4\" media=\"slow/v0/seg-$Number$.m4s\" duration=\"2\" timescale=\"1\" startNumber=\"1\"/>\n\
<Representation id=\"v0\" bandwidth=\"1000000\"/>\n\
</AdaptationSet>\n\
</Period>\n\
</MPD>\n";
    server.set_file_with_type(
        "/video.mpd",
        manifest.as_bytes().to_vec(),
        "application/dash+xml",
    );

    let mut expected = Vec::new();
    expected.extend_from_slice(&init);
    expected.extend_from_slice(&seg1);
    expected.extend_from_slice(&seg2);

    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig {
        download_dir: dir.path().to_path_buf(),
        ..Default::default()
    });
    let id = engine
        .add_download(&server.url("/video.mpd"), HttpOptions::default())
        .unwrap();
    // ≥ 1 intermediate `done` observation (throttled snapshot path)
    let mut saw_intermediate = false;
    let ok = wait_until_ms(30_000, || {
        if let Some(s) = engine.get(id) {
            if s.state == TaskState::Downloading && s.done > 0 {
                saw_intermediate = true;
            }
            s.state == TaskState::Completed
        } else {
            false
        }
    });
    assert!(ok, "err: {:?}", engine.get(id).and_then(|s| s.error));
    assert!(saw_intermediate, "no intermediate progress observation");
    let out = dir.path().join("video.mp4");
    let got = std::fs::read(&out).unwrap();
    assert_eq!(got.len(), expected.len());
    assert_eq!(got, expected, "DASH assembly is not byte-identical");
    // completion semantics: done catches up to the surfaced size
    let snap = engine.get(id).unwrap();
    assert_eq!(snap.done, snap.size);
    assert_eq!(snap.error, None);
    engine.shutdown();
    server.stop();
}

/// SegmentList manifest with `mediaRange` byte-ranges (RFC 7233
/// `first-last`): the media fetches are 206-or-fail ranged GETs; the
/// exact total is known up-front (all pieces ranged).
#[test]
fn dash_segment_list_ranged_assembles_identical() {
    let server = TestServer::start();
    let init = make_segment(4, 2048);
    let mut resource = init.clone();
    let seg1 = make_segment(5, 30_000);
    let seg2 = make_segment(6, 30_000);
    resource.extend_from_slice(&seg1);
    resource.extend_from_slice(&seg2);
    server.set_file("/list/stream.mp4", resource.clone());
    let init_len = init.len();
    let s1_off = init_len;
    let s1_end = s1_off + seg1.len() - 1;
    let s2_off = s1_end + 1;
    let manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<MPD type=\"static\" mediaPresentationDuration=\"PT4S\">\n\
<Period id=\"p0\" duration=\"PT4S\">\n\
<AdaptationSet contentType=\"video\">\n\
<SegmentList duration=\"2\" timescale=\"1\">\n\
<Initialization sourceURL=\"list/stream.mp4\" range=\"0-{}\"/>\n\
<SegmentURL media=\"list/stream.mp4\" mediaRange=\"{}-{}\"/>\n\
<SegmentURL media=\"list/stream.mp4\" mediaRange=\"{}-{}\"/>\n\
</SegmentList>\n\
<Representation id=\"v0\" bandwidth=\"1000000\"/>\n\
</AdaptationSet>\n\
</Period>\n\
</MPD>\n",
        init_len - 1,
        s1_off,
        s1_end,
        s2_off,
        resource.len() - 1
    );
    server.set_file("/list.mpd", manifest.into_bytes());

    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig {
        download_dir: dir.path().to_path_buf(),
        ..Default::default()
    });
    let id = engine
        .add_download(&server.url("/list.mpd"), HttpOptions::default())
        .unwrap();
    assert!(
        wait_until_ms(30_000, || engine.get(id).map(|s| s.state)
            == Some(TaskState::Completed)),
        "err: {:?}",
        engine.get(id).and_then(|s| s.error)
    );
    let out = dir.path().join("list.mp4");
    let got = std::fs::read(&out).unwrap();
    assert_eq!(got.len(), init.len() + seg1.len() + seg2.len());
    assert_eq!(got, resource, "ranged SegmentList assembly mismatch");
    let snap = engine.get(id).unwrap();
    assert_eq!(snap.done, snap.size);
    engine.shutdown();
    server.stop();
}

/// A live (dynamic) manifest loud-fails with the classified reason.
#[test]
fn dash_dynamic_manifest_fails_loudly() {
    let server = TestServer::start();
    let manifest = "<?xml version=\"1.0\"?>\n<MPD type=\"dynamic\" minimumUpdatePeriod=\"PT2S\">\n<Period duration=\"PT4S\">\n<AdaptationSet contentType=\"video\"><SegmentTemplate media=\"slow/v0/seg-$Number$.m4s\" duration=\"2\"/><Representation id=\"v0\" bandwidth=\"1\"/></AdaptationSet>\n</Period>\n</MPD>\n";
    server.set_file("/live.mpd", manifest.as_bytes().to_vec());
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig {
        download_dir: dir.path().to_path_buf(),
        ..Default::default()
    });
    let id = engine
        .add_download(&server.url("/live.mpd"), HttpOptions::default())
        .unwrap();
    assert!(wait_until_ms(30_000, || engine.get(id).map(|s| s.state)
        == Some(TaskState::Failed)));
    let err = engine.get(id).and_then(|s| s.error).unwrap_or_default();
    assert!(err.contains("dash: live DASH"), "unexpected error: {}", err);
    engine.shutdown();
    server.stop();
}

/// A DRM (ContentProtection) manifest loud-fails; the output file is never
/// a silently-downloaded manifest.
#[test]
fn dash_drm_manifest_fails_loudly() {
    let server = TestServer::start();
    let manifest = "<?xml version=\"1.0\"?>\n<MPD type=\"static\" mediaPresentationDuration=\"PT4S\">\n<Period duration=\"PT4S\">\n<AdaptationSet contentType=\"video\"><ContentProtection schemeIdUri=\"urn:uuid:9a04f079-9840-4286-ab92-e65be0885f95\"/><SegmentTemplate media=\"slow/v0/seg-$Number$.m4s\" duration=\"2\"/><Representation id=\"v0\" bandwidth=\"1\"/></AdaptationSet>\n</Period>\n</MPD>\n";
    server.set_file("/drm.mpd", manifest.as_bytes().to_vec());
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig {
        download_dir: dir.path().to_path_buf(),
        ..Default::default()
    });
    let id = engine
        .add_download(&server.url("/drm.mpd"), HttpOptions::default())
        .unwrap();
    assert!(wait_until_ms(30_000, || engine.get(id).map(|s| s.state)
        == Some(TaskState::Failed)));
    let err = engine.get(id).and_then(|s| s.error).unwrap_or_default();
    assert!(
        err.contains("dash: DRM-protected DASH"),
        "unexpected error: {}",
        err
    );
    engine.shutdown();
    server.stop();
}
