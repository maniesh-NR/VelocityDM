# Staged Roadmap

Each stage transition is gated by the adversarial consensus protocol
(`PROTOCOL.md`). Status legend: `DONE`, `GATED` (consensus recorded),
`ACTIVE`, `PENDING`.

| # | Stage | Status | Gate record |
| --- | --- | --- | --- |
| S0 | Engine + repository rebuild (clean-room IDM-style core, CI workflows, packaging, extension trees) | DONE (retroactive) | — |
| S1 | Material You across all platforms + Android rewire + agent harness | DONE (gate 0001b) | `ASSESSMENTS/0001-material-you-and-1.0.0-release.md` |
| S2 | Publish v1.0.0: push main + 6 tags, releases with assets, repo config, roadmap issues | DONE (2026-11-13) | `harness/BACKUPS.md` + MEMORY.md entry |
| S3 | Hardening: branch protection (8 required checks), verify all releases carry assets, CI green | DONE (2026-11-13) | MEMORY.md entry |
| S1R | Post-loss reconciliation + security hardening + doc truth + republish | DONE (2026-11-13; published at b27549a) | `ASSESSMENTS/0002b-round2-consensus.md` |
| S4 | DHT (BEP-5) so trackerless magnets resolve | DONE (2026-11-23: gate 0008 + spec → implementation eca7dfb..126de3a — `bt/dht.rs` KRPC/160-bucket table/single-use tokens/iterative lookup + engine wiring + two-node loopback e2e + idle no-op witness; Windows-entropy CI fix-forward 9fc8e42 (random_bytes fallback call counter); CI 11/11 green at 9fc8e42; bench A/B s4-pre/s4-post no-regression; issue #1 commented) | `ASSESSMENTS/0008-s4-dht-activation.md` |
| S5 | UDP trackers (BEP-15) | DONE (2026-11-24: gate 0014 (5× PWC, zero BLOCK) + spec → implementation 9437e59 — `udp_announce` per BEP-15: connected-socket source pinning, one retransmit reusing tid+datagram (5s/5s), action-3 error WITHOUT code field, sanitized untrusted text, 2048B recv buffer, V4-filtered/non-unicast URL rules, 20s/tracker ceiling; announce_all scheme dispatch + NEW 32-tracker round cap; UDP harness modes drop-first/short/error/wrong-tid; battery 219 green; bench parity 12 conns/37 reqs (scripts/s5-bench-record.txt); 1.75 witness check --workspace --locked 12s (all-targets blocked by getrandom 0.4.3 edition2024 in the win7 DEV graph — recorded); doc truth flips landed) | `ASSESSMENTS/0014-s5-udp-trackers.md` |
| S6 | DASH downloader (.mpd manifests) | PENDING | — |
| S7 | HLS: SAMPLE-AES + fMP4 EXT-X-MAP | PENDING | — |
| S8 | Android ↔ desktop daemon over LAN (shared RPC); also owns the Android engine unification onto the shared Rust core (the Android app ships a Kotlin-port engine until S8) | PENDING | — |
| S9 | Safari native app extension (Xcode conversion) | PENDING | — |
| S10 | Flathub submission | PENDING | — |
| S11 | IDM competitive study: RE-assisted review of the reference product's download-management & browser-integration methods (performance / stability / resource use) | ACTIVE (T2 executed 2026-11-21: findings + backlog in `docs/RESEARCH-IDM-STUDY.md`; D7:B/D8:now deep-dive session 1 (§4b) + session 2 with REA (§4c, 2026-11-24) delivered; T1/T3 remain gated on an owner-provisioned Windows VM with the observation protocol PREPARED per D4:B (2026-11-24, `docs/T1-T3-OBSERVATION-PROTOCOL.md`); adoption runs as S11-i) | — |
| S11-i | S11 incorporation waves: adopt study-backlog techniques spec-first (wave 1 = B1 cross-connection content audit + single-connection recovery, B2 optional connections, B9 file-lock classification/retry → engine; wave 2 = B2-presets + B3 HLS BYTERANGE + B4 NMH resilience + B5 cookie forwarding + B6 takeover UX + B7 detector hardening + B8 MIME table + B13 persisted scheduler rules + B16 browser-proxy propagation → engine/extensions; B10/B11/B12/B14/B15 parked with observe-first; N1–N6 rejected) | ACTIVE (wave 1 DONE 2026-11-21: gate 0006 + specs + implementation + A/B + CI 11/11 at 73b98bd; wave 2 DONE 2026-11-23 under gate 0007 with owner D1:C scope — engine side 2026-11-22 (B3 BYTERANGE + B13 .vdmq + B2p presets + B8 MIME table + B16 proxy propagation, per-item commits c6514d5..f236c9b, A/B no-regression) + EXTENSION SIDE 2026-11-23 (B4 NM host resilience + B5 cookies on every add path + B6 takeover UX/taken.html + B7 detector hardening + B8-js UMD table consumption + B16 report wiring incl. the `proxy` permission, per-item commits 282ab74..1ac2c47, battery 76→240 assertions, CI 11/11 at 357a36f); both wave-2 sides closed); **wave 3 DONE 2026-11-24 under gate 0009 on the owner's round-3 REA directive** — B17 (ranged-request identity coding + response invariants), B18 (If-Range stale-resource fast path), B19 (expired-link guidance) landed (98e03de → 21dc5f2, CI 11/11 at 21dc5f2); **wave 4 DONE 2026-11-24 under the fresh gate 0011 on the owner's round-4 directive ("proceed down the remaining adoption backlog")** — B20 (header hygiene: tchar name refusal + controls/DEL replacement, obs-text passthrough, 0a88a56), B21 (no-resume honesty + `.vdmctl`/`.part` coherency a9a68fb + bounded delta-seconds Retry-After with structural status threading 920375c), B22 (additive `tlsMin` knob 3bd028a), B23 witness (12 conns/37 reqs parity), B24 CocCoc NMH deferred pending empirical path verification; B10–B15 stay parked; N7/N8 + FTP PASV pre-condition recorded; bench parity s11i4-pre/post (scripts/s11i4-bench-record.txt); CI 11/11 at cd5b4a4 after the evidence-then-fix macOS cycle; the §4d backlog is now fully adopted, witnessed, deferred-with-precondition, or recorded as a non-goal — nothing silently dropped | `ASSESSMENTS/0006-s11i-adoption-wave1.md`, `ASSESSMENTS/0007-s11i-adoption-wave2.md`, `ASSESSMENTS/0009-s11i-adoption-wave3.md`, `ASSESSMENTS/0011-s11i-adoption-wave4.md` |
| S12 | Native UI stacks per OS around the shared Rust core — Material You everywhere, OS-specific elements kept (Android leg runs on the Kotlin-port engine today; Rust-core unification is S8's) | ACTIVE (foundation + Win32/D2D + Slint + Swift/AppKit built; S12-e Android Views+KMP built (preview) under gate 0005; S12-lin2 GTK3 built (preview) under gate 0012; S12-mac2 SwiftUI built (preview) under gate 0013; S12-lin3 Qt remains a gated sub-stage) | `ASSESSMENTS/0004-s12-native-frontends.md` + `ASSESSMENTS/0012-s12-lin2-gtk3.md` + `ASSESSMENTS/0013-s12-mac2-swiftui.md` |

## Stage briefs

- **S1**: Material You (MD3 dynamic color) on every shipped UI surface —
  desktop egui (Win 10/11, Linux glibc/musl, macOS), Android (dynamic
  wallpaper color on 12+, seeded fallback), the four shipped browser
  extensions (Chromium/Edge/Opera/Firefox). The Windows 7 payload is
  headless by design (no GUI, `--no-default-features`), so it is out of
  scope by definition. Plus: complete the half-finished Android migration
  to the `com.velocitydm.app` tree, and stand up this harness.
- **S2**: `scripts/publish-velocitydm.sh` — push main + tags (tags trigger
  the Release workflow), configure repo (topics/security/labels), create
  the six releases (v1.0.0, -linux, -windows, -macos, -android,
  -extensions) with verified floor assets; CI attaches fresh builds with
  `--clobber`. Requires the token to be writable (verified by probe).
- **S3**: After CI is green on main, protect `main` with exactly these
  required checks: `Rust format`, `Clippy`, `Test (ubuntu-latest)`,
  `Test (windows-latest)`, `Test (macos-latest)`,
  `Windows 7 payload (Rust 1.75)`, `Extension tests`,
  `End-to-end smoke`.
- **S4–S10**: tracked as GitHub issues on the `v1.1.0 - roadmap`
  milestone; each gets a fresh assessment record when activated.
- **S11**: Competitive study of the reference product this project
  re-creates, owner-directed. **Artifact pinned:**
  `idman643build18.exe` (IDM 6.43 build 18, official distribution URL
  `https://download.internetdownloadmanager.com/idman643build18.exe`,
  SHA-256 `7346128fab42957b4254eabdb012bfdbd751e9069361e98a6916f26bb03d62ee`,
  12,164,608 bytes, PE32 i386; fingerprinted 2026-11-14, binary never
  committed or stored in the repo/backups). **Re-pin policy (fail
  closed):** the vendor rotates builds; any hash/size mismatch or
  fetch failure fails closed — re-pinning requires a fresh fingerprint
  record in the provenance log, a download from the official HTTPS URL
  only (never a mirror), and a note in the stage record; the activation
  gate re-verifies hash + size against a fresh download before any
  work starts. **Prerequisites (stated honestly):** T1/T3 need a
  Windows-capable VM or separate machine (owner-provisioned — this
  sandbox has no KVM/qemu/wine and no Windows). T2's toolchain is
  now provisioned and verified in the sandbox (2026-11-14 probe:
  Ghidra 12.2 `analyzeHeadless` + Temurin JDK 25 at `tools/`,
  import → analysis → runtime script compile → headless C
  decompilation of a targeted routine in ~10 s / 407 MB peak RSS on a
  16 KB ELF; install/recovery in `RUNBOOK.md`; the probe exercised
  the ELF path — PE32 i386 is a documented built-in Ghidra loader —
  and the pinned IDM artifact remains untouched pending the T2
  clearance). T2 legal clearance: **executed and recorded 2026-11-21**
  (owner's dated in-chat directive, quoted verbatim in the research
  doc's provenance table; activation-gate hash/size re-verification
  passed byte-exact against a fresh official download before work
  began). T2 findings + adoption backlog: `docs/RESEARCH-IDM-STUDY.md`;
  clean-room disclosure: `docs/CLEAN_ROOM.md` S11 section. Still
  outstanding: T1/T3 (Windows VM, owner-provisioned) and any
  incorporation work (five-role gate + spec-first + A/B bench per
  brief). **Question:**
  does the reference product contain methods of download management or
  browser integration worth adopting here for performance, stability,
  and resource use? Method tiers:
  - **T1 — black-box observation (default tier):** drive a lawfully
    installed evaluation copy under observation and measure behavior:
    network traces (segment count/sizing, retry & refresh-link cadence,
    resume and auth handling, connection reuse), browser-integration
    channel sniffing (what the extension/hook → core IPC carries and
    when), resource monitoring (RSS, handles, startup cost).
    Lowest-risk tier; the license posture is still recorded in the
    stage record.
  - **T2 — static RE (decompile/disassemble, conditional):** unpack the
    installer, identify the packer, decompile targeted routines
    (scheduler, segment allocator, refresh-link logic, site-login,
    browser marshalling). Clearance is SEPARATE from the activation
    gate: activation authorizes T1 by default and records the legal
    posture (the product license/RE clause quoted, governing
    jurisdiction, the owner's written authorization, and an explicit
    statement that the panel is not legal counsel); T2/T3 additionally
    require a dated, owner-signed clearance in the stage record; any
    unresolved question defaults the stage to T1; a declined clearance
    escalates back to the project owner — never quietly dropped.
  - **T3 — dynamic RE (conditional):** instrumented runs of the product
    in an isolated throwaway VM with API tracing; same clearance
    requirement as T2, plus trace hygiene: throwaway accounts and
    synthetic credentials only; trace files stay inside the VM — only
    functional observations leave it.
  - **Clean-room hygiene (non-negotiable):** VelocityDM is GPL-3.0
    clean-room. Findings enter this repository only as functional
    behavior specs written in our own words; implementation happens
    from the committed spec, spec-first — each adopted technique's
    spec is committed BEFORE the implementing commit. T2/T3 artifacts
    (disassembly, decompiler output, raw traces) live only in a
    throwaway directory outside the repo, wiped after use — never in
    the repo, the backups, or the research notes. Provenance for every
    finding: tier (T1/T2/T3), tool + version, the studied build's
    SHA-256 prefix, date; T2/T3-derived backlog items are flagged for
    stricter spec review. The incorporation gate diffs the
    implementation against the spec and its provenance tier. No
    copying of code, data structures chosen for implementation
    reasons, resources, string tables, or UI. Anything questionable
    stays out.
  - **Deliverables:** `docs/RESEARCH-IDM-STUDY.md` (findings + an
    improvement backlog mapped to engine areas and S4+), the provenance
    log, a clean-room disclosure update (`docs/CLEAN_ROOM.md` gains an
    S11 section and an "IDM behavioral observation" provenance row;
    README clean-room wording aligned), and a **five-role gate before
    any incorporation**; every adopted technique must show a
    before/after pair via `scripts/bench-perf.sh`'s A/B mechanism (a
    measured win on performance, stability, or resource use) with no
    regression in the existing test batteries and no idle-RSS
    regression.
  - **Deep-dive (D7:B commissioned, D8:now — ACTIVE, multi-session):**
    the bounded function-level decompile of the reference engine's
    scheduler and segment allocator. Session 1 (2026-11-21) delivered
    the connection-scaling policy, queue-persistence model, limits
    quota, and completion path (`RESEARCH-IDM-STUDY.md` §4b, 184
    bounded-decompiled functions; tooling + pattern in the toolbox and
    RUNBOOK). Sessions 2+ (worker-thread range arithmetic,
    queue-state writer, site-login/refresh-link) re-verify the
    artifact fingerprint (fail-closed) at each session start and
    record a CLEAN_ROOM session entry each time.
  - Advisory stage: runs in parallel with S4+ and feeds them; it never
    blocks the release train. Tracked as GitHub issue #15 on the
    `v1.1.0 - roadmap` milestone like S4–S10.
- **S11-i**: S11 incorporation waves — the adoption half of the study's
  purpose ("methods ... that can be incorporated in to this project to
  improve performance, stability, and resource use"). Each wave runs the
  full S11 adoption bar: five-role activation gate, spec-first (behavior
  specs in `docs/SPECS/S11-ADOPTIONS.md` committed BEFORE the
  implementing commit), bench-perf A/B evidence, no battery regressions.
  Wave 1 (gate 0006): **B1** cross-connection content audit +
  single-connection recovery, **B2** optional connections (mid-download
  spawn/retire), **B9** file-lock classification + finalize retry —
  engine-side (`vdm-core/src/engine/task.rs` + additive snapshot
  `notice` field; win7 tree ported with MSRV-1.75 syntax; NO
  `ResumeState` change — the Android `.vdmctl` reader is strict-schema).
  Wave 2 (deferred sub-stages, each gets its own gate at activation):
  **B3** EXT-X-BYTERANGE (`hls.rs`, S7-adjacent), **B4** NMH port
  resilience (`extensions/shared/messaging.js` + `velocitydm-nmh`),
  **B5** cookie-jar forwarding, **B6** takeover UX (shelf suppression +
  cancel-then-resume handoff), **B7** video detection hardening
  (`detector.js`), **B8** MIME→extension table (ours from scratch).
  **B10–B16** parked with observe-first (DNR session rules, MAIN-world
  body re-association, session-scoped IPC naming, per-queue scheduler
  state, TS→MP4 remux, segment-info parsing, browser-proxy
  propagation). **N1–N6** rejected (kernel WFP/TDI layer, runtime
  LoadLibrary, custom installer container, profile-dir XPI writes, COM
  link intake, dial-up scheduling) — reasons recorded in the study.
  Adoption status per backlog item lands in
  `docs/RESEARCH-IDM-STUDY.md` §5 as waves close.
  - **Wave 2 (gate 0007, owner D1:C):** scope adds the two parked
    candidates the owner pulled in — **B13** persisted scheduler
    rules (`.vdmq` file, rules-survive-restart; task auto-recovery
    stays a stated non-goal) and **B16** browser-proxy propagation
    (report + suggest + warn; adds the `proxy` manifest permission
    with its review cost) — plus the B2 link-class presets deferred
    from wave 1. Behavior specs: `docs/SPECS/S11-ADOPTIONS-WAVE2.md`.
    Implementation sequencing (per gate 0007): engine-side first
    (B3 → B13 → B2p → B8-core → B16-core), then extension-side
    (B4 → B5 → B6 → B7 → B8-js → B16 wiring), one revertable commit per item,
    batteries + A/B per item, CI witness at each close. **Wave 2 closed
    2026-11-23: both sides landed (engine commits c6514d5..f236c9b;
    extension commits 282ab74..1ac2c47 — the node battery grew 76→240
    assertions; the honest overhead witness: a synthetic 5000-candidate
    gated DOM pass measured 4.63 ms in the dev sandbox; the B8 UMD
    conversion caught the ESM emit being unloadable in a classic service
    worker; B6's shelf + confirmation page still owe the owner's D5:A
    runtime verification).**
  - **Owner decision round 1 (2026-11-21, `harness/DECISIONS.md`)**
    shapes the queue: D2:B runs S4 (DHT) in parallel with wave 2;
    D4:B keeps T1/T3 honest — the owner provisions a Windows VM
    LATER, and the T1/T3 observation protocol is prepared meanwhile
    (measurement plans + trace hygiene per the S11 brief, ready to
    execute when the VM arrives; neither tier is silently dropped);
    D7:B commissions the IDMan.exe bounded multi-session deep-dive
    (scheduler + segment-allocator function-level decompile,
    multi-session Ghidra effort under the same T2 provenance and
    clean-room barriers; toolchain re-provisioned this session per
    the RUNBOOK recipe).
- **S12**: Owner-directed platform matrix — specific UI stacks per
  operating system around the **shared Rust core** (`vdm-core` +
  `vdm-rpc`), **Material You across all operating systems for
  consistency while keeping OS-specific elements**. The design contract
  is `docs/DESIGN-MATERIAL-YOU.md`; the gate record is
  `ASSESSMENTS/0004-s12-native-frontends.md`. Stack status:
  - **Foundation (DONE):** `crates/vdm-md3` — the shared Material You
    token engine (seed → OKLCH tonal palettes → MD3 light/dark
    schemes), consumed by every frontend; the egui `theme.rs` now
    delegates to it (single source of truth); `md3-tokens` CLI
    exports JSON for non-Rust consumers.
  - **Windows (DONE, preview):** Raw Win32 + Direct2D frontend at
    `frontend/win32/` — hand-rolled message loop, GPU-accelerated MD3
    vector chrome, DirectWrite text, native EDIT/BUTTON children, DPI
    awareness; standalone workspace (Windows-only deps); CI-built on
    windows-latest (non-required job). Type-checked locally against
    the msvc target.
  - **Linux modern (DONE, preview):** Slint frontend at
    `crates/velocitydm-slint/` — a root-workspace member, so the
    required fmt/clippy/test checks cover it on all three CI OSes
    (the win7 payload and extension-test jobs target other trees);
    `.slint` UI, MD3 chrome from primitives, native input widgets,
    in-process shared core.
  - **macOS (DONE, preview):** Swift + AppKit package at
    `macos-app/` — programmatic AppKit, Material You via the
    `Md3.swift` port of the shared token engine, JSON-RPC client of
    the shared core over the documented loopback protocol; CI-built
    on macos-latest (non-required job).
  - **Sub-stages (each needs its own five-role gate before work):**
    S12-e Android **Views + Kotlin Multiplatform** restructure —
    **DONE (preview)**, gate 0005, tag-rule-protected until the
    promotion gate (the frozen v1.0.0-family release ASSETS keep the
    Compose APK);
    **queue order per owner D3:D + D5:A (`harness/DECISIONS.md`):**
    promotion-gate preparation DONE (2026-11-23: parity matrices +
    packaging plan + owner-verification protocol at
    `docs/SPECS/S12-PROMOTION-GATES.md`); **SSE wiring for the Swift
    client DONE (2026-11-24, gate 0010: spec
    `docs/SPECS/S12-SSE-WIRING.md` + implementation a0f801f, CI 11/11
    green incl. the swift-frontend compile witness; the 200 ms poll is
    replaced by GET /events consumption and the stale-rows gap is
    closed in source)**; **S12-lin2 gtk3-rs DONE (2026-11-24, gate
    0012: spec `docs/SPECS/S12-LIN2-GTK3.md` + standalone `gtk3/`
    tree, CI 12/12 green at 6c16845 incl. the new `Linux frontend
    (GTK3)` preview job; closes the filename-override gap with a real
    Entry; renders error AND notice lines)**; **S12-mac2 SwiftUI DONE
    (2026-11-24, gate 0013: spec `docs/SPECS/S12-MAC2-SWIFTUI.md` +
    standalone `macos-swiftui/` tree — SwiftPM, macOS 12 floor, zero
    deps; `VDMText` verbatim-only rendering closes the
    LocalizedStringKey markdown/link-injection axis; the gate-0010 SSE
    consumer contract restated incl. launch-with-dead-daemon
    idempotency; CI 13/13 green at d11dd1a5 incl. the new `macOS
    frontend (SwiftUI)` preview job, one evidence-then-fix cycle on
    the golden table's memberwise-init `dark:` param; closes the
    filename + torrentPath gaps in THAT tree — macos-app still carries
    both, its promotion owns them; golden-literal tests 288 values vs
    the Rust baseline via `scripts/gen-md3-golden-swift.py`)**; next:
    S12-lin3 **Qt (cxx-qt)** —
    the owner's runtime verification moment is NOW UNBLOCKED (D5:A:
    verify a full surface — the Swift checklist rows incl. the SSE/
    stale-rows behavior) and precedes the per-surface promotions.
    Tracked as GitHub issue #16 on the `v1.1.0 - roadmap` milestone.
  - **S12-e execution plan (gate 0005 consensus — 5× PROCEED-WITH-
    CONDITIONS, zero BLOCK; conditions folded below):** replace the
    Compose UI with an **Android View-based Material 3** UI (MDC 1.12.0
    — the View-system MD3, `android.view` widgets throughout) and
    restructure the Kotlin code into a **Kotlin Multiplatform
    `:shared` module**. The Compose `Theme.kt` hand-copied baseline
    literals go away.
    (a) `android/shared/` — `kotlin("multiplatform")` 2.0.21 +
    `com.android.library` 8.7.0 (both added apply-false to the root
    build); targets `androidTarget()` + `jvm()`; a custom `jvmCommon`
    intermediate source set (androidMain and jvmMain both depend on it)
    carries the JVM-family code — the moved `SegmentEngine`
    orchestration (OkHttp 4.12.0, `RandomAccessFile`, atomics,
    coroutines pinned 1.8.1 to match :app) — so every `expect` has an
    actual for BOTH targets and `:shared:jvmTest` compiles. `commonMain`
    holds the pure logic: the Kotlin `Md3` token-engine port (full Rust
    role set, NOT the truncated Md3.swift API), with goldens asserted
    against a **checked-in `md3-tokens-baseline.json` generated from
    the Rust `md3-tokens` CLI** (the Rust↔Kotlin drift tripwire — not
    hand-copied literals); `RangePool` interval-leasing math; the
    `.vdmctl` schema (kotlinx-serialization 1.7.3) **with validation**
    (`0 ≤ start ≤ end < size`, sum ≤ size, unknown keys ⇒ restart from
    zero; refuse resume when the server sends no ETag unless stored
    size == probe size); token-bucket wait math; speed/ETA/size
    formatting + `parseLimit`; task models on String paths (no
    `java.io.File` in commonMain); `sanitizeFilename()` and real URL
    validation (http/https only — replaces `startsWith("http")`;
    rejects path separators/`..`/absolute paths/control chars, caps
    length, falls back "download") applied on every `add()` path.
    (b) `:app` Views restructure — Compose plugin/BOM/deps **removed**
    from root and app; MDC `material` 1.12.0 + `recyclerview` 1.3.2 +
    serialization/coroutines pinned through an **activated
    `gradle/libs.versions.toml` catalog** (dead Compose entries pruned,
    coroutines reconciled to 1.8.1); manifest theme stays
    `Theme.VelocityDM`, re-parented `Theme.Material3.DayNight` in ONE
    `values/themes.xml` (the `values-night` copy deleted; MDC widgets
    crash under the framework parents); hardcoded windowBackground
    colors dropped (MDC colorSurface owns the background); runtime
    theming: wallpaper **dynamic color on Android 12+**
    (OS-specific element), pre-12 static scheme from the Kotlin Md3
    port seeded **0x4156A4 — the actual `vdm_md3::DEFAULT_SEED`**
    (gate 0005 corrected the draft's 0x6750A4 "desktop seed" mislabel),
    with a documented **Scheme→MDC-attr mapping table** naming every
    attr intentionally left to MDC defaults and an explicit
    "pre-12 colors visibly change vs the old Google baseline" note;
    programmatic View tree with **full Compose parity**: MDC toolbar +
    Browser action, FAB → add-URL dialog (URL + filename override +
    speed limit, validated), RecyclerView cards (filename, state label
    with semantic colors, `LinearProgressIndicator`, done/total/speed,
    per-task error text, pause/resume/remove) + empty state;
    VIEW-intent auto-add preserved; **`browser/BrowserActivity.kt`
    rewritten as a real View-based Activity** (the manifest's dangling
    entry finally points at an existing class): MDC toolbar + address
    field + sniffed-media list + WebView; ViewModel/DownloadService/
    BootReceiver imports re-pointed to `:shared` (behavior preserved,
    imports and construction change); **versionCode 1 / versionName
    1.0.0 / applicationId frozen** (the promotion gate owns bumps);
    `proguard-rules.pro` extended (`-keep` for `com.velocitydm.shared.**`
    + kotlinx.serialization rules) with CI exercising
    `assembleRelease` so the R8 path compiles on every change; the
    unreferenced `res/xml/network_security_config.xml` deleted (the
    manifest `usesCleartextTraffic` attribute carries unchanged — one
    source of truth); the orphan `gradle-wrapper.properties` (no
    jar/scripts exist) deleted with docs updated to the setup-gradle
    reality; empty `dm/` tree cruft removed.
    (c) CI: one **non-required** `android-frontend` job —
    **ubuntu-22.04** for byte-parity with the frozen release job;
    `actions/checkout@v4` + `actions/setup-java@v4` temurin 17 +
    `gradle/actions/setup-gradle@v4` pinned `8.9` (byte-identical
    mechanism to release.yml; no new third-party actions, no secrets):
    `gradle -p android :shared:jvmTest :app:assembleDebug
    :app:assembleRelease --no-daemon`; the 10 existing jobs untouched;
    branch protection re-verified at close-out (8 contexts unchanged,
    `android-frontend` absent). Compile witness: the first CI run on
    main (KMP-on-Gradle-8.9 is unproven in-repo; runtime verification
    stays in the promotion criteria, per the 0004 pattern) — or a
    locally provisioned SDK build before the push, if provisioning
    succeeds.
    (d) Release integrity: a GitHub **ruleset (tag target, pattern
    `v*`, restrict creation/update)** is added BEFORE the
    implementation merge — the six frozen v1.0.0-family releases stay
    frozen (re-running release on a v1.0.x tag would attach the Views
    build: the freeze is procedural, now guarded by the ruleset); the
    S12-e commit is **push-main-only** (no `publish-velocitydm.sh`, no
    tag moves, no release.yml edits — release.yml `workflow_dispatch`
    is forbidden until the promotion gate); one revertable
    implementation commit (android/** + the additive ci.yml job only).
    (e) Docs: `android/README.md`, `android/docs/ARCHITECTURE.md`
    (Views tree, real BrowserActivity, Scheme→MDC mapping),
    `docs/DESIGN-MATERIAL-YOU.md` Android row, stale `README.md` /
    `docs/BUILDING.md` wrapper + ABI-splits fixes;
    `harness/RUNBOOK.md` gains an "Android preview (S12-e)" section
    (local/CI build, `:shared`, failure policy: required-green +
    android-frontend-red ⇒ block all tag cuts until green + promotion
    gate; rollback = git revert of the S12-e commit + device reinstall
    of the v1.0.0 APK). Honest scope: the KMP module ships android+jvm
    targets only; the Android engine remains the Kotlin port — S8's
    brief now explicitly owns "Android engine unification onto the
    shared Rust core", and the stage row notes the Android leg runs on
    the Kotlin port today.
  - Windows 7 stays the headless payload by design (out of scope,
    as in S1).
