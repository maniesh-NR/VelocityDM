# Assessment 0002 — Reconciliation & hardening (5-role panel)

**Date:** 2026-11-13 (UTC). **Stage under review:** the post-environment-loss
rebuild + the plan to republish. **Panel:** a five-role adversarial panel
(ATKR / FALS / REQ / INT / REL — see the roles section below), dispatched
independently (each agent received the evidence pack and repository access,
no peer verdicts). This panel complements the RED/BLUE/AUDITOR protocol
(0001/0001b) with five narrower mandates.

## Round 1 — verdicts (all OBJECT; stage blocked)

| Agent | Role | Verdict | Core objection |
| --- | --- | --- | --- |
| ATKR | Red Team / Security | **OBJECT — blocker** | The loopback RPC performed no Host/Origin/Content-Type validation: any web page could drive unauthenticated JSON-RPC (no-cors `text/plain` POST; full read/write via DNS rebinding since Host is ignored), escalating to RCE via `queue.set onQueueDrained` -> `sh -c`, arbitrary file writes via `options.out_dir`, file deletion, proxy-credential disclosure; plus CRLF injection in outgoing headers; plus an unused `webNavigation` permission |
| FALS | Skeptic / Falsifier | **OBJECT — major** | Protocol docs described the defunct dual-port framed interface (docs/RPC.md entirely; EXTENSIONS.md NMH section; docs/README.md; TEST-REPORT.md incl. un-runnable bench.sh); the evidence pack misattributed the dead-file removal commit |
| REQ | Requirements Guardian | **OBJECT — major** | Android deliverable could not compile (dead Room/DataStore tree vs missing deps; release.yml android job would fail; architecture docs described the dead tree); the `extension/` duplicate inflated verification counts |
| INT | Integration Adversary | **OBJECT — blocker** | `Windows 7 payload (Rust 1.75)` job could not pass on first run: lockfile v4 rejected by cargo 1.75 (reproduced with a real 1.75 install) AND the command targeted the main tree (no `--manifest-path`); android job copied `app-debug.apk` that ABI splits never produce; Kotlin 2.0.21 Compose plugin missing; gradle wrapper not committed |
| REL | Release Blocker / SRE | **OBJECT — blocker** | The remote was NOT empty: six tags existed (at `5cd2e63`), six public releases existed whose binaries contained the broken `daemon.shutdown` (reproduced live on the stale prebuilt musl asset), remote main had diverged (push would be non-FF), the prebuilt SHA256SUMS were stale (7/11 mismatch, APK unlisted), `poll-runs.sh` printed a false "ALL DONE" on failures, and no rollback runbook existed |

## Remediation applied (this session)

1. **Reconcile onto the CI-proven remote line** — local `main` reset onto
   `origin/main` (`e564779`): recovers the Material You desktop
   `theme.rs` + Android dynamic color + `extensions/shared/md3.js`, the
   working Android tree, the Darwin/osxcross pipelines, the win7
   `--manifest-path --locked` job fix, and the v3 lockfiles. (This also
   resolves INT's win7 blockers and REQ's Android blocker — the remote
   line had already fixed them; the session rebuild had regressed them.)
2. **daemon.shutdown fixed properly** (remote line still shipped it
   broken — REL reproduced it): dispatch now winds the RPC server down
   (`stop.store(true)`) and the daemon main loop exits when the server
   stops; the CLI `shutdown` subcommand restored; regression-covered by
   the rewritten full-stack e2e test (`crates/velocitydm/tests/e2e_bin.rs`,
   restored from the rebuild line and passing: 1 test, real binaries,
   daemon+CLI+NMH+SSE+limits+pause/resume).
3. **ATKR hardening (both trees)**: `host_allowed` Host validation
   (loopback-name/ bound-host matrix), unconditional Origin/Referer
   rejection, `Content-Type: application/json` enforcement on POST /rpc,
   16 KiB header-line cap, CRLF/NUL sanitization of all outgoing request
   headers (incl. the user-settable User-Agent), `webNavigation` permission
   removed (verified absent in all four rebuilt zips). Regression tests:
   `web_page_request_shapes_are_refused` (6 attack shapes refused,
   2 legitimate shapes pass) + `host_allowed_matrix` (17 assertions).
4. **FALS doc truth**: docs/RPC.md rewritten to the actual single-address
   HTTP JSON-RPC 2.0 + SSE surface (22 methods, snapshot shape, trust
   boundary); docs/EXTENSIONS.md rewritten (shipped `extensions/` tree,
   `{cmd:...}` NMH wire, Material You section); docs/README.md corrected
   (components, quick start, test counts); scripts/bench.sh rewritten to
   the current CLI (`--addr`, XDG_CONFIG_HOME isolation, limits --global,
   status polling); TEST-REPORT.md regenerated from this session's real
   runs (below).
5. **REQ hygiene**: the `extension/` v2 duplicate cut entirely (tree +
   its CI step + doc references); verification counts now cover shipped
   code only.
6. **REL tooling**: `poll-runs.sh` — failure-aware ALL DONE (exit 1 on
   red runs), TIMEOUT marker (exit 2), 1800 s default budget;
   `publish-velocitydm.sh` rewritten for the real remote state (fast-forward
   push + force-move of tags one-per-command + remote verification +
   idempotent labels/milestone/issues, no stale-floor uploads — CI
   clobbers via `gh release upload --clobber`); `harness/RUNBOOK.md`
   added (rollback, partial failure, protection, post-protection change
   workflow, auth failure modes, backup posture).
7. Dead files the remote line still carried removed
   (`vdm-core/src/hls.rs`, `vdm-rpc/src/{framed,server,sse}.rs`); win7
   tree re-synced (security + shutdown fixes, e2e test, header
   sanitization; keeping its 1.75-conservative constructs).

## Verification evidence (this session, on the reconciled tree)

| Check | Result |
| --- | --- |
| `cargo test --workspace` (stable 1.100.0) | 127 passed, 0 failed (97 core unit + 25 core integration + 4 vdm-rpc + 1 full-stack e2e) |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 errors |
| `cargo fmt --all -- --check` (root + win7) | clean |
| win7 tree `cargo check --workspace` | clean |
| `node extensions/tests/run-tests.mjs` | 76 assertions passed (incl. MD3 scheme) |
| `bash scripts/e2e.sh` (release binaries) | 24/24, daemon RSS 6344 KB |
| `bash scripts/e2e-cross.sh` | 27/27, daemon RSS 7440 KB after mixed load |
| `bash scripts/build-ext.sh` + zip audit | 4 zips, `shared/` present, no `webNavigation` |

## Round 2

Re-review by the objecting agents against this record + the repository:
see `0002b-round2-consensus.md` (filed after the round closes).
