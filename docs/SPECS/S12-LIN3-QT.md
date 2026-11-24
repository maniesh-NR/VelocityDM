# S12-lin3 spec — Qt frontend (cxx-qt / Qt Quick, standalone `qt/` tree)

Behavior spec for the S12 sub-stage queued by gate 0012/0013's gate
order ("next S12-lin3 Qt"). Gate:
`harness/ASSESSMENTS/0017-s12-lin3-qt.md` (round 1: 5×
PROCEED-WITH-CONDITIONS, zero BLOCK; conditions folded into this
text). Target: a new standalone `qt/` tree (the win32/gtk3 shape:
explicit package fields, a trailing `[workspace]` to detach from the
root workspace, a committed `Cargo.lock`, CI builds `--locked`) + one
additive non-required CI job. **CI is the sole compile/test witness**
— the dev sandbox has no Qt and no display, and no record may claim
local verification.

**Hard constraints:** ships in no release asset until its promotion
gate; `release.yml`, `packaging/**`, the `v*` ruleset, the root
`Cargo.toml`/`Cargo.lock` all byte-untouched; push-main-only; no
`workflow_dispatch`; no `crates/**` changes.

## Stack decisions (fixed by this spec)

- **cxx-qt** with **Qt Quick (QML)** for the chrome — cxx-qt exposes
  Rust state as QObjects with properties/invokables; QML renders the
  Material You surfaces (the Slint tree's "MD3 chrome from
  primitives" approach, now in QML). Qt **Widgets** is NOT used
  (cxx-qt's binding surface is QObject/QML-centric). The default is
  the pure-Rust cargo-only build path (no CMake, no C++ bootstrap —
  `cxx-qt-build`'s qmake feature probes `QMAKE`/`qmake6`; the
  `examples/cargo_without_cmake` shape). If — and only if — the
  resolved cxx-qt line forces a C++ bootstrap, the equivalent cmake
  invocation joins the CI job IN THE SAME COMMIT (the CI-witness
  claim must never silently break).
- **Versions pinned to the runner reality:** CI runs on
  `ubuntu-24.04` (pinned, not `ubuntu-latest` — freezes the distro
  Qt against the committed lock; the runner ships Qt 6.4.2 today).
  cxx-qt is resolved DOWN to the runner's Qt — the newest line whose
  declared minimum fits (today cxx-qt 0.10.0 supports all Qt 6; its
  MSRV 1.85 ≤ CI stable 1.100). NEVER via PPAs or aqtinstall (that
  would change the witness surface). Exact versions pinned in
  `qt/Cargo.toml` (= pinned) + committed `qt/Cargo.lock`
  (resolution is Qt-free — `cargo generate-lockfile` does not run
  build scripts — verified feasible in the Qt-less sandbox); the
  resolved cxx-qt/cxx versions, the observed apt Qt version, and the
  chosen build path are recorded in `qt/README.md`.
- Path dependencies on `../crates/{vdm-core,vdm-rpc,vdm-md3}` with
  default features (tls on — gtk3 parity) + explicit serde/serde_json;
  the crate's `rust-version` recorded. `forbid(unsafe_code)`: cxx-qt
  macro expansion generates unsafe internally, so a crate-wide forbid
  is impossible — the policy is "no hand-written unsafe",
  `#![deny(unsafe_code)]` is applied to the pure-logic modules
  (config/theme/format — no cxx-qt macros there), and the tree README
  carries the honest note (never a false forbid claim).

## Behavior parity (the gtk3 contract — the FULL long tail)

- In-process `Engine` + `RpcServer` bound **exactly** 127.0.0.1:36990
  (no port fallback — daemon/NMH discovery is fixed-port per
  docs/RPC.md; never 0.0.0.0); honest loud failure on bind error
  ("is another VelocityDM frontend running?"). The trust boundary is
  inherited from `vdm-rpc` (loopback bind, Host check, Origin/Referer
  absence, JSON content requirement) — never reimplemented. Shared
  `config.json`/`ui.json` (same fields, tolerant loads); `--portable`
  honored.
- MD3 tokens from `vdm-md3`: seed → scheme runs in Rust; QML receives
  role colors via a cxx-qt tokens bridge — **typed data only**
  (`#[qproperty]` fields of `QColor`/u32, or hex strings built
  exclusively via `format!("#{:06x}", v & 0xFFFFFF)`); zero runtime
  strings in any color/style channel. The role subset shipped is the
  set the QML actually consumes, named honestly in the README (the
  gtk3 19/27 honesty pattern).
- Task list: ListView with per-row filename, state label with
  semantic colors, linear progress, done/total/speed, **BT p/s
  stats**, error AND notice lines visibility-toggled, per-task
  pause/resume/remove with can-pause/can-resume state mapping;
  **in-place keyed updates** — the model mechanism is a
  cxx-qt `QAbstractListModel` subclass (id-keyed roles, per-index
  dataChanged; full reset ONLY on membership change); a recorded
  fallback (per-row QObjects) if subclassing proves impractical —
  never a 200 ms full-list QVariantList reset. Header total-speed
  banner + honest empty state.
- Add path: URL field + **quality default "best"** + **speed-limit
  entry** (`parse_bytes` → limit) + **filename override** (a real
  bound field → `HttpOptions.filename`) + **magnet: `add`** + **local
  `.torrent` typed path** (top-level params, per
  `crates/vdm-rpc torrent_spec_from_params`); Enter-to-add;
  **inline add-failure error label** (cleared on next success).
- Settings panel: connection clamps (1..32, max-active 1..16 applied
  on Apply), **download-dir entry**, **global-limit entry**, proxy
  parse (http/socks5; empty/`"none"` = clear; tolerant bad-parse),
  stop window (both ends must parse), shutdown-when-done toggle;
  **Apply starts from `engine.config()` so unexposed fields
  (`user_agent`, scheduler) survive** then `set_config` + save +
  re-push (the clobber-bug guard from the sibling trees).
- Seed cycling over `vdm_md3::SEEDS` + seed-name label + ui.json
  persistence; dark-mode toggle.
- **Lifecycle:** exit path = stop poller → `server.stop()` →
  `engine.shutdown()` (no orphan process).
- Polling: a 200 ms poll — **QML `Timer { interval: 200 }`** element
  invoking a Rust invokable (cxx-qt-lib ships no QTimer bindings) on
  the Qt main thread. SSE consumption is a recorded deferred upgrade
  (named in RUNBOOK + README + the promotion-gates progress cell
  wording).
- Rendering discipline for attacker-controlled strings: **every
  text-bearing type** (`Text`, `Label`, `TextEdit`, `TextArea`,
  `TextField`, ListView delegates) carries the literal
  `textFormat: Text.PlainText` — never rely on QML's default
  `AutoText` (which sniffs HTML into rich text, including remote
  `<img>` fetches). `Text.StyledText`/`Text.RichText`/
  `Text.MarkdownText` are FORBIDDEN tree-wide. No
  `onLinkActivated`/`Qt.openUrlExternally` on daemon-derived
  strings. **All QML/JS is static and embedded** — `Qt.createQmlObject`,
  `Qt.createComponent` from runtime strings, `Loader { source:
  <runtime> }`, and `eval` are forbidden (QML injection class). All
  attacker strings additionally pass a Rust `display_text()` helper
  (control chars → space, trim, headless-tested) and row labels carry
  `maximumLineCount: 1` + elide.
- Headless-safe tests: the pure-logic modules are unit-tested WITHOUT
  a QApplication or QQmlEngine; tests NEVER start `RpcServer` on the
  fixed product port (bind `127.0.0.1:0` if an RPC-path test is
  needed). The QML layer is witnessed by the CI build + a **grep
  tripwire** (a CI step or data-only test scanning `qt/**` QML
  sources for `Text.StyledText`/`Text.RichText` and missing
  `textFormat: Text.PlainText` on daemon-string sinks).

## Module layout (pinned)

`qt/src/main.rs` (bootstrap + lifecycle), `qt/src/config.rs` (mirror
of `gtk3/src/config.rs`), `qt/src/tokens.rs` (vdm-md3 → typed bridge
values), `qt/src/model.rs` (the task-list model + pure logic split
for headless tests), the QML module directory (`qt/qml/`).

## CI (additive, non-required)

- New job id `qt-frontend`, display name `Linux frontend (Qt)`,
  **`runs-on: ubuntu-24.04`**, step order per repo convention:
  checkout → Rust toolchain (mirroring the gtk3 job's install
  steps) → `apt-get install qt6-base-dev qt6-declarative-dev
  qt6-base-dev-tools qmake6 libgl1-mesa-dev` (the tools package is
  named explicitly so a future Ubuntu package split cannot silently
  break moc/rcc provisioning; `qmake6` + `/usr/lib/qt6/bin` on PATH
  as insurance against Ubuntu's qtchooser quirk) → `cargo fmt
  --manifest-path qt/Cargo.toml --all -- --check` → `cargo clippy
  --manifest-path qt/Cargo.toml --all-targets --locked -- -D
  warnings` (if cxx-qt macro output trips unrelated lints, clippy
  runs without `-D warnings` and build+tests remain the gate — the
  choice recorded in the README) → `cargo test --manifest-path
  qt/Cargo.toml --locked` → `cargo build --manifest-path
  qt/Cargo.toml --locked` (+ the QML tripwire scan). The
  workflow-level concurrency group already covers the new job; no
  `needs:` changes.
- Failure policy (same class as gtk3/swiftui): a red `qt-frontend`
  job is evidence-then-fix forward; it does NOT block tag cuts or
  the release train (ships nothing). Job count becomes 14
  (8 required + 6 preview). Branch protection re-verified at close
  (8 required contexts unchanged, `qt-frontend` absent).

## Rules / commit split

- **Local battery (honest, complete):** `cargo generate-lockfile`
  inside `qt/` + `cargo fmt --manifest-path qt/Cargo.toml --all --
  --check` ONLY. No record may claim a local build/clippy/test of
  the tree. `git diff --exit-code qt/Cargo.lock` after any
  lockfile-touching step (drift fails CI, not the fix-forward).
- **C1** = activation (spec + gate record 0017); **C2** = the single
  revertable implementation commit — `qt/**` + the additive
  `ci.yml` job + the `.gitignore` `/qt/target` line +
  `docs/BUILDING.md` Qt section — revert byte-restores main; **C3**
  = close docs; **C4** = backup LAST (`scripts/backup-sandbox.sh`),
  zero dirty files. Rollback = `git revert` of C2.

## Docs at close (complete chain)

`qt/README.md` (tree rules: canonical-copy rule vs the other trees,
drift rule, honest no-local-verification sentence, the PlainText +
no-dynamic-QML rendering rules, role-subset honesty, resolved
cxx-qt/cxx/Qt versions + build path); `docs/BUILDING.md` Qt section;
`docs/DESIGN-MATERIAL-YOU.md` stack-matrix row + consumers note;
**parity-matrix Qt columns (§1.1–§1.5 of
`docs/SPECS/S12-PROMOTION-GATES.md`) grounded in `qt/src` at close —
never pre-filled**; gap register update — filename override CLOSED
**in the Qt tree** (cited to the qt/ source file); Win32/Slint/AppKit
still carry it, their promotions own it (the register wording
follows the mac2 pattern); §2 packaging-plan + §3 owner-verification
Qt rows recorded as debt owed before the promotion gate is queued;
`harness/ROADMAP.md` S12 row + brief flip; RUNBOOK "Qt preview
(S12-lin3)" section (CI-only witness, failure policy, lockfile
discipline); `harness/MEMORY.md` entry; `harness/ASSESSMENTS/0017`
record (noting the 0014–0016 numbers reserved by the parallel S5/S6/
S7 gates); BACKUPS.md row; issue #16 comment; backup LAST.
