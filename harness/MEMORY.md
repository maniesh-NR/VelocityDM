# Project Memory (durable)

Append-only per session; newest entries at the bottom. An agent starting a
session reads this file first, then `ROADMAP.md`.

## Environment

| Path | Meaning |
| --- | --- |
| `/home/z/my-project/velocitydm-repo/` | the repository (origin: `https://github.com/maniesh-NR/VelocityDM.git`) |
| `/home/z/my-project/scripts/` | sandbox tooling (`publish-velocitydm.sh`, `backup-sandbox.sh`) |
| `/home/z/my-project/release-assets/` | floor assets for the 6 releases |
| `/home/z/my-project/.gh_token` | fine-grained PAT. **Never commit, print, or back up.** |
| Rust | rustup at `~/.cargo/bin` (stable + 1.75.0 installed; add `export PATH="$HOME/.cargo/bin:$PATH"`; use `CARGO_INCREMENTAL=0` — disk is tight) |
| Node | v24.x on PATH |
| Android | local build works: SDK at `/tmp/android-sdk`, Gradle 8.9 at `/tmp/gradle-8.9`, full JDK at `/tmp/jdk-root` (use `GRADLE_USER_HOME=/tmp/gradle-home`, `JAVA_HOME=<jdk-root>`, `ANDROID_HOME=/tmp/android-sdk`; the default JDK lacks `jlink`) |
| Zig/dlltool | zig 0.13.0 at `/tmp/zig-linux-x86_64-0.13.0`, mingw binutils at `/tmp/mingw-root/usr/bin` for local win7 zigbuild |
| Disk | ~10 GB total; free space is the scarce resource — clean `target/debug`, `/tmp` between heavy builds |

## Hard-won learnings

- **Headless push auth:** credential helpers never fire without a TTY; use
  `git -c http.extraheader="Authorization: Basic $(printf 'maniesh-NR:%s' "$TOK" | base64 -w0)" push ...`.
- **Token writability:** the API `permissions` block can lie; verify with a
  probe-branch push.
- **GitHub runners already ship mingw binutils (dlltool); this sandbox does
  not** — local win7 zigbuild needs `/tmp/mingw-root/usr/bin` on PATH.
- **rustfmt/clippy move with the toolchain:** re-run `cargo fmt --all` and
  `cargo clippy --workspace --all-targets -- -D warnings` after every Rust
  edit (both workspaces; CI checks root + `win7/` fmt).
- **The remote can diverge between sessions.** On 2026-11-08 a parallel
  session pushed a 17-commit line (engine socket fixes, WouldBlock handling,
  osxcross Darwin builds, win7 rustls 0.23.46 security pin, Android
  Compose-plugin fix) with GREEN CI + 6 live releases. Always
  `git fetch origin` and reconcile before planning a push; a plain push
  into a diverged main dies at step 1 with a misleading "token" error.
- **Rust 1.75 (win7 payload) gotchas:** root Cargo.lock is v4 (unparseable
  by cargo 1.75) and resolves edition-2024 crates — build from the pinned
  `win7/Cargo.toml` workspace with `--locked`; zeroize must stay 1.8.1;
  the GH runner's mingw binutils satisfy the i686 zig/lld link (locally a
  no-op framestub was needed only because of the older rustls pin).
- **`zip -q <out> .` is not recursive** — use `zip -qr` in packaging.
- Background (`nohup ... &`) jobs do not survive between tool calls here;
  run long checks in the foreground with a timeout.
- Two Android package trees in one source set is a trap: the manifest's
  namespace decides; the other rots invisibly. (The CI-proven
  `com.velocitydm.android` tree ships in 1.0.0; the richer
  `com.velocitydm.app` Room/DataStore rewrite is a v1.1.0 candidate.)

## 2026-11-13 — Material You completion + harness + adversarial gate session

- Token re-verified writable (probe branch pushed + deleted).
- Round 1 of the adversarial panel **BLOCKed**: found the 1.75 lockfile
  blockers, an unbuildable Android module, an inert themeMode, a dead
  progress bridge, stale tests, dead files. All fixed locally (including a
  real local `gradle :app:assembleDebug` that caught 15 Kotlin errors).
- Round 2 **BLOCKed again — on ship-path facts**: the remote had diverged
  (the parallel Nov-8 line above) with green CI and live releases; the
  local tree had drifted from it. Resolution: **reset the working tree to
  `origin/main` (CI-proven: engine fixes + Darwin/osxcross + win7 security
  pins + their Material You desktop theme.rs and Android theming) and layer
  the unique session work on top**: the agent harness (this directory),
  `scripts/backup-sandbox.sh` + `.github/workflows/sandbox-backup.yml`
  (event-driven backups, never cron), and the dynamic Material You browser
  extension engine (`extensions/shared/md3.js` + seed pickers in
  popup/options + 12 tests, superseding the static-token version).
- Deferred: the `com.velocitydm.app` Room/DataStore Android app (built
  locally, but the CI-proven simpler tree ships 1.0.0); the companion
  `extension/` tree stays dev/test-only (not shipped by release.yml).
- Next: round-3 panel, then S2: commit, force-move the 6 tags (remote tags
  exist at old commits; releases exist — the Release workflow refreshes
  assets via `gh release upload --clobber`), push, delete cron job
  514177, poll CI/Release, verify the 6 releases, protect main with the 8
  required checks, run the closing backup.

## 2026-11-13 (later) — S2/S3 complete

- S1 gate closed (0001b, round-3 consensus: 3× PROCEED-WITH-CONDITIONS, zero BLOCK).
- Commit 5cd2e63 pushed to main (fast-forward onto the CI-green 26343d7).
- Six tags force-moved to 5cd2e63 — **pushed one per command**: GitHub
  suppresses push events when >3 tags move in a single push, which would
  have silently skipped the Release runs (RED round-3 finding).
- All six Release workflows succeeded; all six releases verified with
  refreshed assets (2026-11-13T10:44–10:48); `shared/md3.js` verified
  inside the published `chromium-1.0.0.zip`.
- CI on main: first run failed `Test (macos-latest)` — newest stable
  (1.100) breaks `objc2 0.4.1` (eframe 0.27 macOS stack via icrate 0.0.4;
  the Darwin release job passed only because the osxcross container pins
  an older rustc). Fix (58c877e): pin the macOS test job to rustc 1.99.1,
  the exact toolchain of the Nov-8 green run. CI now green: 8/8 required
  checks.
- Branch protection verified via API: main is protected with EXACTLY the
  8 required checks (byte-exact names), strict=false, force-pushes and
  deletions disallowed.
- Cron job 514177 (publish retry) deleted after the push succeeded.
- Backups executed per the event-driven rule (see BACKUPS.md).

## 2026-11-13 (session 3) — post-environment-loss rebuild, 5-role panel, hardening

- The sandbox was lost AGAIN after S2/S3 completed; a fresh session rebuilt
  the repo from a stale pre-Material-You snapshot. That rebuild regressed
  the parallel line's fixes (win7 job shape, Android tree, theme.rs, md3.js).
  **Lesson repeated: ALWAYS `git fetch origin` and compare before building
  anything — the remote is the source of truth, and it was 17 commits ahead.**
- A five-role adversarial panel (ATKR security / FALS falsifier / REQ
  requirements / INT integration / REL release-SRE) reviewed the rebuild +
  republish plan: 5/5 OBJECT. Findings and remediation:
  `ASSESSMENTS/0002-reconciliation-hardening.md`.
- Notable catches beyond the panel's predecessors:
  - **Loopback RPC was web-page-reachable** (no Host/Origin/Content-Type
    checks; RCE via `queue.set onQueueDrained` -> `sh -c`). Fixed with the
    three-rule trust boundary + regression tests (both trees).
  - **The remote's `daemon.shutdown` shipped broken** (engine stopped,
    process never exited) — reproduced live on the published musl asset.
    Fixed: dispatch winds the server down; daemon loop exits on
    `server.stopped()`; CLI `shutdown` restored.
  - **`cargo test --workspace` does NOT emit plain sibling bins** into
    `target/debug` (only `cargo build` does) — the full-stack e2e test
    needs the NMH binary, so CI Test jobs now run
    `cargo build --workspace --bins` before `cargo test --workspace`.
  - **Outgoing request headers are RPC-reachable** (options.headers) —
    now CRLF/NUL-sanitized at emission (incl. user-agent).
- `extension/` (the dev-only v2 duplicate) cut entirely; CI counts only the
  shipped `extensions/` suite now. `webNavigation` permission dropped
  (unused).
- poll-runs.sh: failure-aware (exit 1 on red runs), TIMEOUT marker
  (exit 2), 1800 s default. publish-velocitydm.sh rewritten for the real
  remote state (tags force-moved one-per-command, verified; idempotent
  config; CI clobbers release assets). harness/RUNBOOK.md added.
- Docs brought to truth: RPC.md / EXTENSIONS.md / docs README describe
  the ACTUAL single-address protocol; bench.sh runs against the current
  CLI; TEST-REPORT regenerated (127 + 76 + 24 + 27).

## 2026-11-13 (session 3, close) — S1R published

- S1R gate closed (0002b: five-role panel, rounds 2+3, 5/5 PASS, zero
  outstanding objections; the two post-consensus tightening touches are
  documented in the record).
- Publish executed: main fast-forwarded to b27549a; all six tags re-cut
  (annotated, one-per-command force-pushes) and verified on the remote;
  7 workflows green (1 CI + 6 Release); all six releases carry assets
  with timestamps postdating the push (16:49-16:54 UTC) — the broken
  daemon.shutdown binaries are clobbered. Branch protection re-verified:
  exactly the 8 byte-exact required checks, strict/enforce_admins false,
  force-pushes and deletions disallowed.
- Topics API quirk: a repo cannot have more than 20 topics (422
  otherwise). Applied exactly 20.
- Cron 514177 no longer exists on this session's cron list (the retry
  cycle ended) — nothing to delete.
- Local tags note: the working repo's tags now point at the new tag
  objects (re-cut by the publisher at runtime), matching the remote.

## 2026-11-14 (session 4) — recovery verification: the harness survived an environment loss

- The sandbox was recycled again after the S1R close; this session
  bootstrapped exactly as `harness/README.md` prescribes (MEMORY → ROADMAP
  → PROTOCOL → WORKFLOW) instead of rebuilding blind. Remote main was 29
  commits ahead of the stale local snapshot and already contained ALL
  S1/S2/S3/S1R work (Material You, harness, adversarial gates, publishes).
  Fast-forwarded to `b41d5b8`; zero rework, zero regression. **The backup
  rule and the memory-first bootstrap did their job — this is the third
  environment loss and the first one recovered with no rework.**
- Token re-verified writable via a Contents API probe (commit `b41d5b8`;
  the probe file `harness/.write-probe` is removed by this session's
  closing commit, as sessions 2/3 did with theirs).
- Live GitHub state verified by API in this session:
  - 6 releases, all carrying assets (v1.0.0: 14 assets; -linux/-macos: 3;
    -windows/-extensions: 5; -android: 2).
  - Branch protection on `main`: EXACTLY the 8 byte-exact required checks;
    enforce_admins false; force-pushes and deletions disallowed.
  - All six remote tags point at `b27549a` (the S1R publish HEAD).
  - Cron job 514177 is gone (deleted after the successful publish; the
    retry cycle is closed).
- Sandbox tooling restored from the `sandbox-backup` branch snapshot
  (`backup-sandbox.sh`, `poll-runs.sh`, `move_test_mod.py`) — the branch
  carried them across the recycle, as designed.
- `PROTOCOL.md` codified: the five-role panel (ATKR/FALS/REQ/INT/REL) is
  the comprehensive standard for future stage gates; RED/BLUE/AUDITOR
  remains the minimum quorum. README wording aligned.
- Next actionable stage is S4 (DHT, BEP-5) — PENDING; per the protocol it
  must open with a fresh five-role gate assessment before any work starts.
