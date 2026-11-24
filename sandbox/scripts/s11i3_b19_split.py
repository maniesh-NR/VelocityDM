#!/usr/bin/env python3
"""Wave-3 commit split helper: temporarily revert (or re-apply) the B19 bits
so commit 1 = B17+B18 and commit 2 = B19, per gate 0009 REL-1 (per-item
revertable commits). The FINAL tree is the verified-green one either way.

Usage: s11i3_b19_split.py revert|apply   (run from the repo root)
"""
import sys
from pathlib import Path

MODE = sys.argv[1]
ROOT = Path("/home/z/my-project/velocitydm-repo")
TREES = [ROOT / "crates/vdm-core", ROOT / "win7/crates/vdm-core"]

FAIL_NOTICE = '''        // S11-i B19: refreshable failures surface an actionable hint on the
        // EXISTING note channel (every frontend renders notes; no new
        // surface, no new permission, no RPC change).
        if msg.contains(REFRESHABLE_MARKER) {
            s.notice = Some(
                "This link looks expired or forbidden \\u{2014} open the source page, re-grab the link (extension grab-links or the page context), then add the fresh address."
                    .to_string(),
            );
        }
'''

STATUS_FN = '''/// S11-i B19: user-facing status classification. 403/410 carry a stable
/// marker the frontends' note channel acts on; every other status keeps its
/// historical exact string (tests and callers depend on them).
const REFRESHABLE_MARKER: &str = "link may be expired or forbidden";

fn status_failure_reason(status: u16) -> String {
    if status == 403 || status == 410 {
        format!(
            "HTTP {} ({} \\u{2014} re-open the source page and re-grab the link)",
            status, REFRESHABLE_MARKER
        )
    } else {
        format!("HTTP {}", status)
    }
}
'''

B19_TEST = '''    #[test]
    fn refreshable_status_classification() {
        for s in [403u16, 410] {
            let msg = status_failure_reason(s);
            assert!(msg.contains(REFRESHABLE_MARKER), "{}", msg);
            assert!(msg.starts_with("HTTP "));
        }
        // other statuses keep their historical exact strings
        assert_eq!(status_failure_reason(404), "HTTP 404");
        assert_eq!(status_failure_reason(500), "HTTP 500");
        assert_eq!(status_failure_reason(503), "HTTP 503");
        assert!(is_retryable(&Error::Http("HTTP 503".into())));
        assert!(!is_retryable(&Error::Http(status_failure_reason(403))));
    }
'''

# (relative path, old, new) — applied in order. `old` uses the POST-FMT text.
EMDASH = "\u2014"

def pairs(mode):
    if mode == "revert":
        return [
            ("src/engine/task.rs",
             '        Some(status_failure_reason(head_status))',
             '        Some(format!("HTTP {}", head_status))'),
            ("src/engine/task.rs",
             "                    fatal = Some(status_failure_reason(r2.status));",
             '                    fatal = Some(format!("HTTP {}", r2.status));'),
            ("src/engine/task.rs",
             '                    return WorkerExit::Failed(format!(\n                        "server returned {}",\n                        status_failure_reason(code)\n                    ));',
             '                    return WorkerExit::Failed(format!("server returned {}", code));'),
            ("src/engine/task.rs",
             FAIL_NOTICE.replace("\\u{2014}", EMDASH),
             ""),
            ("src/engine/task.rs",
             STATUS_FN.replace("\\u{2014}", EMDASH) + "\n",
             ""),
            ("src/engine/task.rs",
             B19_TEST + "\n",
             ""),
            ("tests/common/mod.rs",
             '    /// S11-i B19 e2e: when armed, HEAD and ranged requests to "/b19.bin"\n    /// return 403 (the expired/forbidden-link simulation).\n    pub b19_forbid: Arc<AtomicBool>,\n',
             ""),
            ("tests/common/mod.rs",
             "        let b19forbid = Arc::new(AtomicBool::new(false));\n",
             ""),
            ("tests/common/mod.rs",
             "        let b19f = b19forbid.clone();\n",
             ""),
            ("tests/common/mod.rs",
             "                            b19f,\n",
             ""),
            ("tests/common/mod.rs",
             "    b19_forbid: b19forbid,\n",
             ""),
            ("tests/common/mod.rs",
             "    b17compress: Arc<AtomicU32>,\n    b18_return_200: Arc<AtomicBool>,\n    b18_if_range_seen: Arc<AtomicBool>,\n    b19_forbid: Arc<AtomicBool>,\n",
             "    b17compress: Arc<AtomicU32>,\n    b18_return_200: Arc<AtomicBool>,\n    b18_if_range_seen: Arc<AtomicBool>,\n"),
            ("tests/common/mod.rs",
             "        // S11-i B19 e2e: expired/forbidden link simulation\n        if path == \"/b19.bin\"\n            && b19_forbid.load(Ordering::SeqCst)\n            && (method == \"HEAD\" || range.is_some())\n        {\n            write_simple(&mut stream, 403, \"Forbidden\", b\"forbidden\")?;\n            continue;\n        }\n",
             ""),
        ]
    else:  # apply = inverted pairs
        return [(p[0], p[2], p[1]) for p in reversed(pairs("revert"))]


def main():
    changed = 0
    for rel, old, new in pairs(MODE):
        for base in TREES:
            f = base / rel
            text = f.read_text(encoding="utf-8")
            if old in text:
                f.write_text(text.replace(old, new, 1), encoding="utf-8")
                changed += 1
            elif MODE == "apply" and new in text:
                pass  # already applied
            else:
                print(f"WARN not found: {base.name}/{rel} :: {old[:60]!r}")
    print(f"{MODE}: {changed} replacements")


if __name__ == "__main__":
    main()
