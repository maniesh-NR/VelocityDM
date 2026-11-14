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
| S4 | DHT (BEP-5) so trackerless magnets resolve | PENDING | — |
| S5 | UDP trackers (BEP-15) | PENDING | — |
| S6 | DASH downloader (.mpd manifests) | PENDING | — |
| S7 | HLS: SAMPLE-AES + fMP4 EXT-X-MAP | PENDING | — |
| S8 | Android ↔ desktop daemon over LAN (shared RPC) | PENDING | — |
| S9 | Safari native app extension (Xcode conversion) | PENDING | — |
| S10 | Flathub submission | PENDING | — |
| S11 | IDM competitive study: RE-assisted review of the reference product's download-management & browser-integration methods (performance / stability / resource use) | PENDING | — |

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
  sandbox has no KVM/qemu/wine and no Windows); T2 needs a PE RE
  toolchain (e.g. Ghidra, Apache-2.0 — locally only binutils `objdump`
  exists); until provisioned, the deliverable is the study plan plus
  literature-level analysis, never invented measurements. **Question:**
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
  - Advisory stage: runs in parallel with S4+ and feeds them; it never
    blocks the release train. Tracked as GitHub issue #15 on the
    `v1.1.0 - roadmap` milestone like S4–S10.
