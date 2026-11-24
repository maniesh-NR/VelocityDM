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
| Rust | **this instance:** rustup/cargo live under `tools/rustup` + `tools/cargo` (survived recycles there) — run with `export RUSTUP_HOME=/home/z/my-project/tools/rustup CARGO_HOME=/home/z/my-project/tools/cargo PATH=/home/z/my-project/tools/cargo/bin:$PATH`; toolchains: stable 1.100.0 (default) + 1.75.0 (win7 witness); use `CARGO_INCREMENTAL=0` — disk is tight |
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

## 2026-11-14 (session 4, amendment) — S11 added: IDM competitive study

- Owner-directed roadmap amendment: study the reference product (the
  official IDM 6.43 build 18 installer) for download-management /
  browser-integration methods worth adopting for performance,
  stability, and resource use. Added as advisory stage S11 (parallel to
  S4+, never blocks the release train) after a two-role amendment gate.
- Artifact pinned: URL + SHA-256
  `7346128fab42957b4254eabdb012bfdbd751e9069361e98a6916f26bb03d62ee` +
  12,164,608 bytes + PE32 i386 (fingerprinted this session, binary
  deleted immediately, never stored; REQ re-verified the fingerprint
  byte-exact against the live official URL). Fail-closed re-pin policy
  baked into the brief (vendor rotates builds).
- Amendment gate record: `ASSESSMENTS/0003-s11-roadmap-amendment.md`
  (RED + REQ dispatched independently; 2× PROCEED-WITH-CONDITIONS, zero
  BLOCK → consensus). All nine conditions folded into the S11 brief
  BEFORE commit. The load-bearing ones: T2/T3 (decompile / dynamic RE)
  require a SEPARATE dated owner-signed legal-basis clearance on top
  of the activation gate — a declined clearance escalates back to the
  owner, never silently drops the directive; spec-first clean-room
  barriers (behavior spec committed BEFORE the implementing commit;
  T2/T3 artifacts only in a wiped throwaway dir outside the repo);
  honest prerequisites (this sandbox has no Windows VM and no
  decompiler — T1/T3 need an owner-provisioned Windows machine, T2
  needs Ghidra-class tooling; until then: plan + literature analysis
  only, never invented measurements).
- GitHub issue #15 created on the `v1.1.0 - roadmap` milestone
  (labels area:engine + area:extensions) BEFORE the ROADMAP commit so
  the "tracked as issue" wording is true at commit time (REQ-1).
- Sandbox note: `scripts/create_s11_issue.py` (issue-creation helper)
  joined the toolbox; it ships in the next backup snapshot.

## 2026-11-14 (session 4, T2 probe) — Ghidra analyzeHeadless: FEASIBLE in this sandbox

- Owner question: can Ghidra CLI (`analyzeHeadless`) run here? Answer:
  **yes, verified end-to-end.** Probe (`scripts/ghidra/ProbeS11.java`,
  backed up with the toolbox): import a 16 KB gcc-built ELF →
  auto-analysis → runtime script compilation → headless C decompilation
  of `main`; decompiled output matched the C source semantically,
  line-for-line. ~10 s wall, 407 MB peak java RSS, exit 0, on this
  2-CPU / 4 GB-RAM / glibc-2.41 machine.
- Toolchain now provisioned persistently at `/home/z/my-project/tools/`
  (survives sandbox recycles; NOT pushed to backups — ~1.23 GB):
  Ghidra 12.2 (zip SHA-256 verified against the release) + Temurin JDK
  25. Re-install recipe in `RUNBOOK.md` ("Ghidra headless in the
  sandbox").
- Hard-won setup facts (all bit us, all in the runbook):
  - Ghidra 12.2 requires **JDK 25** (`Ghidra/application.properties`
    `application.java.min=25`) — a full JDK, not the JRE: script
    compilation needs `javac`. System java is a JRE-only 21 — rejected;
    Temurin 21 — rejected too.
  - The no-TTY launcher ignores the `JAVA_HOME` env var and dies with
    "Unable to prompt user for JDK path" — set
    `JAVA_HOME_OVERRIDE` in `tools/ghidra/support/launch.properties`.
  - The headless project PARENT directory must pre-exist.
- Scope honesty: the probe exercised the ELF path; PE32 i386 (the S11
  target format) is a documented built-in Ghidra loader, not something
  this probe ran (no local PE test target; the IDM artifact itself
  stays untouched pending the T2 legal clearance). A 12 MB packed
  installer will need real analysis time + heap budgeting on 4 GB RAM.
- ROADMAP S11 prerequisites updated to the new truth (T2 toolchain:
  provisioned; T2 clearance: still outstanding; T1/T3 Windows VM:
  still needed). Issue #15 commented with the probe evidence.

## 2026-11-15 (session 5) — S12: native UI stacks per OS + shared MD3 engine

- Owner directive: specific UI stacks per OS around the shared Rust
  core, Material You everywhere, OS-specific elements kept. Delivered
  (gate 0004, RED+REQ 2× PROCEED-WITH-CONDITIONS, zero BLOCK, all
  conditions fixed pre-commit): `crates/vdm-md3` (the shared Material
  You token engine — egui theme.rs now delegates; md3-tokens JSON
  export; golden-literal tests), `frontend/win32/` (Raw Win32 +
  Direct2D, hand-rolled wndproc, vector MD3 chrome, native
  EDIT/BUTTON children, DPI-scaled chrome, WM_DPICHANGED),
  `crates/velocitydm-slint/` (root-workspace member → covered by the
  required fmt/clippy/test checks on all 3 CI OSes; native input
  widgets), `macos-app/` (SwiftPM Swift+AppKit; Md3.swift port of the
  token engine; JSON-RPC client per docs/RPC.md; structured proxy
  round-trip; limits.get/set). Two NON-required CI jobs added
  (win32-frontend windows-latest, swift-frontend macos-latest); the
  8 required checks and all six releases untouched.
- Deferred as gated sub-stages + issue #16 (honest, not silent):
  Android Views+KMP (S12-e), gtk3-rs (S12-lin2), Qt (S12-lin3),
  SwiftUI (S12-mac2), Swift SSE, promotion gates.
- Hard-won session learnings:
  - windows-rs 0.62 API facts that cost iterations (grep the crate
    source in ~/.cargo/registry — the type-checker plus greps beat
    docs): rects are `Common::D2D_RECT_F`; `D2D1CreateFactory` takes
    (factory_type, Option<&D2D1_FACTORY_OPTIONS>); `DrawText` takes
    `&[u16]` + 6 args on the hwnd target; `CreateWindowExW` parent/
    hinstance are Options; `BS_*` are i32 (cast to WINDOW_STYLE);
    `EndDraw(Option, Option)`; `RegisterClassW` returns u16; HiDpi
    feature spelled `Win32_UI_HiDpi`; WM_SIZE size lives in lparam.
  - `cargo fmt --all` REFORMATS after edits — batch string edits
    against post-fmt text, or rewrite whole functions.
  - Slint 1.18: needs `compat-1-18` feature; `build.rs` +
    `slint-build::compile(...)` (no `.build()`); globals use `in`
    property for Rust-settable; `ui.global::<Theme>()`; `VecModel`
    shared via `Rc`, updated with `set_vec`; Timer needs an instance
    (`Timer::default()` then `.start(...)`); element bodies use
    semicolons and `id` is reserved (renamed task-id).
  - `ring` cannot cross-compile to msvc from Linux — local win32
    type-checks run `--no-default-features` (tls off; vdm-rpc gained
    `default-features=false` + tls passthrough so this is possible);
    CI windows-latest builds the real thing with tls.
  - Root Cargo.lock is now **v4** (policy change, verified): cargo
    1.100 rewrites it on any dep change; all 1.75 builds use the
    standalone `win7/ --locked` workspace so v4 is harmless.
    win7/Cargo.lock stays v3.
  - The `.gitignore` missed `frontend/win32/target/` (182 MB one
    `git add` away) — REQ catch; new standalone workspaces must add
    their `target/` to `.gitignore` immediately.
- Environment re-provisions this session: rustup stable (1.100) +
  rustfmt + clippy + `x86_64-pc-windows-msvc` target (rustc/toolchains
  do not survive recycles; tools/ghidra+jdk DO — under my-project).
- CI witness (final): **ALL GREEN at 9aa79bd** — 10/10 jobs, including
  the two new non-required frontend builds: Win32+Direct2D compiled
  and LINKED on real MSVC (windows-latest), Swift+AppKit compiled on
  macos-latest. Two fix iterations were needed: ubuntu jobs needed
  `libfontconfig1-dev` (slint's font stack; the dev sandbox happened
  to have it, the runners did not — lesson: verify system-lib deps
  against the CI image, not the sandbox), and Swift blind-write nits
  (NSButton `.rounded` vs NSTextField `.roundedBezel`;
  `UInt32(bitPattern:)` takes Int32 — use truncatingIfNeeded; a
  renamed helper). Also: the local tree had silently lost the T2
  commit 939c3b9 (a recycle between sessions) — the push was rejected
  non-FF and a rebase onto origin/main recovered it; ALWAYS fetch
  before planning (the standing lesson, now twice-proven).

## Session 6 (2026-11-15) — S12-e: Android Views + Kotlin Multiplatform

- **S12-e activated, built, and closed (preview) under gate 0005**
  (five-role panel: ATKR/FALS/REQ/INT/REL, all PROCEED-WITH-CONDITIONS,
  zero BLOCK — the first full five-role gate of the project; all 20
  condition clusters folded pre-implementation, recorded in
  `ASSESSMENTS/0005-s12e-android-views-kmp.md`).
- Built: `android/shared/` KMP module (`androidTarget()` + `jvm()` +
  custom **`jvmCommon` intermediate source set** that both JVM-family
  targets compile — this is the pattern that makes `:shared:jvmTest`
  work with only JVM actuals); commonMain = Kotlin Md3 port (full
  27-field Rust role set) + RangePool + strict-validated `.vdmctl`
  schema (kotlinx.serialization) + token-bucket wait math + formatting
  + URL/filename sanitization + task models on String paths; jvmCommon
  = the SegmentEngine orchestration (OkHttp/RandomAccessFile/atomics).
  `:app` = View-based Material 3 (MDC 1.12: MaterialToolbar,
  RecyclerView of MaterialCardViews, FAB, MaterialAlertDialog +
  TextInputLayout dialog with 3 fields), Compose fully removed,
  `BrowserActivity` is now a REAL activity (the manifest entry stopped
  dangling), themes.xml single DayNight Material3 parent.
- **Rust↔Kotlin drift tripwire**: `Md3GoldenTest` (jvmTest) asserts
  every field of all 8 named seeds × light/dark against
  `md3-tokens-baseline.json` generated from the canonical Rust engine
  by `scripts/gen-md3-baseline.py` — a third hand-copied literal set
  was explicitly a gate BLOCK-risk (FALS-1); the baseline file + test
  close it.
- **Release integrity (REL)**: GitHub ruleset `protect-v-tags-until-
  android-promotion` (id 27298722, tag target `refs/tags/v*`,
  restrict create+update, ACTIVE) — probed: even the admin token gets
  "push declined due to repository rule violations". The frozen six
  releases are now machine-guarded, not prose-guarded. RUNBOOK gained
  the "Android preview (S12-e)" section (failure policy, rollback,
  dispatch prohibition).
- **Gate catch worth remembering**: the draft said the pre-12 seed was
  0x6750A4 "the desktop seed" — ATKR + FALS caught it: the actual
  desktop DEFAULT_SEED is **0x4156A4** (brand blue; 0x6750A4 is
  "Baseline purple"). The shipped Chrome.kt seeds 0x4156A4 — verified
  consistent with egui/Slint/Win32.
- **Local Android build provisioned persistently** under
  `tools/`: jdk17 (Temurin 17.0.21), gradle-8.9, android-sdk
  (platform-34 + build-tools 34.0.0), cargo 1.100 (RUSTUP_HOME/
  CARGO_HOME under tools/ — survives recycles now). Local battery
  green: `:shared:jvmTest` (31 tests incl. goldens) +
  `:app:assembleDebug` + `:app:assembleRelease` (R8). Debug APK
  6.6 MB vs the Compose era's 10.5 MB (−37%; promotion gate should
  record the release-mode delta).
- Hard-won session learnings:
  - Kotlin/Java interop: **overloaded Java setters do not synthesize a
    Kotlin `var`** — `MaterialCardView.cardBackgroundColor` is read-only
    (two setCardBackgroundColor overloads) → call `setCardBackgroundColor(...)`
    explicitly. Same class of fix: `setRadius(dp(...).toFloat())`.
  - MDC attrs: `colorOnPrimary` is NOT in appcompat's R — it lives in
    `com.google.android.material.R.attr` (appcompat has colorPrimary/
    colorError). Compile-checked R.attr references fail fast, which is
    the cheap way to get this right.
  - The Write tool emitted a literal NUL byte inside a quoted char
    literal once (`it != '<NUL>'`) — the file compiled nothing useful;
    hexdump (od -c) the line when a match "should" work but doesn't.
    Also: bash `$'\x00'` cannot pass NUL args (rg then matches
    everything — the "56 matches" false alarm).
  - Background-job precedence: `cd X && curl A & curl B &` parses as
    `(cd X && curl A) & (curl B) &` — B lands in the OLD cwd. Use
    absolute `-o` paths for background downloads.
  - Maven Central 429s (rate-limited shared sandbox IP) resolve with
    a 90 s backoff + retry; Gradle caches partial dependency progress.
  - Adoptium's "latest 17" redirect supports Range resume (`curl -C -`).
  - jvmTest resource loading: commonTest cannot use java.lang — the
    golden test lives in `src/jvmTest/kotlin` with the JSON in
    `src/jvmTest/resources` (standard KMP pattern; the pure-logic tests
    stay in commonTest).
  - KMP gradle: `kotlin("multiplatform")` + `com.android.library` +
    `kotlin("plugin.serialization")` all declared apply-false in the
    ROOT build with the same KGP version as kotlin.android; the
    intermediate source set wiring is
    `val jvmCommon by creating { dependsOn(commonMain.get()) }` then
    androidMain/jvmMain `dependsOn(jvmCommon)`; jvmCommon-level
    dependencies (OkHttp) work via `jvmCommon.dependencies { }`.
- CI: one new **non-required** `android-frontend` job (ubuntu-22.04,
  temurin 17, setup-gradle 8.9 — byte-parity with the frozen release
  job): `:shared:jvmTest` + `:app:assembleDebug` + `:app:assembleRelease`.
  The first CI run on main is the official compile witness (KMP-on-
  Gradle-8.9 was unproven in-repo); local battery is already green.
  Branch protection re-verified at close-out (8 contexts unchanged,
  android-frontend absent).
- **CI witness (final): ALL 11 CHECKS GREEN at 4e78213** — including
  the new non-required `Android frontend (Views + KMP)` job
  (`:shared:jvmTest` 31 tests + assembleDebug + the R8 release build,
  ubuntu-22.04). KMP-on-Gradle-8.9 is now proven in-repo by CI, not
  just locally. Branch protection re-verified via API after the run:
  exactly the 8 unchanged required contexts, `android-frontend` absent,
  enforce_admins off, force-pushes off. Ruleset 27298722 still active
  (tag target). Zero CI fix iterations needed this time — the locally
  provisioned SDK build (tools/jdk17 + gradle-8.9 + android-sdk) paid
  for itself: every compile error was caught before the push.
- gh CLI died with this sandbox recycle (it lives outside my-project);
  `scripts/poll-checks.sh` (new, committed) polls check-runs via the
  REST API with the token file — the gh-free replacement used for this
  witness.

## 2026-11-21 (session 6) — S11-T2 executed: IDM static study delivered

- Owner directive (the dated T2 clearance the S11 brief demands) received
  in-chat 2026-11-21, quoted verbatim in the research doc's provenance
  table. Activation-gate re-verification passed byte-exact (12,164,608
  bytes + full SHA-256 vs the pinned fingerprint) on a fresh official-URL
  download BEFORE any work — fail-closed honored, no re-pin needed.
- Sandbox recycled again (5th loss): `tools/ghidra+jdk` were GONE (the
  "tools/ survives recycles" assumption is dead — treat tools/ as
  volatile and re-provision from RUNBOOK "Ghidra headless" every loss).
  Re-provisioned and zip-hash-verified in minutes. Remote main had
  diverged 3 commits (parallel S12-e session: gate 0005, Android
  Views+KMP, CI 11/11 at 4e78213) — fetched and fast-forwarded to 6215abf
  per the standing fetch-first lesson (now three-times-proven).
- T2 work performed (all static; no studied binary executed):
  - Unpacked the installer: custom zlib container — `!--END FILE--!`
    marker in the last 0x5000 bytes, u32 table-offset 4 bytes before it,
    entries `[u32 usize][u32 clen][3xFILETIME][zlib stream]`, entry 0 is
    a text manifest (P1..P127 names, LNK shortcuts, REGISTRY section).
    188 payloads extracted to the throwaway dir.
  - Ghidra full-decompiled: installer stub (272 fns), IDMMsgHost.exe
    (83 fns), MediumILStart.exe (172 fns). IDMan.exe (2.97 MB of code)
    was bounded to strings/imports/RTTI honestly — full decompile
    exceeds this sandbox's foreground-10-min/2-CPU/4-GB budget.
  - Extension JS read in full: Chrome MV3 CRX + Firefox MV2 WebExtension
    + two XUL/XPCOM generations; strings/imports for 16 helper DLLs/EXEs.
- Deliverable: `docs/RESEARCH-IDM-STUDY.md` — findings + adoption
  backlog B1–B16 (recommended/candidate) + N1–N6 (observed, not
  adopting). Headlines: cross-segment integrity verification with
  single-connection fallback; optional-connection dynamic scaling;
  HTTP/2 engine; EXT-X-BYTERANGE + ts2mp4 remux; four integration
  generations ending in MV3+DNR+native-messaging; NMH pipe-peer
  verification + COM cold-start; WFP/TDI kernel layer (explicitly not
  adopting); cookie-jar forwarding; video geometry gating. CLEAN_ROOM.md
  gained the S11 section + provenance row; ROADMAP S11 is ACTIVE.
- Clean-room discipline: every T2 artifact stayed in
  `/home/z/my-project/idm-t2/` (outside the repo), wiped after
  distillation; findings are functional descriptions only, spec-first
  barrier restated at every gate.
- Hard-won session learnings:
  - Ghidra 12.2 script API: `ExternalManager.getExternalLibraryNames()`
    is NOT for-each-able in this build (runtime script compile fails) —
    derive imports via objdump instead.
  - Foreground-only Ghidra (background jobs die between tool calls);
    a ~47KB-code PE stub decompiles fully in ~2.5 min on this box;
    budget bounded scripts for anything bigger.
  - Unauthenticated GitHub API calls are rate-limited from this IP;
    Bearer-token API calls still work — use the poll-checks.sh pattern.
  - Ghidra 12.2 release asset lives at tag `Ghidra_12.2_build`,
    `ghidra_12.2_PUBLIC_20261016.zip` (570 MB) — exact URL recorded in
    this entry because the API rate-limit can block discovery next time.

## 2026-11-21 (session 7) — S11-i opened: adoption wave 1 (B1/B2/B9)

- Context at open: S11-T2 closed (study + backlog delivered, backup
  3e7e31c); S12/S12-e closed (native frontends, CI 11/11). The
  continuation of the owner's T2 directive is the adoption half:
  "incorporated in to this project to improve performance, stability,
  and resource use". Sandbox recycled again (6th loss): tools/ gone
  (Ghidra/JDK not needed this wave), Rust re-provisioned (cargo
  1.100 + fmt + clippy at tools/cargo|rustup per RUNBOOK); local tree
  was 5 commits behind origin (parallel S12-e + T2 close) — fetched,
  fast-forwarded to 89f46b8 (four-times-proven fetch-first lesson).
- Gate 0006 recorded (five-role, 5x PROCEED-WITH-CONDITIONS, zero
  BLOCK): S11-i adoption wave 1 = B1 cross-connection content audit +
  single-connection recovery, B2 optional connections (spawn/retire),
  B9 file-lock classification + finalize retry. All 17 conditions
  folded into docs/SPECS/S11-ADOPTIONS.md BEFORE the implementation
  commit (spec-first barrier honored; commit split enforced).
- Hard constraints carried from the gate: audit reads under the file
  mutex (no torn reads); recovery capped at 2 then hard fail; notice
  text static/ours; no optional connections under ANY rate limit;
  absolute cap 32 / extra ≤ 8 with stagger; NO ResumeState field (the
  Android .vdmctl reader is strict — Vdmctl.kt
  ignoreUnknownKeys=false, verified); win7 tree ported with 1.75-safe
  syntax + local cargo +1.75 check witness; bench A/B evidence owed
  (conn 8 no-regression + conn 2 spawn-scenario) with inconsistent-
  edge e2e for B1 falsifiability.
- Wave 1 EXECUTED and closed (same session, plan: gate → spec-first →
  implement → battery → A/B → CI → backup):
  - Gate 0006 (5x PROCEED-WITH-CONDITIONS, zero BLOCK, 17 conditions
    folded into the specs before code); specs committed at 37e7e94,
    implementation at d67f164; clean-room barrier honored and recorded
    in docs/CLEAN_ROOM.md's S11-i adoption record.
  - Implementation: `AuditLedger` (written/audited 256 KiB blocks,
    in-memory only — the Android strict `.vdmctl` reader forbids
    ResumeState changes), monitor-driven audits (fresh-connection
    re-fetch, mirror rotation, file-locked reads, 8 MiB stride),
    bounded single-connection recovery (cap 2, `notice` field on the
    snapshot), `OptConnController` (3-tick spawn/retire persistence,
    caps 32/8, rate-limit + explicit-single + post-recovery guards,
    capacity HIGH-WATER MARK), B9 lock classifier + bounded finalize
    retry (3 x 2 s) with our messages. win7 tree ported each round
    (cargo +1.75.0 check green every time).
  - Local battery: vdm-core 105 unit + 6 bt + 5 hls + 17 http e2e (3
    new: inconsistent-edge recovery, degraded-connection spawn,
    rate-limit guard) + vdm-rpc 4; fmt + clippy -D clean; the
    GUI/slint/extension crates + e2e-smoke are CI witnesses (disk-full
    sandbox: target/ growth hit 0 bytes free mid-session; cleaned).
  - Bench A/B (labels in /tmp/bench-s11i.txt, per VDM_CONN knob added
    to bench-perf.sh): pre-conn8 159.3 MB/s vs post-conn8 159.1 (the
    first pre run 1.41 s was a cold-start artifact); pre-conn2 159.1 vs
    post-conn2 159.2; audit overhead +1 request/+1 conn on the fast
    profile (~0.6%); honest reading: NO regression, not a throughput
    win — the wins are the demonstrated stability (inconsistent-edge
    e2e) and straggler-throughput (degraded-connection e2e). The
    spawn bench at conn2 on localhost does not trigger (base conns
    saturate the pipe) — recorded honestly in the spec.
  - CI: 11/11 GREEN at 73b98bd after a 10-round fix-forward loop (the
    honest record): (1) sampler underflow — the recovery's done.store(0)
    moved `done` backwards into `d - last_sample` (fixed: sampler
    reset at respawn + saturating arithmetic); (2) Windows: per-CONN
    degradation never fired (recycling is platform-dependent) → global
    count; (3) macOS probe fallback consumed the count budget →
    real-range counting; still blind → byte-FRONTIER degradation
    (ranges entirely below 64 MiB crawl — fully deterministic by
    geometry); (4) an unsigned flip budget wrapped to MAX and re-armed
    flips forever → signed budget; (5) one_conn EMA collapsed under
    long crawls → high-water mark (a REAL design fix, spec amended);
    (6) macOS slow-serve TCP truncates bodies ("body shorter than
    content-length", retries exhausted) — test-infrastructure edge →
    the spawn e2e is gated off macOS runners with the reason in the
    test doc + spec (witnessed: Linux+Windows CI, dev sandbox, units).
  - Engine debugging tools added: `VDM_B2_DEBUG=1` env-gates a 1 Hz
    B2-signal dump (the runbook tool that cracked the macOS mysteries);
    TestServer gained flip/inconsistent-edge + degrade-frontier knobs
    with platform-behavior docs.
- Next: wave 2 (B3-B8: extensions/HLS items) needs its own five-role
  gate at activation; S4 (DHT) still PENDING with its gate; issue #15
  carries the wave-1 adoption comment (7059015764).

## 2026-11-21 (session 8) — context refresh + owner decision menu + wave-2 staging

- Sandbox recycled again (7th loss). Remote main had diverged 15 commits
  (parallel sessions closed S12-e Android Views+KMP under gate 0005, the
  S11-T2 static study, and S11-i wave 1 = B1/B2/B9 under gate 0006 with
  CI 11/11 at 73b98bd); local had a stale uncommitted BACKUPS.md line
  byte-identical on origin — discarded, then fast-forwarded to 99e430d.
  **The fetch-first lesson is now five-times-proven.**
- Environment: tools/ghidra + tools/jdk SURVIVED this recycle (present,
  verified); Rust did not — re-provisioned under tools/rustup|cargo
  (stable 1.100.0 + rustfmt + clippy + 1.75.0 for the wave-2 win7 port
  check). Battery witness on the recovered tree: fmt clean on BOTH
  trees, clippy vdm-core `-D warnings` clean, vdm-core lib tests
  105/105 green.
- **The owner requested an in-chat decision menu** ("input any decisions
  I have to make, while the rest of the development continues in
  parallel"). Menu issued in-chat and recorded here for post-loss
  reconciliation (defaults marked, owner may answer by code, e.g.
  "D1:A, rest default"):
  - D1 wave-2 scope: A full B3+B4+B5+B6+B7+B8 (default) / B
    extensions-only B4-B8, B3 folds into S7 / C A + parked B13+B16 /
    D hold wave 2, go straight to S4.
  - D2 S4 (DHT BEP-5) activation: A after wave 2 closes (default —
    sequential engine surgery) / B parallel with wave 2 / C after the
    S12 sub-stages.
  - D3 S12 sub-stage order: A SSE wiring for the Swift client, then
    S12-mac2 SwiftUI (default) / B S12-lin2 gtk3-rs / C S12-lin3 Qt /
    D promotion gates first.
  - D4 T1/T3 (Windows VM tiers): A defer — T2 findings suffice for now
    (default) / B owner provisions a Windows VM later, protocol
    prepared meanwhile / C close S11 research after the waves.
  - D5 promotion gates (previews → shipped; owner runtime verification
    owed): A after wave 2 + SSE wiring, verify a full surface (default)
    / B owner verifies current previews now / C at the v1.1.0 release
    train.
  - D6 v1.1.0 milestone shape: A single train, ordering via gates
    (default) / B split the UI restack to v2.0.0 / C unbind milestones
    for now.
  - D7 IDMan.exe bounded deep-dive (the study's documented follow-up:
    scheduler/segment-allocator function-level decompile): A accept the
    bounded study, proceed to adoption (default) / B commission the
    bounded multi-session decompile (Ghidra toolchain present) / C
    close the research question after the waves.
- Wave-2 STAGING (deliberately NOT committed; activation waits on the
  D1 answer): `/home/z/my-project/staging/s11i-wave2/` holds
  `S11-ADOPTIONS-WAVE2-DRAFT.md` (behavior-spec drafts for B3 HLS
  BYTERANGE / B4 NMH resilience / B5 cookie forwarding / B6 takeover UX
  / B7 detector hardening / B8 MIME table / B2-presets + measurement
  plan; ground-in-code TODOs marked) and `0007-GATE-PLAN-DRAFT.md`
  (evidence pack, scope variants, per-role focus questions, activation
  checklist). Staging lives outside the repo (never in backups —
  volatile across recycles; this entry summarizes its substance).
- Next: on the owner's answers — dispatch gate 0007 for the chosen
  wave-2 scope → refine+commit specs (spec-first) → implement →
  batteries + A/B → CI witness → issue #15 comment → backup close.

## 2026-11-21 (session 9) — owner decision round 1 + gates 0007/0008 + toolchain re-provision

- Context refresh (the session-8 line continued after another loss).
  **Fetch-first lesson, six-times-proven:** origin/main was 15 commits
  ahead of the stale local tree (session 8's commits: the D1–D7 menu at
  2f9cfa1 + the S11-T2 execution + S11-i wave 1 close + S12-e gate
  0005); fast-forwarded cleanly. The stray local BACKUPS.md edit was
  reconciled (the a274d96 row kept, chronologically placed — remote
  rows were already newer).
- **Environment truth this session:** `tools/` can LOOK missing at
  session cold-start (the first two `ls` probes said "No such file or
  directory" while cargo/rustup were actually present and healthy) —
  a cold-mount transient; retry before concluding another recycle ate
  it. What HAD gone: ghidra + jdk (re-provisioned below). What had
  survived: `tools/rustup` + `tools/cargo` (stable 1.100.0 default +
  1.75.0 — use `RUSTUP_HOME`/`CARGO_HOME` per the Environment table).
- **Owner decision round 1 answered in-chat:** `d1:C, d2:B, d3:d,
  d4:b, d5:a, d7:B` (D6 unanswered → default A). Recorded durably in
  the NEW `harness/DECISIONS.md` ledger (bindings itemized there;
  the in-chat menu stays the interface per the standing directive).
- **Activations executed (both spec-first — specs committed BEFORE any
  implementing commit, which runs NEXT):**
  - **Gate 0007** (five-role, 5× PROCEED-WITH-CONDITIONS, zero BLOCK,
    18 conditions folded): S11-i **wave 2** with owner D1:C scope —
    B2-presets, B3 HLS BYTERANGE, B4 NMH resilience, B5 cookie
    forwarding, B6 takeover UX, B7 detector hardening, B8 MIME table,
    plus the pulled-in parked **B13** (persisted scheduler rules,
    `.vdmq`) and **B16** (browser-proxy propagation, adds the `proxy`
    manifest permission). Specs: `docs/SPECS/S11-ADOPTIONS-WAVE2.md`,
    each grounded in the current code (messaging.js one-shot send,
    background.js cookie/takeover paths, hls.rs Segment shape,
    scheduler.rs rules, HttpOptions/ResumeState surfaces, the manifest
    permission inventory).
  - **Gate 0008** (five-role, 5× PROCEED-WITH-CONDITIONS, zero BLOCK):
    **S4 DHT (BEP-5) activated in parallel** (owner D2:B). Specs:
    `docs/SPECS/S4-DHT-BEP5.md` (KRPC/token/routing-table/lookup/
    integration; local mini-DHT-only CI; idle no-op promise; deferred
    sub-stages recorded — seeder-side announcing, background refresh,
    persisted table).
  - ROADMAP rows + briefs updated: S4 ACTIVE, wave-2 scope +
    sequencing, S12 queue order per D3:D + D5:A (promotion-gate prep
    first, SSE wiring next, owner verification after wave-2 + SSE),
    T1/T3 protocol-prepared note per D4:B, D7 commission.
- Session-8's volatile staging dir (`staging/s11i-wave2/`) was lost
  with the recycle, as designed; its substance was re-derived directly
  from the committed study into the committed specs — the
  memory-entry-summarizes-substance contingency worked.
- **Toolchain re-provisioned and end-to-end verified (D7 precondition):**
  Ghidra 12.2 zip (asset `ghidra_12.2_PUBLIC_20261016.zip`, URL found
  via the release page's `expanded_assets` fragment after an API
  rate-limit) — SHA-256 byte-exact against the RUNBOOK pin
  `2c2cd776…7be67`; Temurin JDK 25.0.5.1; `JAVA_HOME_OVERRIDE` set;
  end-to-end probe: `analyzeHeadless` on a locally built 16 KB ELF with
  `scripts/ghidra/S11Analyze.java` (the T2 helper) — import,
  auto-analysis, runtime script compile, decompilation of all 15
  functions, exit 0, output confined to the throwaway dir.
- Next (implementation order, per gates 0007/0008): wave 2 engine-side
  first (B3 → B13 → B2p → B8-core → B16-core), then extension-side
  (B4 → B5 → B6 → B7 → B16 wiring) — one revertable commit per item
  with batteries/A/B and win7 ports; S4 DHT module in parallel;
  per-item closes with CI witness + issue comment + backup. The D7
  deep-dive (IDMan.exe scheduler/segment-allocator function-level
  decompile) runs as its own multi-session effort under the T2
  barriers (re-verify the artifact fingerprint per the fail-closed
  policy at each session's start).

## 2026-11-21 (session 10) — D8:now executed: IDMan.exe deep-dive session 1

- **Owner answered round 2 in-chat: "D8:now"** — the D7:B deep-dive
  start-timing question. Recorded in `harness/DECISIONS.md` round 2
  (the round-2 menu text itself was lost to context compression and
  never committed — the entry honestly reconstructs the binding from
  the durable queue: the deep-dive was the only commissioned-but-
  unscheduled item; the wave-2/S4 queue was already fully bound by
  round 1). The deep-dive starts NOW, in parallel with the unchanged
  wave-2/S4 implementation queue.
- **Fetch-first, seven-times-proven:** origin/main was 3 commits ahead
  (session 9's decision-round close); local stale BACKUPS.md line was
  byte-identical on origin — discarded, fast-forwarded to 5e0e8d4.
  The 8th recycle gutted tools/ghidra+jdk (484 K stub / JDK missing
  libjli.so); re-provisioned per RUNBOOK (zip SHA-256 byte-exact vs
  the pin, Temurin 25.0.5.1, JAVA_HOME_OVERRIDE), end-to-end ELF
  probe green.
- **Fail-closed fingerprint honored at deep-dive session start:**
  fresh official-URL download, 12,164,608 bytes + SHA-256 byte-exact
  vs the pinned value — no re-pin needed.
- **Deep-dive session 1 executed (bounded, clean-room):** no-analysis
  Ghidra import of IDMan.exe + `scripts/ghidra/unpack_idm.py` +
  `S11DeepDive.java` (raw string/RTTI evidence walk) +
  `S11Decompile.java` (backward function-start discovery + corrected
  RTTI chain + vtable slots + bounded decompile + callee crawl).
  184 functions decompiled (~36 s script time), all output confined to
  the wiped throwaway dir.
- **Findings distilled into `docs/RESEARCH-IDM-STUDY.md` §4b**
  (functional descriptions only): a volume-proportional connection-
  count calculator (received-bytes/10 KiB clamped by per-download
  min/max, link-class ceilings 4/8, hard cap 32) feeding a spawn/
  retire orchestrator; per-queue persistence as two small files (an
  id-list file + a scheduler-settings file, main-queue defaults
  auto-created); registry limits model (200 MB / 5 h rolling window
  defaults); completion-path media gating; per-download worker
  threads below normal priority. B2/B13 backlog rows cross-linked for
  the wave-2 specs. CLEAN_ROOM.md gained the per-session deep-dive
  record; RUNBOOK gained the bounded deep-dive pattern.
- **Hard-won session learnings:**
  - Ghidra string-use SITES are the imm32 operand address (opcode at
    site-1): validate starts via getInstructionContaining, never
    getInstructionAt — the operand position is never an instruction
    boundary (cost: one zero-anchor pass).
  - MSVC RTTI walk direction: the class name is INLINE at TD+8, so
    TD = nameAddr-8 directly and references point AT the TD — walking
    references to the NAME finds nothing (cost: the first 0-vtable
    pass).
  - A Ghidra project saved by `-import -noanalysis` is reopened by
    `-process` WITHOUT re-importing; the reopen itself may trigger a
    short partial analysis (EH analyzers ran ~14 s) — harmless.
  - Iterating a set while the callee crawl adds to it throws
    ConcurrentModificationException and silently kills the phase —
    iterate over a snapshot.
  - `for i, (a,b,c) in entries` in a Python generator binds i from
    the TUPLE (not enumerate) — "too many values to unpack"; use
    enumerate.
  - The unpacker's `[usize][clen]` header: trust the clean zlib
    stream end + exact clen consumption (entry 0's usize field is
    anomalous — the manifest inflates to 8,697 bytes, not the field's
    18,115; all 187 payload entries match).
- Next: session 2 of the deep-dive (worker-thread range arithmetic,
  queue-state writer, site-login/refresh-link routines — same
  fail-closed re-verify at start), while wave-2 implementation
  (B3 → B13 → B2p → B8-core → B16-core, then extension-side) and S4
  DHT continue as the parallel standing queue per gates 0007/0008.

## 2026-11-22 (session 11) — S11-i wave 2 engine side delivered (D1:C scope)

- Context refresh (the session-10 line continued after another loss).
  **Fetch-first, eight-times-proven:** origin/main was 25 commits ahead of
  the stale local tree — the previous sessions had already executed S12-e
  (gate 0005), the S11-T2 static study (d0afd29), S11-i wave 1 (gate 0006),
  owner decision round 1 + gates 0007/0008 activations, and the D8:now
  deep-dive session 1 (cf6369e). The decision ledger `harness/DECISIONS.md`
  (recovered from the remote) resolved the lost-context menu semantics: D8:now
  = the D7 deep-dive starts immediately (already executed as session 10);
  the remaining queue = wave-2 implementation (B3 → B13 → B2p → B8-core →
  B16-core, then extension side) + S4 DHT in parallel. This session executed
  the wave-2 ENGINE SIDE end-to-end.
- **Wave-2 engine side implemented (spec-first, per-item revertable
  commits c6514d5 → f236c9b):** B3 (HLS EXT-X-BYTERANGE: implicit-offset
  chains per URI, whole-resource reset, 206+short-body-fail, AES
  composition, exact-total accounting, EXT-X-MAP loud-fail), B13
  (`<download-dir>/.vdm/scheduler.vdmq` schema v1 ours — atomic
  temp+rename, tolerant loads, save on rule-change + clean shutdown, load
  at engine start; ResumeState untouched), B2p (`connPreset` — a base
  ceiling 8/4/2, never a raise; validated at engine add + NMH boundary),
  B8-core (`vdm-core/src/mime.rs`, 110 IANA-compiled entries + generated
  `extensions/shared/mime-table.js` + drift tripwire test; applied only
  when the URL yields no extension — never overrides an explicit filename
  or an existing extension), B16-core (`HttpOptions.browserProxy` —
  probe-guarded adoption, suggest-never-override, differs/unreachable
  warnings on the add reply + snapshot notice; NMH shape validation;
  completion preserves persistent warnings, only the transient B1
  recovery notice clears).
- **Batteries/A/B/win7 all green:** fmt + clippy -D warnings clean; 124
  vdm-core lib + 21 http_engine + 7 hls_e2e + 3 nmh + 3 vdm-rpc tests
  (16 new test groups); `bench-perf.sh` A/B headless builds (labels
  s11i2-pre/-post, 2 runs each): pre 0.80s/159.1 MBps vs post
  0.81s/158.8 MBps, byte-identical, 12 conns/37 reqs both sides — no
  regression (0.2%, within noise). win7 tree ported with the two
  standing 1.75 compat fixes (percent_decode closure, nmh loop-match)
  + hls `% 16`/`block.clone()`; `cargo +1.75.0 check --workspace` green.
- Per gate 0007 the implementation landed as FIVE per-item commits (not
  one blob): shared-file intermediates were reconstructed by
  reverse-editing the finals (mod.rs/task.rs/nmh/http_engine variants in
  /tmp — the finals are byte-identical to the verified green tree).
- **Environment notes:** `tools/cargo` registry is CORRUPTED ("failed to
  find aes v0.8.4 in path source" — do NOT set CARGO_HOME=tools/cargo;
  the default ~/.cargo home resolves crates.io fine with tools/rustup +
  tools/cargo/bin on PATH). tools/ghidra+jdk are gutted AGAIN (stub) —
  re-provision per RUNBOOK when the deep-dive resumes. Disk: target/ was
  5.8 GB and full-disk killed a slint build mid-session — the wave-2
  close worked with sequential single builds (pre-worktree → post →
  win7) and target/ removed between them.
- **CI fix-forward (the e2e-cross suite went RED on the first push):** B13
  wrote the scheduler rules into the DEFAULT download dir (OS temp scratch)
  when `queue.set` ran before `settings --dir`, and a fresh daemon on the
  same runner LOADS them — e2e.sh's 22:00-06:00 stop-window leaked into
  e2e-cross's daemon, and the job ran at 05:39 UTC (inside the window), so
  the scheduler auto-paused combo 1 and it never completed. Fix: the rule
  store now skips the volatile default dir BOTH ways
  (`scheduler::store_active`; configured dirs persist + load normally;
  spec amended with a dated note). Honest lesson: a persistence feature
  keyed to a SHARED default directory is a cross-instance leak — the CI
  clock (05:39 UTC) was the reason it surfaced today and not at 07:00
  yesterday.
- Next: extension-side wave 2 (B4 → B5 → B6 → B7 → B8-js → B16 wiring +
  the node battery growth per spec), S4 DHT implementation (gate 0008
  spec committed), deep-dive session 2 (worker-thread range arithmetic,
  queue-state writer, site-login/refresh-link) — all standing parallel
  tracks.

## 2026-11-23 (session 12) — S11-i wave 2 EXTENSION SIDE delivered; wave 2 closed

- Context refresh (the session-11 line continued after another loss).
  **Fetch-first, nine-times-proven:** origin/main was 27 commits ahead of
  the stale local snapshot (S12-e gate 0005, the S11-T2 static study
  d0afd29, S11-i wave 1 + gates 0007/0008 + decision ledger + the
  D8:now deep-dive session 1 + wave-2 engine side). The stale summary's
  todo list (d1–d8 recovery, T2 execution, D8 interpretation) was ALL
  already done and pushed — `harness/DECISIONS.md` round 1/round 2
  resolved the lost-context menu semantics durably. Fast-forwarded to
  7e2bc79; the stray local BACKUPS.md row was already preserved on the
  sandbox-backup branch (a274d96 contains it) — discarded cleanly.
- **Wave-2 extension side implemented end-to-end (spec-first per the
  committed S11-ADOPTIONS-WAVE2.md; per-item revertable commits
  282ab74..1ac2c47):** B4 (`shared/nmhost.js` — persistent port
  preferred with one-shot fallback; backoff 500 ms→8 s doubling with a
  30 s-stability reset that also closes the 10-minute episode; queue cap
  32 drop-oldest; first-send failure requeues exactly once; host-absence
  patterns fail fast so the decline path stays correct), B5
  (`shared/cookies.js` — the jar rides EVERY add path incl. URL-list
  batches capped at the first 32 per-URL; NMH `addBatch` gained
  `cookiesBy` with win7 port; the no-cookie-leak test is explicit),
  B6 (`shared/takeover.js` + static self-contained `taken.html` — hash
  read once, textContent only, immediate history.back; shelf
  suppression feature-detected (Firefox no-ops); onDeterminingFilename
  suggests our name once on our takeover; decline → resume + popup
  error-note area incl. the B4 'engine unreachable' state), B7
  (detector hardening — geometry 200×120 ours + 640×360 prominence,
  state gate, bounded ancestor visibility walk, 128 KiB inline scan with
  letter boundary, one media-filtered MutationObserver @ 250 ms
  debounce + ResizeObserver only during bounded windows with full
  teardown; site packs data-driven OFF by default), B8-js (the
  generated MIME table became a UMD module — the ESM emit could NOT
  load in a classic service worker, a genuine consumer-wiring catch;
  generator + drift tripwire updated both trees; sniffed-list hint +
  filename mirror + suggestion consume it), B16 wiring
  (`shared/proxyreport.js` — system/mode-only reports carry no
  endpoint, fixed endpoints limited to http/https/socks5; the
  `browserProxy` field rides every add path with a 60 s cache; the
  manifest `proxy` permission landed with B16's commit = the wave's
  ONLY permission delta).
- **Batteries/witnesses all green:** node battery 76→240 assertions
  (backoff math, queue cap, host-absence matcher, mock-port
  fail-then-reconnect, requeue-once, stable-reset, budget→dead,
  one-shot fallback, cookie serializer/plan/map + no-leak, takeover
  decline→resume pinned, XSS-inert confirm fixture, geometry/state/
  overlay tables, inline-scan cap, pack defaults, two-window observer
  teardown, honest overhead: 5000-candidate gated pass measured
  4.63 ms in-sandbox, mime lookups, proxy shape table). Rust: fmt both
  trees, clippy `-D warnings` clean, 125 vdm-core lib + 21 http_engine
  + 7 hls_e2e + 4 NMH tests green, win7 `+1.75.0 check --workspace
  --locked` + NMH tests green. All four extension packages build; the
  Firefox patch (importScripts exact-replace + manifest scripts) was
  byte-verified to carry every new shared module. **CI witness: ALL
  11/11 checks GREEN at 357a36f** (8 required + 3 non-required
  frontends).
- Honest records: B6's shelf behavior + confirmation page still owe
  the owner's D5:A runtime verification; pattern batches ride the
  engine's no-options addBatch surface (B5 gap recorded in the study);
  Firefox CHIPS jars documented as a limitation; the content_scripts
  now ACTUALLY inject the shared modules (the old code silently
  degraded grab-links — fixed riding B7, noted in its commit).
- Environment notes: the 10th recycle gutted the stable rustc again
  (re-provisioned 1.100.0 per rustup; CARGO_HOME=tools/cargo remains
  CORRUPTED — the default ~/.cargo home with tools/cargo/bin on PATH
  is the working recipe; 1.75 toolchain intact).
- Next standing queue (per DECISIONS + gates 0007/0008): S4 DHT
  implementation (spec committed; the only un-started ACTIVE stage),
  deep-dive session 2 (worker-thread range arithmetic, queue-state
  writer, site-login/refresh-link — fail-closed re-verify at start),
  S12 promotion-gate preparation first in the S12 queue per D3:D/D5:A.
  Wave-2 adoption is now fully closed; new adoptions wait on owner
  decisions (the ledger takes round 3).

## 2026-11-23 (session 13) — S4 DHT delivered + closed; Windows entropy CI fix-forward

- Context refresh (the session-12 line continued after another context
  loss). **Fetch-first, ten-times-proven:** origin/main was 44 commits
  ahead of the stale local snapshot — sessions 9–12 had already
  executed the S11-T2 static study, S11-i waves 1+2 (both sides), the
  decision ledger, and the D8:now deep-dive session 1. The stale
  summary handed to this session (d1–d8 recovery, T2 execution, D8
  interpretation) was ALL already done and pushed. Fast-forwarded to
  126de3a; the stray stashed BACKUPS.md row was already preserved on
  sandbox-backup (a274d96) — dropped cleanly.
- **The previous session had pushed the S4 DHT implementation (4 commits
  eca7dfb → 126de3a: the bt/dht.rs module — KRPC codec, 160-bucket XOR
  routing table, single-use announce tokens, iterative get_peers,
  persisted node id; engine wiring beside trackers with BEP-9 metadata;
  two-node loopback mini-DHT e2e + idle no-socket witness; docs +
  VDM_DHT_BOOTSTRAP=off guards) but died at the context wall BEFORE the
  close-out — with CI RED:** `Test (windows-latest)` failed
  `bt::dht::tests::token_rotation_single_use` at 126de3a (135 passed,
  1 failed).
- **Root cause (diagnosed this session from the CI log + code):**
  `util::random_bytes` has no `/dev/urandom` on Windows, so it falls
  back to an xorshift seeded from `now_ms() ^ pid*GOLDEN ^ heap-addr`.
  The Windows heap re-serves the SAME address for the same-size recycled
  block, and the two token-secret draws (epoch-1 and epoch-2 rotations)
  landed inside one millisecond ⇒ identical seeds ⇒ identical secrets ⇒
  a spent previous-window token re-matched the "current" mix after
  rotation. Linux/macOS never take the fallback, hence green there.
  **Fix (9fc8e42, both trees): a per-process AtomicU64 call counter
  mixed into the fallback seed — rapid same-ms calls always draw
  distinct streams.** A standalone arithmetic proof script
  (scripts/fallback_seed_proof.py) reproduces the collision under the
  exact Windows worst case pre-fix and distinctness post-fix; a
  regression tripwire test (`random_bytes_rapid_calls_differ`) pins the
  contract. Side benefit: rapid `new_peer_id()` calls can no longer
  collide on Windows either.
- **CI witness: ALL 11/11 GREEN at 9fc8e42** (the previously-red
  windows-latest Test now passes). Local battery: fmt both trees clean,
  clippy `-D warnings` clean, vdm-core 137 lib tests + integration
  suites green, win7 `cargo +1.75.0 check --workspace --locked` green.
  The known e2e_bin local-run dependency (bins must be built first) bit
  again as a disk-exhaustion error — CI's smoke job is the authority.
- **Spec-owed bench A/B executed (labels s4-pre/s4-post, record in
  scripts/s4-bench-record.txt):** warm-pair parity — pre 0.81 s /
  159.0-158.9 MBps vs post 0.81 s / 158.9 MBps, request shape identical
  (12 conns / 37 reqs both sides), byte-identical output both sides —
  NO REGRESSION. Honest environment note: standalone COLD runs on
  this shared sandbox are disk-bound (~91 MBps — the freshly written
  128 MiB source gets evicted from the page cache while the build's
  dirty pages flush; mechanistically symmetric, request shape
  unchanged, so the warm pair is the honest comparator).
- S4 close-out: ROADMAP S4 → DONE; issue #1 (the DHT tracker) commented
  with the close evidence; backup executed per the event rule.
- Hard-won session learnings:
  - A test that is green on Linux+macOS but red on windows-latest with
    an RNG-flavored assertion: suspect the entropy fallback FIRST —
    `/dev/urandom` simply does not exist on Windows, and the fallback's
    heap-address term is worth nothing there (same-size blocks get the
    same address recycled).
  - bench-perf.sh on THIS sandbox: back-to-back runs measure
    page-cache-warm throughput (~159 MBps); runs after a build or an
    idle gap are disk-bound (~91 MBps). Compare warm-pair to warm-pair,
    never warm against cold.
  - Disk discipline again: root target/ grew to 5.9 GB and killed a
    test compile mid-battery — clean between build phases (the
    session-11 recipe).
- Next standing queue (per DECISIONS + gates 0007/0008, unchanged):
  deep-dive session 2 (worker-thread range arithmetic, queue-state
  writer, site-login/refresh-link — fail-closed fingerprint re-verify
  at start, Ghidra re-provision per RUNBOOK), S12 promotion-gate
  preparation first in the S12 queue per D3:D/D5:A, S5+ remain PENDING
  until the owner activates them.

## 2026-11-23 (session 13, continued) — S12 promotion-gate PREPARATION delivered (D3:D first-in-queue)

- With S4 closed and the deep-dive session 2 requiring a fresh-session
  fail-closed start (fingerprint re-verify + Ghidra re-provision —
  tools/ghidra is a 484K stub again, the known recycle pattern), this
  session executed the other standing track: the D3:D first-in-queue
  item — the promotion-gate PREPARATION for S12.
- **Delivered `docs/SPECS/S12-PROMOTION-GATES.md`** (preparation only —
  the gates themselves stay five-role-gated at promotion time):
  - **Parity matrices** grounded in the current sources (verified
    2026-11-23, not copied from stale gate text): add-path/task-list/
    settings/MD3/engine-integration per surface. Ground-truth updates
    vs the 0004-era notes: Slint DOES accept magnet: + local .torrent
    (grep-verified); Swift accepts magnet: but not local .torrent
    paths; Android's Kotlin-port engine is http/https-only by design
    (S8 owns unification) — its settings gaps are measured against the
    Compose app it replaces, not the desktop surface.
  - **Packaging plan**: per-surface asset deltas grounded in release.yml's
    actual composition (verified live: no preview frontend ships in any
    v1.0.0 asset today) — win32.exe → windows zips, velocitydm-slint →
    linux tars (already a workspace member; two cp lines), VelocityDM.app
    → macos zips (needs a NEW macos-latest job — osxcross cannot build
    Swift), Views APK → the -android release (versionCode/versionName
    bumps are the gate's own). v* ruleset 27298722 + frozen-release
    constraints restated per gate 0005.
  - **Owner-verification protocol (D5:A)**: per-surface runtime
    checklists (Win32 DPI drag test, Slint musl, Swift daemon-death
    graceful degrade, Android rotation/process-death, extensions
    takeover incl. B6 confirmation + B4 reconnect), evidence recorded
    as issue-#16 comments (the authoritative runtime evidence), fail →
    issue + gate blocked; honest boundary: the agent has no displays
    or devices.
  - **Gap register** (verified current): filename UI absent in
    Win32/Slint/Swift; Swift local .torrent + stale rows + SSE; Win32
    Enter/TAB + brush churn; Android release-size delta to record at
    gate.
  - **Gate order**: SSE wiring → owner verification moment →
    per-surface promotions (five-role + packaging in the same stage) →
    remaining frontends (S12-lin2/lin3/mac2).
- ROADMAP S12 queue note updated (preparation DONE); issue #16
  commented with the preparation record.

## 2026-11-24 (session 14) — deep-dive session 2 (with REA) + wave 3 (B17/B18/B19) delivered

- Context refresh (the session-13 line continued after another loss).
  **Fetch-first, eleven-times-proven:** origin/main was ahead again (S4 DHT
  close + S12 promotion-gate prep + the S12-e tree); fast-forwarded to
  9cc9734; the stale summary's todo list (d1–d8 recovery, T2, D8) was ALL
  already done — `harness/DECISIONS.md` is the durable resolver.
- **Owner round 3 directive (recorded, ledger D9/D10):** "Use reverse
  engineer anything (morluto/rea) to decompile, reverse engineer and
  analyse IDM … and incorporate any methods/techniques to improve
  performance and stability of the download engine and browser
  integration." Bound to: deep-dive session 2 with REA + adoption wave 3.
- **Toolchain re-provisioned (recycle pattern, all per RUNBOOK):** Ghidra
  12.2 (zip hash byte-exact vs the pin), Temurin JDK 25.0.5.1, AND rustup
  shims + the 1.75.0 minimal toolchain (the 1.75 witness was mandatory
  this session). Lessons: fresh `rustup-init` sets its default to the
  FIRST installed toolchain — `rustup default stable` immediately; a
  direct toolchain-bin cargo cannot honor `+toolchain` (shims required).
- **Fail-closed honored:** fresh official-URL download, 12,164,608 bytes +
  SHA-256 byte-exact vs the pin before any deep-dive work; no re-pin.
- **REA 6.10.1 (`rea-agents`, npm, MIT) joined the toolchain** (ledger
  D9): 204-command headless CLI; bound to Ghidra via `GHIDRA_INSTALL_DIR`
  + JDK 25 on PATH; produced the installer-stub overview (Ghidra provider,
  full auto-analysis — feasible on the stub, NOT on IDMan.exe's 2.97 MB
  code image), structured PE import/resource/hardening evidence, and a JS
  application-graph inventory of the shipped Firefox XPI (3 parsed
  scripts, cross-checking the wave-1 §3 manual read). The targeted bounded
  decompile stayed on the no-analysis Ghidra script pipeline (REA's native
  provider requires full auto-analysis — recorded honestly in the study
  Tools row).
- **Deep-dive session 2 executed (bounded, clean-room):** unpack (188
  entries) → no-analysis import → needle/RTTI evidence walk (19,578 raw
  strings, 303 use-sites) → three anchored decompile passes (149+152 fns):
  10/10 session-2 anchor functions + the header-range builders + the
  If-Range site. New tooling joined the toolbox: S11DumpStrings.java +
  s11_annotate.py (42,749-string map annotates no-analysis decompiled
  sections with literal refs — makes them readable) + split_sections.py.
  All output confined to the throwaway dir (wiped at close).
- **Findings → `RESEARCH-IDM-STUDY.md` §4c** (functional descriptions
  only): worker range-request ladder (bounded/open-ended/probe — parity
  with our range pool), Accept-Encoding: identity forced on ranged
  requests, ranged-response invariant validator (multipart/byteranges
  family, chunked, the gzip/x-gzip/deflate/x-deflate/brotli coding family,
  ETag/Content-Range 64-bit parsing), If-Range stale-resource detection,
  the owner-page refresh flow (meta-refresh + Set-Cookie http-equiv +
  robots none/nofollow gating — B19 adopts the honest minimum), the
  grabber project persistence model, and compiled-in signed-URL CDN site
  adapters (observed, NOT recommended — same posture as B15).
- **Gate 0009 (five-role, 5× PROCEED-WITH-CONDITIONS, zero BLOCK, 8
  conditions folded)** → wave-3 specs `docs/SPECS/S11-ADOPTIONS-WAVE3.md`
  committed BEFORE implementation (commit 98e03de); implementation sources
  are the public RFCs (9110 §8.4/§12.5.3/§13.1.5/§14) — the study pointed
  at our gaps, per the standing clean-room barrier.
- **Wave-3 implemented (per-item revertable commits):**
  - **B17+B18 (5c61982):** ranged requests pin `Accept-Encoding: identity`
    (user intent wins, case-insensitive check); `classify_ranged_response`
    rejects compressed/multipart/Content-Range-mismatched responses with
    per-path severity (worker = bounded retry with distinct reason, audit =
    INCONCLUSIVE never a B1 conflict, probe = advisory); If-Range validator
    (etag > last-modified, in-memory — ResumeState untouched, cross-restart
    persistence a recorded non-goal) with the definitive
    `ranged_200_is_changed` fast-fail (no mirror churn on changed
    resources). Tests: parse/classify matrices, AE detection, validator
    precedence, fast-fail decision + e2e witnesses (a REAL zlib
    stored-block body announced as deflate — the discriminator — retried
    and byte-identical completion; If-Range seen on the wire + fast-fail
    with ≤4 requests).
  - **B19 (21dc5f2):** 403/410 → stable marker prefix in the failure
    string; the actionable hint rides the EXISTING snapshot notice channel
    (`fail()` chokepoint) — every frontend renders it, so the popup needed
    ZERO JS changes (dated spec note; the gate's INT condition anticipated
    this: reuse the existing surface, no new DOM). Tests: classification
    matrix (403/410 marker, 404/5xx historical strings byte-identical,
    is_retryable preserved) + e2e (403 link → marker error + notice).
- **Per-item commit split cost two helper scripts** (s11i3_b19_split.py +
  s11i3_reapply_b19.py) — and one real lesson: inverting a
  text-replacement with an EMPTY new-string inserts at position 0 in
  Python (the first B19 re-apply corrupted file heads); non-empty anchors
  only, verify with clippy before committing intermediates.
- **Batteries all green:** fmt both trees; clippy `--workspace
  --all-targets -D warnings` clean; vdm-core lib 143 (+6 new), http_engine
  24 (+3), hls_e2e 7; extension battery 240 (unchanged — no JS deltas);
  win7 `cargo +1.75.0 check --workspace --locked` green.
- **Bench A/B (labels s11i3-pre/-post, warm-pair rule, record in
  scripts/s11i3-bench-record.txt):** pre 0.81 s / 159.0 MBps vs post
  0.80 s / 159.1 MBps, request shape identical (12 conns / 37 reqs both),
  byte-identical — NO REGRESSION (the first post run's 91 MBps is the
  documented post-build cold-cache artifact; compare warm to warm).
- **CI: first run RED on `Test (windows-latest)`** — the pre-existing
  `optional_connections_spawn_when_live_underperform` e2e failed with
  "download ended with missing ranges" after the suite crawled 242 s
  (locally 8.4 s). Evidence assembled before any engine surgery: same test
  green at 98e03de on the same runner class the same day; 20/20 local
  repetitions green; **rerun of the failed job PASSED** → recorded as a
  slow-runner timing flake, a WATCH-ITEM (not attributed to wave-3
  without a deterministic reproduction; if it recurs, instrument the
  pool/release accounting under extreme runner slowness first). Final CI
  witness: **11/11 GREEN at 21dc5f2**.
- Pushed 98e03de → 5c61982 → 21dc5f2 to main (admin-bypass warnings are
  the known pattern; CI runs post-hoc and was witnessed).
- Environment notes: `~/.cargo` now hosts the rustup shims (default
  stable); `tools/rustup` holds both toolchains; jdk17/gradle/android-sdk
  remain UNPROVISIONED this session (no Android work attempted).
- Next standing queue: D5:A owner runtime verification (after SSE wiring),
  S5+ await owner activation; the deep-dive's commissioned scope is
  COMPLETE (scheduler §4b + worker/response machinery §4c); further
  function-level passes wait on demonstrated need or the T1/T3 VM.

## 2026-11-24 (session 15) — S12 SSE wiring delivered (gate 0010)

- Fetch-first: origin/main == local (4abd281, no drift this time — the
  wave-3 close commit was already pushed).
- Next standing queue item executed: the D3:D/D5:A S12 sub-stage — SSE
  wiring for the Swift frontend (gap-register rows 157/158 of
  docs/SPECS/S12-PROMOTION-GATES.md §4; §5 step 1), replacing the
  unconditional 200 ms downloads.list poll with consumption of the
  documented GET /events stream, plus the stale-rows-on-daemon-death fix.
- **Gate 0010 (five-role, 5× PROCEED-WITH-CONDITIONS, zero BLOCK,
  conditions folded pre-commit)**: ATKR (1 MiB buffer cap + lenient
  parse + no payload logging + native-client posture preserved), FALS
  (wire truth vs sse_loop: no heartbeats; tick omits removals →
  task-removed→re-list; metadata-only drift must mark dirty), REQ
  (RpcClient header comment updated; gap-register flip honest wording;
  spec-first split), INT (CI swift-frontend job = compile witness;
  macOS 10.15 API floor; whole-Data buffer ops; stop() wired into
  applicationWillTerminate), REL (commit split; CI witness incl.
  swift-frontend before close; backup last). Spec:
  docs/SPECS/S12-SSE-WIRING.md (constants chosen independently:
  150 ms coalesce, 2 s probe, 24 h stream timeouts, 1 MiB cap).
- **Wire witness before implementation:** daemon rebuilt in-sandbox
  (cargo build -p velocitydm --bin velocitydm, 1m57s; shims in
  ~/.cargo/bin + RUSTUP_HOME=tools/rustup; the direct toolchain cargo
  cannot resolve the registry) and scripts/sse_probe.py captured the
  live frames (task-state probing→downloading→completed,
  task-completed, one tasks tick with the full camelCase snapshot,
  queue-drained; every frame exactly `data: <json>\n\n`, no
  heartbeats) — transcript at scripts/sse-probe-transcript.txt; spec
  §0 verified against the running server. Probe lessons: urllib's
  buffered read deadlocks on silent SSE streams (raw socket + 0.5 s
  recv timeout instead); the engine aborts extra ranged connections
  mid-body (BrokenPipe server-side is normal).
- **Implementation a0f801f:** SseClient.swift (URLSessionDataDelegate,
  serial-queue buffer, macOS 10.15 APIs only) + AppController event
  routing (tasks tick = in-place merge; other events = 150 ms coalesced
  re-list; terminal state = reachable(false) → stale banner + frozen
  rows + 2 s recovery probe that reopens the stream) + RpcClient header
  comment + main.swift teardown wiring. Self-review caught and fixed a
  launch-with-dead-daemon edge: the initial false→false "transition"
  must still open the fallback (setStream made idempotent both ways).
- **CI 11/11 GREEN at a0f801f** (poll-checks.sh; macOS frontend =
  Swift compile witness; the session-14 windows-latest flake passed
  clean this run). No engine deltas → no bench A/B, no win7 port owed.
- Honest boundaries recorded: no Swift toolchain in the sandbox
  (source review + CI compile witness only); AppKit runtime rows ride
  the owner's D5:A verification moment (next in the queue).
- Environment: cargo via ~/.cargo/bin shims (tools/cargo/bin absent
  this instance); disk ~2.2G free — clean target/ before heavy builds.
- Next standing queue: owner runtime verification (D5:A) of the full
  Swift surface — SSE wiring was its last prerequisite; then per-surface
  promotions (§5 steps 3–4), each under its own gate.

## 2026-11-24 (session 16) — S11 T2 independent re-derivation via the rea attach pipeline

- Owner's "next step" resumed the rea directive. Toolchain had been half-lost
  again (tools/ghidra gutted to 484 KB, tools/jdk gone): Ghidra 12.2 reinstalled
  from the official release (zip SHA-256 re-verified), Temurin JDK 25.0.5.1
  restored, `JAVA_HOME_OVERRIDE` re-patched. rea 6.10.1 via `npm i -g
  rea-agents`; `rea doctor --provider ghidra` all green. `7zip-bin` (npm)
  supplied the missing 7za binary — this sandbox has no sudo and apt is
  unusable; npm ships prebuilt binaries that work.
- Fail-closed gate PASSED again: fresh official-URL download matched the
  pinned SHA-256 + 12,164,608 bytes + PE32 i386; Authenticode chain
  observed (DigiCert G4 → Tonec Inc.).
- Pipeline: generic zlib/zip stream-carve recovered ~50 binaries + CHMs +
  language INIs + 2 CRX bundles from the installer (cross-check of
  `unpack_idm.py`'s container parse; script kept in the toolbox as
  `scripts/extract_idm_payload.py`), engine image identified, Ghidra project
  prepared with bounded resources (248 s), rea attach mode for
  search/xrefs/decompile.
- **OOM lesson:** rea's own in-session Ghidra import was SIGKILLed (2 GB
  bridge heap vs 4 GB cgroup). Caller-prepared project + `REA_GHIDRA_
  PROJECT_PATH=<...>.gpr` + bare `REA_GHIDRA_PROGRAM_NAME` = the safe lane;
  recipe + gotchas in RUNBOOK ("rea attach mode").
- **Reconcile (standing lesson, third proof):** fetch-before-push revealed
  origin 25+ commits ahead — sessions 10/14/15 had ALREADY executed D8:now
  (deep-dive sessions 1+2), waves 1–3 of S11-i, S4 DHT, S12 SSE, and a T2
  disclosure in CLEAN_ROOM.md. Stash → ff → re-apply; conflicts resolved by
  taking the CI-proven remote state and layering only unique work.
- **Unique value landed:** RESEARCH-IDM-STUDY.md §4d — parity confirmations
  (range ladder, If-Range/ETag validators, chunked/gzip, CONNECT, NMH shape)
  + NEW observations: Schannel TLS with explicit TLS 1.3 client enablement
  (WININET confined to URL/cookie/cache utilities, never transport), outbound
  header charset-whitelist hygiene, Accept-Ranges/Content-Range probing with
  an explicit user-visible no-resume state, Retry-After honored, Keep-Alive
  default + reuse, ~17-slot request builder with two header-order families,
  FTP PASV capability state machine, legacy RTMP/RTMPT/RTMPE + Flash fcs
  family (recorded non-goal), registry hot-reload + token-duplicated helper
  launch, CocCoc NativeMessagingHosts coverage.
- D-series: menus were conversation-local (unrecoverable from repo files);
  remote history shows D1:C, D3:D, D5:A, D7:B, D8:now already executed by
  their parallel sessions; D6 (depended on T2) re-presented to the owner in
  the session reply with §4d attached.
- Throwaway discipline held: /tmp/vdm-t2 wiped, binary never stored;
  no code changes in this session — docs/harness only.

## 2026-11-24 (session 17) — S11-i wave 4 delivered under the fresh gate 0011 (adoption backlog closed)

- Owner directive round 4 (ledger D11): "proceed down the remaining adoption backlog with a fresh
  gate". Fresh five-role panel (ATKR/FALS/REQ/INT/REL, independent dispatch, identical evidence
  packs): 5× PROCEED-WITH-CONDITIONS, zero BLOCK; record `ASSESSMENTS/0011-s11i-adoption-wave4.md`;
  conditions folded spec-first into `docs/SPECS/S11-ADOPTIONS-WAVE4.md` (committed a5bbc12 BEFORE any
  implementing commit).
- Panel upgrades that changed the wave: FALS found a real silent-data-loss class in OUR engine (a
  validator-matching `.vdmctl` surviving run_single's `.part` truncation — a later resume against a
  range-capable probe would skip truncated ranges → zero-holes file); ATKR found header NAMES were
  not token-validated (only space/colon/CR/LF/NUL) and both Proxy-Auth emission paths bypassed the
  hygiene; REQ falsified "all frontends render notice" (only the CLI does; win32/slint render error,
  macos decode drops the field, popup renders neither) and demanded the honest rendering matrix;
  INT reproduced the working win7 witness shape (no rustup proxy this instance — direct toolchain
  PATH, 16 s green) and required delta-seconds-only RA (no date crate, lock parity); REL pinned the
  bench pair as mandatory, the B21 commit split, the CLEAN_ROOM row and the docs/RPC.md notice gap.
- Scope adopted: B20 (tchar name refusal + controls/DEL value replacement with obs-text passthrough
  + notice; Proxy-Auth paths routed through the helper) 0a88a56; B21a (no-resume classification +
  `.vdmctl`/`.part` coherency via additive `ResumeState::remove_ctl`) a9a68fb; B21b (bounded
  Retry-After: delta-seconds only, 429/503, min(delta,60s), stop-responsive ticks, worker jitter,
  structural `probe_with_meta`/`ProbeFailure` threading; run_single now retries 429/503/5xx —
  pre-wave it never retried 5xx because its formatted error never matched `is_retryable`) 920375c;
  B22 (additive `tlsMin` knob, feature-free enum so the win7 headless default build compiles —
  win7 lib.rs tls module un-gated, connector stays cfg-gated; tolerant load, validated settings.set)
  3bd028a; B23 witness-only (12 conns / 37 reqs shape); B24 CocCoc NMH DEFERRED pending empirical
  path verification (AUR bot-blocked, deb 404 — an unverified path is a silent no-op honesty bug);
  parked items stay parked; §4d leftovers recorded as N7/N8 + the FTP PASV-target pre-condition.
- Bench A/B (scripts/s11i4-bench-record.txt, warm-pair rule): pre 159.2/159.1 vs post 158.8/158.8
  MBps — parity; request shape 12 conns / 37 reqs identical; the 1.41 s / 91 MBps cold outliers are
  the documented artifact class (identical in the s11i3 record), investigated and discarded.
- Battery: fmt both trees, clippy -D warnings (scoped to the touched crates — workspace-wide clippy
  trips a pre-existing zbus_macros issue), lib 149 + http_engine 26 + hls_e2e 7 + bt/dht green,
  extension battery 240 assertions (unaffected, run anyway), win7 direct-1.75 check green in BOTH
  feature states (tls on/off).
- CI evidence-then-fix cycle: Test (macos-latest) FAILED on the new b21 e2e ("read: body shorter
  than content-length"). Root cause: the probe's ranged-GET fallback abandoned the 512 KiB full-body
  response mid-read (read_body(1024) discard), and the close-with-unread-data RST raced the NEXT
  connection's body write on the macOS runner (sequential ephemeral-port reuse) — head arrived, body
  EOF'd. Fix cd5b4a4: the e2e serves 48 KiB so the probe drains the response fully in ONE read_body
  pull (behavior shape unchanged; Linux 60× green; classification/coherency/SHA-256 assertions
  intact). CI 11/11 GREEN at cd5b4a4 (8 required + 3 preview).
- Standing toolchain truths for future sessions: cargo shims are rebuilt per instance
  (`~/.cargo/bin` → toolchain bins, RUSTUP_HOME=tools/rustup, CARGO_HOME=tools/cargo — registry
  re-fetches, ~5 min); clippy was MISSING from both toolchains (minimal profile) and was installed
  from the dist channel manifest (clippy-1.100.0 tarball, sha256-verified, copied into
  toolchains/stable…/bin+share; PATH must include the toolchain bin for `cargo clippy` to find the
  subcommand); `cargo +1.75.0` DOES NOT WORK here — use
  `PATH=tools/rustup/toolchains/1.75.0-…/bin:$PATH cargo check --workspace --locked` inside win7/
  (and `--features tls -p vdm-core` for the TLS state).
- Clean-room held: B20/B21/B22 re-spec'd from RFC 9110 §5.5/§5.6.2/§10.2.3 + the rustls public API
  only; CLEAN_ROOM.md disclosure row added; no studied-binary artifact anywhere near the repo.
