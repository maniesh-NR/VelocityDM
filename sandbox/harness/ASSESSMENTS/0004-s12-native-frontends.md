# Assessment 0004 — S12 native frontends (gate record)

**Date:** 2026-11-14/15 (UTC). **Change under review:** the S12 session —
shared Material You token engine (`crates/vdm-md3`), three native
frontends (Win32+Direct2D, Slint, Swift+AppKit), CI wiring, design
contract, sub-stage records. **Panel:** RED (Adversary) + REQ
(Requirements Guardian) dispatched independently with repository
access (per PROTOCOL.md; a roadmap increment of this size uses the
two-role amendment-gate form, as in 0003 — the five-role panel is
reserved for stage ACTIVATION of the recorded sub-stages).

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| RED | **PROCEED-WITH-CONDITIONS** | win32 `WM_SIZE` read the size from `wparam` (flags) instead of `lparam` — surface collapses to 1×1 after first resize; win32 DPI mismatch (native children scaled, drawn chrome not); Swift `settings.get` decode fails on the structured proxy shape (whole EngineSettings decode thrown) and `settings.set` would drop a plain-string proxy; Swift global-limit dead control; `parseBytes` traps on negative/NaN; per-paint brush/text-format churn; ROADMAP "ALL eight checks" overstatement; dangling 0004 record; config module triplication; md3.js stale header; Swift stale rows on daemon death. Retired with evidence: lockfile (win7 builds via `--manifest-path win7/Cargo.toml --locked` in both ci.yml and release.yml — root v4 lock never parsed by cargo 1.75), slint MSRV (1.92) vs the macos 1.99.1 pin, feature unification, releases untouched, theme.rs delegation real, Swift/Rust MD3 parity constants-identical, Swift loopback trust-boundary compliance |
| REQ | **PROCEED-WITH-CONDITIONS** | `.gitignore` missing `/frontend/win32/target/` (182 MB of artifacts one `git add` away from being committed); slint `add-filename` dead property; filename UI missing in all three preview frontends (parity gap vs promotion criterion); vdm-md3 golden test tautological; Swift cannot add local `.torrent` paths; MEMORY entry pending; all deferred stacks (Android Views+KMP, gtk3-rs, Qt, SwiftUI) recorded honestly as gated sub-stages + issue #16; branch protection untouched (8 byte-exact required checks; new job names non-colliding); every design-contract path exists; R1–R12 coverage verified |

## Consensus

Zero `BLOCK` → the gate closes with this record; every condition was
fixed or recorded below.

## Conditions and resolution (all fixed before the S12 commit)

1. win32 `WM_SIZE` uses `lparam` LOWORD/HIWORD for the client size;
   `WM_DPICHANGED` handler added; invalidate on both. **Fixed.**
2. win32 DPI: drawn chrome now scales by `dpi/96` (all layout
   constants, fonts, radii, zones — hit zones stay physical-px
   consistent with mouse coords). **Fixed.**
3. Swift proxy modeled structurally both ways (`RpcProxy` Codable for
   the tagged enum get; `RpcProxy.parse` + object emission for set);
   `getLimits`/`setLimits` round-trip for the global-limit control;
   `parseBytes` guards non-finite/negative input. **Fixed.**
4. `.gitignore` gains `/frontend/win32/target/`. **Fixed.**
5. ROADMAP "ALL eight required CI checks" → precise wording (required
   fmt/clippy/test on all three OSes; win7/ext jobs target other
   trees). **Fixed.**
6. vdm-md3 golden test now pins literal hexes (dark/light, generated
   by `md3-tokens`) + chroma-boost proof; tautology removed. **Fixed.**
7. Slint dead `add-filename` property removed. **Fixed.**
8. md3.js header updated (names `vdm-md3` as the canonical engine;
   documents the CIELAB-vs-OKLab difference). **Fixed.**
9. This record + MEMORY.md session entry. **Fixed.**

## Recorded (not fixed this session — promotion/sub-stage work)

- Win32: Enter-to-add and TAB traversal absent (recorded; the native
  Add button exists); per-paint brush/text-format churn (cache on
  retheme); per-RED, preview-scoped.
- Swift: local `.torrent` path support (recorded as promotion-gap);
  stale rows on daemon death; SSE wiring (currently 200ms polling).
- Filename UI missing in all three preview frontends (promotion
  criterion gap; recorded in issue #16 promotion checklist).
- Config/prefs plumbing triplicated (daemon.rs / slint config.rs /
  win32 config.rs — behavior-identical today; extract a shared crate
  at the next touch).
- Root `Cargo.lock` is now **v4** (cargo 1.100 regenerates it on any
  dependency change; preserving v3 was the old hygiene rule). Verified
  harmless: every cargo-1.75 build uses the standalone
  `win7/Cargo.toml --locked` workspace (ci.yml + release.yml); no 1.75
  command parses the root lock. `win7/Cargo.lock` remains v3, and the
  vdm-rpc dependency-shape change (`default-features = false` +
  feature passthrough) preserves root-workspace resolution (tls
  default on) — witnessed by the local green battery and the CI run
  on main.
- Residual unprovable in-sandbox: real MSVC link, macOS Swift compile,
  Windows/macOS runtime behavior — the first CI run on main is the
  compile witness; runtime verification stays in the promotion
  criteria (owner on real hardware).

## Sub-stages opened (each needs its own five-role gate)

S12-e Android Views+KMP; S12-lin2 gtk3-rs; S12-lin3 Qt; S12-mac2
SwiftUI; Swift SSE wiring; frontend promotion gates. Tracked as
issue #16 on the `v1.1.0 - roadmap` milestone.
