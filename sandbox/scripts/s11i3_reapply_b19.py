#!/usr/bin/env python3
"""Re-apply the B19 bits onto the committed B17+B18 state (commit 5c61982),
for the per-item commit B. Non-empty anchors only (no empty-string replaces).

Usage: s11i3_reapply_b19.py   (run from the repo root)
"""
from pathlib import Path

ROOT = Path("/home/z/my-project/velocitydm-repo")
TREES = [ROOT / "crates/vdm-core", ROOT / "win7/crates/vdm-core"]
EMDASH = "\u2014"

STATUS_FN = (
    '\n/// S11-i B19: user-facing status classification. 403/410 carry a stable\n'
    '/// marker the frontends\' note channel acts on; every other status keeps its\n'
    '/// historical exact string (tests and callers depend on them).\n'
    'const REFRESHABLE_MARKER: &str = "link may be expired or forbidden";\n\n'
    'fn status_failure_reason(status: u16) -> String {\n'
    '    if status == 403 || status == 410 {\n'
    '        format!(\n'
    '            "HTTP {} ({} ' + EMDASH + ' re-open the source page and re-grab the link)",\n'
    '            status, REFRESHABLE_MARKER\n'
    '        )\n'
    '    } else {\n'
    '        format!("HTTP {}", status)\n'
    '    }\n'
    '}\n'
)

NOTICE_BLOCK = (
    '        // S11-i B19: refreshable failures surface an actionable hint on the\n'
    '        // EXISTING note channel (every frontend renders notes; no new\n'
    '        // surface, no new permission, no RPC change).\n'
    '        if msg.contains(REFRESHABLE_MARKER) {\n'
    '            s.notice = Some(\n'
    '                "This link looks expired or forbidden ' + EMDASH + ' open the source page, re-grab the link (extension grab-links or the page context), then add the fresh address."\n'
    '                    .to_string(),\n'
    '            );\n'
    '        }\n'
)

UNIT_TEST = (
    '\n    #[test]\n'
    '    fn refreshable_status_classification() {\n'
    '        for s in [403u16, 410] {\n'
    '            let msg = status_failure_reason(s);\n'
    '            assert!(msg.contains(REFRESHABLE_MARKER), "{}", msg);\n'
    '            assert!(msg.starts_with("HTTP "));\n'
    '        }\n'
    '        // other statuses keep their historical exact strings\n'
    '        assert_eq!(status_failure_reason(404), "HTTP 404");\n'
    '        assert_eq!(status_failure_reason(500), "HTTP 500");\n'
    '        assert_eq!(status_failure_reason(503), "HTTP 503");\n'
    '        assert!(is_retryable(&Error::Http("HTTP 503".into())));\n'
    '        assert!(!is_retryable(&Error::Http(status_failure_reason(403))));\n'
    '    }\n'
)

E2E_TEST = (
    '\n#[test]\n'
    'fn b19_expired_link_classifies_and_notices() {\n'
    '    let server = TestServer::start();\n'
    '    server.set_file("/b19.bin", b"payload".to_vec());\n'
    '    server.b19_forbid.store(true, std::sync::atomic::Ordering::SeqCst);\n'
    '    let dir = tempfile::tempdir().unwrap();\n'
    '    let engine = engine_in(dir.path());\n'
    '    let id = engine\n'
    '        .add_download(&server.url("/b19.bin"), HttpOptions::default())\n'
    '        .unwrap();\n'
    '    assert!(wait_until_ms(15_000, || state_of(&engine, id) == TaskState::Failed));\n'
    '    let snap = engine.get(id).unwrap();\n'
    '    let err = snap.error.as_deref().unwrap_or_default();\n'
    '    assert!(err.contains("HTTP 403"), "error: {:?}", snap.error);\n'
    '    assert!(\n'
    '        err.contains("link may be expired or forbidden"),\n'
    '        "error: {:?}",\n'
    '        snap.error\n'
    '    );\n'
    '    // the actionable hint rides the EXISTING note channel\n'
    '    let notice = snap.notice.as_deref().unwrap_or_default();\n'
    '    assert!(notice.contains("re-grab the link"), "notice: {:?}", snap.notice);\n'
    '    engine.shutdown();\n'
    '    server.stop();\n'
    '}\n'
)

B19_FIELD_DOC = (
    '    /// S11-i B19 e2e: when armed, HEAD and ranged requests to "/b19.bin"\n'
    '    /// return 403 (the expired/forbidden-link simulation).\n'
    '    pub b19_forbid: Arc<AtomicBool>,\n'
)

B19_USAGE = (
    '        // S11-i B19 e2e: expired/forbidden link simulation\n'
    '        if path == "/b19.bin"\n'
    '            && b19_forbid.load(Ordering::SeqCst)\n'
    '            && (method == "HEAD" || range.is_some())\n'
    '        {\n'
    '            write_simple(&mut stream, 403, "Forbidden", b"forbidden")?;\n'
    '            continue;\n'
    '        }\n'
)


def sub(path, old, new, must=True):
    text = path.read_text(encoding="utf-8")
    if old in text:
        path.write_text(text.replace(old, new, 1), encoding="utf-8")
        return True
    if must:
        raise SystemExit(f"ANCHOR MISSING in {path}: {old[:70]!r}")
    return False


def main():
    n = 0
    for base in TREES:
        t = base / "src/engine/task.rs"
        sub(t, '        Some(format!("HTTP {}", head_status))',
            '        Some(status_failure_reason(head_status))'); n += 1
        sub(t, '                    fatal = Some(format!("HTTP {}", r2.status));',
            '                    fatal = Some(status_failure_reason(r2.status));'); n += 1
        sub(t, '                    return WorkerExit::Failed(format!("server returned {}", code));',
            '                    return WorkerExit::Failed(format!(\n'
            '                        "server returned {}",\n'
            '                        status_failure_reason(code)\n'
            '                    ));'); n += 1
        sub(t, '        s.error = Some(msg.to_string());\n        s.speed = 0;\n    });',
            '        s.error = Some(msg.to_string());\n        s.speed = 0;\n'
            + NOTICE_BLOCK + '    });'); n += 1
        sub(t, 'const RESOURCE_CHANGED_MSG: &str = "resource changed since download started";\n',
            'const RESOURCE_CHANGED_MSG: &str = "resource changed since download started";\n'
            + STATUS_FN); n += 1
        # unit test before the mod-tests closing brace
        text = t.read_text(encoding="utf-8")
        idx = text.rstrip("\n").rfind("\n}")
        t.write_text(text[:idx] + UNIT_TEST + "}\n", encoding="utf-8"); n += 1

        c = base / "tests/common/mod.rs"
        sub(c, '    pub b18_if_range_seen: Arc<AtomicBool>,\n',
            '    pub b18_if_range_seen: Arc<AtomicBool>,\n' + B19_FIELD_DOC); n += 1
        sub(c, '        let b18seen = Arc::new(AtomicBool::new(false));\n',
            '        let b18seen = Arc::new(AtomicBool::new(false));\n'
            '        let b19forbid = Arc::new(AtomicBool::new(false));\n'); n += 1
        sub(c, '        let b18s = b18seen.clone();\n',
            '        let b18s = b18seen.clone();\n        let b19f = b19forbid.clone();\n'); n += 1
        sub(c, '                    let b18s = b18s.clone();\n',
            '                    let b18s = b18s.clone();\n'
            '                    let b19f = b19f.clone();\n'); n += 1
        sub(c, '    b18_if_range_seen: Arc<AtomicBool>,\n',
            '    b18_if_range_seen: Arc<AtomicBool>,\n    b19_forbid: Arc<AtomicBool>,\n'); n += 1
        sub(c, '            b18_if_range_seen: b18seen,\n',
            '            b18_if_range_seen: b18seen,\n            b19_forbid: b19forbid,\n'); n += 1
        sub(c, '        if range.is_some() && if_range_req {\n'
               '            b18_if_range_seen.store(true, Ordering::SeqCst);\n        }\n',
            '        if range.is_some() && if_range_req {\n'
            '            b18_if_range_seen.store(true, Ordering::SeqCst);\n        }\n'
            + B19_USAGE); n += 1

        e = base / "tests/http_engine.rs"
        s = e.read_text(encoding="utf-8").rstrip("\n")
        e.write_text(s + "\n" + E2E_TEST, encoding="utf-8"); n += 1
    print(f"re-applied {n} hunks")


if __name__ == "__main__":
    main()
