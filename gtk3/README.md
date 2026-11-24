# GTK3 frontend (S12-lin2 — preview)

The "older-distro Linux" desktop stack: GTK3 via the gtk3-rs bindings
(`gtk 0.18.2`, the mature 0.18 binding line chosen over the newer 0.19
for stability parity with the other preview stacks). Like the egui GUI
and the Slint frontend, the binary embeds the shared Rust core
in-process (engine + JSON-RPC server on `127.0.0.1:36990`), renders
Material You chrome from the shared `vdm-md3` token engine through a
`GtkCssProvider`, and talks to the SAME `config.json` / `ui.json` as
every other frontend.

**This is a PREVIEW surface** (`harness/ASSESSMENTS/0012-s12-lin2-gtk3.md`,
spec `docs/SPECS/S12-LIN2-GTK3.md`): it ships in NO release asset; the
egui GUI remains the shipped Linux surface until this frontend's own
promotion gate. The compile/clippy/test witness is the CI
`gtk3-frontend` job (ubuntu-latest + `libgtk-3-dev`); there is NO local
GTK in the dev sandbox (no sudo), so local work is limited to
`cargo generate-lockfile` + `cargo fmt` (see harness/RUNBOOK.md
§"gtk3 preview (S12-lin2)").

## Build (where GTK3 exists)

```sh
sudo apt-get install -y libgtk-3-dev pkg-config
cargo build --manifest-path gtk3/Cargo.toml --locked
./gtk3/target/debug/velocitydm-gtk3            # add --portable for exe-dir config
cargo test  --manifest-path gtk3/Cargo.toml --locked   # headless-safe (no gtk::init)
```

## Layout

- `src/config.rs` — the shared `config.json`/`ui.json` mirror
  (tolerant load; `--portable` → exe-dir `velocitydm.config.json`).
- `src/theme.rs` — the MD3 stylesheet generator (hex from `vdm-md3`
  tokens only — no runtime strings in CSS), the task-state chip
  mapping, and the pure settings-apply logic. Unit-tested headless.
- `src/main.rs` — the GTK3 app: add path (URL / magnet: / .torrent /
  filename override / quality / speed limit), task list with in-place
  row updates (pause / resume / remove, progress, stats, error +
  notice lines), settings panel, seed cycling + light/dark toggle,
  200 ms poll refresh. Every attacker-controlled string renders
  through `GtkLabel::set_text` — this crate never calls `set_markup`.
