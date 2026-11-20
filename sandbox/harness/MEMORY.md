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
