# VelocityDM — Qt Quick frontend (S12-lin3 preview)

A Qt 6 Quick (QML) frontend for VelocityDM on the **shared Rust core**
(`vdm-core` in-process + `vdm-rpc` bound to the fixed product address
`127.0.0.1:36990` — loopback trust boundary inherited from `vdm-rpc`,
never reimplemented; honest loud failure on bind error). Material You
tokens come from the shared `vdm-md3` engine; the chrome is built from
**QtQuick primitives only** (no `QtQuick.Controls` import) — the same
"MD3 chrome from primitives" approach as the Slint tree.

This tree is a **preview**: it ships in no release asset until its
promotion gate (`docs/SPECS/S12-PROMOTION-GATES.md`); `release.yml`, the
`v*` tag ruleset and the root workspace are untouched by it.

## Resolved stack (recorded honestly)

| Component | Version | Notes |
| --- | --- | --- |
| cxx-qt | **0.10.0** (pinned `=0.10.0`) | MSRV 1.85; supports all Qt 6.x |
| cxx-qt-lib | 0.10.0 | `qt_gui` + `qt_qml` features |
| cxx | 1.0.205 | as resolved by the committed lock |
| cxx-qt-build | 0.10.0 | `link_qt_object_files` (the cargo-without-cmake linking path — **no CMake**, no C++ bootstrap file; `qmake` probed from `QMAKE` or the PATH) |
| Qt | the pinned CI runner's apt | `ubuntu-24.04` = **6.4.2** today; never PPAs/aqtinstall |
| rust-version | 1.85 | the crate's recorded MSRV |

The lockfile is committed and CI builds `--locked`; any
lockfile-touching step must leave `git diff --exit-code qt/Cargo.lock`
clean.

## Layout

- `src/main.rs` — bootstrap (core + fixed-port RPC bind → QGuiApplication
  → QML load) and the exit path: the QML poll timer dies with the QML
  engine, then `server.stop()`, then `engine.shutdown()` (no orphan
  process). `--portable` honored.
- `src/app_model.rs` — the ONLY Qt-touching Rust file: the `AppModel`
  **QAbstractListModel** (via the cxx-qt `#[base]` + `#[cxx_override]`
  mechanism from the upstream `todo_app`/`custom_base_class` examples),
  id-keyed roles, **per-index `dataChanged`** on the 200 ms refresh and a
  full model reset ONLY when task membership changes (never a per-poll
  full-list reset), plus every QML-facing invokable/property.
- `src/model.rs` — pure rendering logic (display sanitizer, state chips,
  stats line, settings Apply with clamps and merge semantics) —
  headless-tested, no Qt types.
- `src/config.rs` — the shared `config.json`/`ui.json` mirror (gtk3
  parity: tolerant loads, portable paths).
- `src/tokens.rs` — `vdm-md3` → hex-string color bridge (typed data
  only: `format!("#{:06x}", v & 0xFFFFFF)`).
- `qml/main.qml` — the surface (see the rendering rules below).

## Rendering rules (load-bearing)

1. **Every text-bearing element carries the literal
   `textFormat: Text.PlainText`** — QML's default `AutoText` sniffs HTML
   into rich text (including remote `<img>` fetches); a CI tripwire
   scans the QML for violations. `Text.StyledText`/`Text.RichText`/
   `Text.MarkdownText` are forbidden tree-wide. No link handlers on
   daemon-derived strings.
2. **All QML/JS is static and embedded** — no `Qt.createQmlObject`, no
   `Qt.createComponent` from runtime strings, no `Loader` with a runtime
   source, no `eval`.
3. Daemon-derived strings additionally pass the Rust `display_text()`
   sanitizer (control chars → spaces, trim) before reaching a role.
4. Property mutations happen exclusively on the Qt main thread — the
   200 ms QML `Timer` invokes `refresh()` there (cxx-qt-lib ships no
   QTimer bindings; the QML element is the documented mechanism).
5. Colors are hex strings built in Rust from the shared scheme — the
   role subset shipped is the **16 roles the QML consumes** (the gtk3
   19/27 honesty pattern; `tokens::ROLE_SUBSET` asserts the count).
6. `#![deny(unsafe_code)]` on the pure-logic modules (`config.rs`,
   `tokens.rs`, `model.rs`); `app_model.rs` cannot forbid `unsafe`
   (the macro expansion + the inherited begin/endResetRows calls are
   inherently unsafe) — "no hand-written unsafe" beyond those inherited
   model-lifecycle calls is the honest policy.

## Honest boundaries

- **The dev sandbox has no Qt** — no local build/test/clippy of this
  tree is possible or claimed. Local battery: `cargo generate-lockfile`
  + `cargo fmt --check` only. The canonical compile/test witness is the
  non-required `Linux frontend (Qt)` CI job (`ubuntu-24.04` + the apt
  list above, fmt → clippy → test → build, all `--locked`, plus the QML
  PlainText tripwire).
- SSE consumption is a **recorded deferred upgrade** — this tree polls
  at 200 ms (gtk3 parity); the promotion-gates matrix names the upgrade.
- Tests are headless (no QApplication/QQmlEngine) and never bind the
  fixed product port.
- Headless unit suites live in `config.rs` / `tokens.rs` / `model.rs`
  — **15 test functions, 15 passing on the CI witness** (sanitizer
  literalness, chip mapping exhaustiveness, pause/resume truth table,
  progress clamps, settings clamp/parse/merge behavior, proxy
  display/parse, seed cycling, token hex format). Locally the same
  suites run via the RUNBOOK's scratch-crate probe (a LIB crate with
  path deps — no Qt needed).

## Drift rules

- This tree mirrors the shared core like its siblings (win32/gtk3/
  slint/Swift): the canonical sources are `../crates/*`; any engine
  behavior change lands there first. When copying rendering logic,
  `gtk3/src` is the reference tree.
- The MD3 token math is NEVER re-implemented here — `vdm-md3` is a path
  dependency and the single source of truth.
