# Material You across operating systems — design contract

Status: S12 foundation + three native frontends implemented (see
`harness/ROADMAP.md` S12 for stage status). The desktop egui GUI remains
the shipped v1.0.x default; the native stacks below are preview
frontends (CI-built) until each passes its promotion gate.

## The stack matrix (owner-directed)

| OS | Stack | State |
| --- | --- | --- |
| Windows 10+ | Raw Win32 + Direct2D (GPU-accelerated 2D vector drawing) | `frontend/win32/` — CI-built (windows-latest); preview |
| Windows 7 | headless payload (no GUI, by design) | unchanged (`win7/`) |
| macOS (older) | Swift + AppKit | `macos-app/` — SwiftPM package; CI-built (macos-latest); client of the shared core over the loopback JSON-RPC protocol |
| macOS (newer) | SwiftUI variant | sub-stage (S12-mac2); AppKit covers the older systems first |
| Linux (modern) | Rust + Slint | `crates/velocitydm-slint/` — workspace member, covered by the required fmt/clippy/test checks on all three CI OSes; in-process shared core |
| Linux (older) | Rust + gtk-rs / gtk3-rs | sub-stage (S12-lin2): needs GTK3 dev libs on CI (absent in the dev sandbox); Slint with the software renderer covers older hardware in the meantime |
| Linux (Qt alternative) | Qt (cxx-qt) | sub-stage (S12-lin3): requires a C++/Qt toolchain; tracked, not built |
| Android | Android Views + Kotlin Multiplatform | sub-stage (S12-e): the shipped app (Compose, dynamic color) stays v1.0.x default; the Views+KMP restructure is its own gated stage |
| Browser extensions | MV3 + JS (`extensions/shared/md3.js`) | shipped since S1 |
| All | shared Rust core (`vdm-core`) + shared MD3 token engine | the engine is in-process for egui/Slint/Win32; remote (JSON-RPC+SSE, `docs/RPC.md`) for the macOS Swift client |

## The shared pieces

### 1. The token engine: `crates/vdm-md3`

One crate every frontend consumes. Clean-room MD3 dynamic color:
seed color -> OKLCH -> five tonal palettes (chroma gamut-clipped at
constant tone) -> full light/dark MD3 role set. The OKLab math is
Björn Ottosson's public-domain formulation; tone tables follow the
published MD3 role assignments. Serialization (`json` feature) and a
`md3-tokens` CLI export the scheme for non-Rust frontends.

Consumers today:

| Frontend | How it gets the tokens |
| --- | --- |
| egui GUI (shipped) | in-process (`theme.rs` delegates to the crate) |
| Slint frontend | in-process (fills the `Theme` global each paint) |
| Win32/Direct2D frontend | in-process (brushes built from scheme roles) |
| macOS Swift | **Swift port of the same math** (`Md3.swift` — identical constants; deterministic parity, no build step) |
| Browser extensions | the JS engine (`md3.js`), same model (tone tables aligned; CIELAB vs OKLab color space documented there) |
| Android (today, Compose) | OS dynamic color on 12+, seeded fallback; the KMP restructure will carry a Kotlin port of the crate |

`vdm-md3::SEEDS` is the canonical seed list across all pickers.

### 2. The shared Rust core

`vdm-core` (engine) + `vdm-rpc` (JSON-RPC 2.0 + SSE over loopback,
`docs/RPC.md`). Frontends either embed the core in-process (egui, Slint,
Win32 — engine + RPC server inside the frontend binary, one process
like the daemon) or drive it remotely (macOS Swift client against the
daemon binary; future KMP module). Both paths speak the same surface,
share the same `config.json`, and honor the loopback trust boundary.

## Material You, consistently — OS-specific by design

**Consistent** (from the token engine, on every OS): the MD3 role set
(primary / containers / surfaces / on-colors / outlines), tone
assignments, shape language (rounded cards 12px, pill buttons, 8px
chips), the seed picker + light/dark toggle, state chips
(done/active/queued/failed...), and progress bars in `primary`.

**OS-specific** (deliberately kept): input controls and interaction
follow each platform's standard — Win32 EDIT/BUTTON children and a
hand-rolled message loop; AppKit NSTableView/NSTextField/NSButton with
the standard menu bar and window chrome; Slint's native-feeling input
widgets under winit; Android system bars and OS dynamic color; window
chrome, file dialogs, keyboard conventions, and accessibility belong
to each platform's stack. Material You colors the content; the
platform owns the interaction.

## Frontend-specific notes

- **Win32/Direct2D** (`frontend/win32/`): standalone workspace
  (Windows-only `windows` crate must not enter the root workspace —
  Linux/macOS CI resolves it). Hand-rolled `wndproc`, `SetTimer`
  200ms refresh, Direct2D vector chrome (FillRoundedRectangle geometry
  + solid brushes — no bitmaps), DirectWrite text, per-monitor-v2 DPI
  awareness, hit-tested drawn buttons for pause/resume/remove, native
  EDITs for URL/quality/limit and the settings panel. Same
  `config.json`/`ui.json` as the other frontends. `tls` is a feature
  (default on; the dev sandbox type-checks `--no-default-features`
  because `ring` cannot cross-compile to msvc from Linux).
- **Slint** (`crates/velocitydm-slint/`): workspace member, so the
  eight required CI checks cover it. `.slint` UI with the MD3 chrome
  drawn from primitives; native LineEdit/CheckBox/Slider; Timer-driven
  model refresh; in-process engine + RPC server.
- **Swift/AppKit** (`macos-app/`): SwiftPM, macOS 10.15+, programmatic
  layer-backed AppKit; polls `downloads.list` at 200ms (SSE wiring is a
  follow-up); `Md3.swift` is the token-engine port; prefs in
  UserDefaults (`vdm.seed`, `vdm.dark`). Build: `swift build`; run
  against the daemon binary.

## Promotion criteria (a frontend becomes the shipped default)

1. Runtime verification on real hardware by the owner (the sandbox has
   no Windows/macOS displays; CI link success is not run evidence).
2. The full local/CI battery green.
3. Feature parity with the egui GUI (add/quality/limit/filename,
   pause/resume/remove, settings incl. proxy + stop window +
   shutdown-when-done, seed/dark picker, e2e flows via the shared
   core).
4. A five-role adversarial gate (`PROTOCOL.md`) recorded in
   `ASSESSMENTS/`.
5. Release packaging (`release.yml`) updated in the same stage — no
   premature asset changes; the six v1.0.0 releases stay frozen until
   then.
