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
