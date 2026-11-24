# S12-lin2 — GTK3 Linux desktop frontend (behavior spec)

**Status:** activation gate 0012 (`harness/ASSESSMENTS/0012-s12-lin2-gtk3.md`,
5× PROCEED-WITH-CONDITIONS, zero BLOCK). This spec is committed BEFORE any
implementing commit (spec-first barrier). Stage type: standard S12 frontend
sub-stage — **no S11/T2 linkage, no T2 provenance**; the CLEAN_ROOM bench
A/B bar does not apply (zero engine code changes). The frontend is a
PREVIEW surface until its own promotion gate: it ships in NO release asset,
`release.yml` is untouched, the `v*` tag ruleset (id 27298722) stays active
and untouched, and the work is push-main-only.

## Scope

**IN** (each row becomes a gtk3 column value in
`docs/SPECS/S12-PROMOTION-GATES.md` §1.1–§1.5 at the CLOSE commit,
grounded in the implemented code — never pre-filled at activation):

1. In-process engine binding: `Engine::new(config)` +
   `RpcServer::start(engine, "127.0.0.1:36990")` (slint parity), honest
   bind-failure exit ("is another VelocityDM frontend running?"), no new
   listener, trust boundary lives only in `vdm-rpc`.
2. Shared configuration: this frontend reads/writes the SAME
   `config.json`/`ui.json` as the daemon, egui GUI and slint frontend
   (XDG `$HOME/.config/velocitydm/` or `$XDG_CONFIG_HOME`, `--portable`
   → exe-dir `velocitydm.config.json`), via a module mirroring
   `crates/velocitydm-slint/src/config.rs`; tolerant load (any read or
   serde failure → defaults, never panic).
3. Add path: URL entry (`GtkEntry::activate` = Enter-to-add), filename
   override (a real entry bound to `HttpOptions.filename` — gtk3 CLOSES
   the registered Win32/Slint/Swift gap), quality preset entry (default
   `best`), speed-limit entry (`parse_bytes`), `magnet:` URLs
   (`vdm_core::bt::Magnet::parse` → `BtSpec::Magnet`), local `.torrent`
   path typed by the user (`BtSpec::TorrentBytes`; same routing as
   slint — no file-picker helper this stage), plain http(s)
   (`add_download`). Add errors surface on an inline error label under
   the add row (cleared on the next successful add).
4. Task list: `GtkListBox` rows updated IN PLACE keyed by task id (no
   per-tick clear/rebuild) — name, state chip (text + CSS class),
   progress bar, stats line (done/size, speed, ETA, BT `p/s`), per-task
   error text, per-row Pause/Resume/Remove buttons with the same
   can-pause/can-resume state mapping as slint; total-speed banner in
   the header bar and an empty-state label when no tasks exist.
5. Settings panel (revealed section): download dir entry, connections
   spin (clamp 1..=32), max-active spin (clamp 1..=16), global limit
   entry (`parse_bytes`, empty = none), proxy entry
   (`ProxyConfig::parse_url`, empty/`none` = clear), scheduler stop
   window (`parse_hhmm` on both ends before storing),
   shutdown-when-done check button; Apply starts from
   `engine.config()` so fields this panel does not expose survive
   (`user_agent`, scheduler details), then `set_config` +
   `save_config` + re-push.
6. Material You: tokens from `vdm-md3` (`Scheme::from_seed_hex(seed,
   dark)`); seed cycling over `vdm_md3::SEEDS` (button, slint parity),
   light/dark toggle; seed label shown. Token subset = the same 19 of
   27 Scheme roles slint consumes (stated honestly — NOT "full token
   parity").
7. Refresh: `glib::timeout_add_local` at 200 ms polling
   `engine.list()` (slint parity). Matrix wording: "glib timeout 200 ms
   poll (in-process; SSE change is Swift-only)" — the SSE upgrade and
   stale-rows banner are this surface's own LATER gated sub-stage, not
   owed here (`S12-SSE-WIRING.md` scope is macos-app only).
8. Exit path: after the GTK main loop returns,
   `engine.shutdown()` + `server.stop()` (no orphan process; the
   owner-verification checklist depends on this).

**OUT** (explicit): no `release.yml` change, no tag/ruleset change, no
release asset gains `velocitydm-gtk3`, no e2e-smoke additions (headless
CI cannot open a GTK window), no bench A/B (zero engine change), no
egui retirement (the egui GUI remains the shipped Linux surface until
this frontend's promotion gate), no README frontend-table creation
(none exists; at most an optional Layout line, choice recorded), no
file-picker dialogs (typed paths, slint parity), no SSE.

## Rules (gate-0012 conditions folded)

1. **Crate mechanics — the `frontend/win32` precedent, NOT `win7/`**
   (which is a vendored-copy fork): `gtk3/Cargo.toml` is a single
   `[package]` with EXPLICIT version/edition/license/description (no
   `version.workspace = true` — the detached tree has no
   `[workspace.package]`), path-deps `vdm-core`/`vdm-rpc`/`vdm-md3` on
   `../crates/*` with default features (tls on — slint parity), direct
   deps pinned `gtk = "0.18.2"` + `glib = "0.18"` + explicit
   serde/serde_json, and a trailing empty `[workspace]` table (without
   it cargo errors: "current package believes it's in a workspace").
   Pin rationale: the 0.18 line (0.18.2 final) is the mature gtk3-rs
   binding line chosen over the newer 0.19 for stability parity with
   the other preview stacks; CI GTK 3.24 satisfies the sys minimum.
2. **Root workspace untouched:** `gtk3/` is NOT a root member — root
   `Cargo.toml`/`Cargo.lock` unchanged, required-job blast radius zero
   by construction; committed `gtk3/Cargo.lock`; CI builds `--locked`
   (deliberately stricter than the win32 job, justified because there
   is NO local compile witness); `.gitignore` gains `/gtk3/target`
   (the dirty-count tripwire stays honest).
3. **CI witness:** one non-required preview job `gtk3-frontend`
   (label `Linux frontend (GTK3)`), ubuntu-latest, rustup stable +
   rustfmt + clippy components, `sudo apt-get update -y && sudo
   apt-get install -y libgtk-3-dev pkg-config` (pkg-config listed
   explicitly — `system-deps` hard-requires it), then — all with
   `--manifest-path gtk3/Cargo.toml`: `cargo fmt --all -- --check`,
   `cargo clippy --all-targets -- -D warnings`,
   `cargo build --locked`, `cargo test --locked`. fmt rides the
   preview job ONLY (win32 precedent; the required `Rust format` job
   gains no third line and no name changes).
4. **Text rendering policy:** every attacker-controlled string (task
   name, url, dir, out_path, error, notice, stats) renders through
   `GtkLabel::set_text()` — `set_markup()` is used NOWHERE in this
   crate; Pango markup cannot be injected from any server-controlled
   field (markup spoofing / silent label blanking class, gate 0012
   ATKR #1). A headless test pins the policy by exercising the
   rendering-path helper with `<span ...>` payloads.
5. **Static stylesheet policy:** the `GtkCssProvider` stylesheet is
   generated ONLY from `vdm_md3::Scheme` fields formatted as hex
   (`u32` → `#rrggbb`) plus static selectors — zero `String`
   interpolation (no task names, paths, proxy strings or seed names in
   CSS, gate 0012 ATKR #2). The provider is rebuilt and reloaded on
   seed/dark toggle (GTK3 CSS has no `var()` custom properties).
6. **Unsafe discipline:** `#![forbid(unsafe_code)]` on the crate;
   no `glib::clone!`/`glib::closure!` macros (manual closures only) so
   the lint cannot trip on macro-expanded unsafe.
7. **Threading:** GTK is main-thread-only; engine access happens on
   the GTK main loop (the `Engine` is `Arc`-shared and the 200 ms
   poll + button handlers run as local callbacks); no channels/threads
   added by this stage.
8. **Headless test discipline:** tests NEVER call `gtk::init()`,
   `gdk::Display::open`, `gtk::Application::new`, and NEVER
   `RpcServer::start` on the fixed product port (bind-collision
   flake). Honest test surface (all pure logic, link-only headless on
   CI): state→chip text/class mapping (exhaustive over `TaskState`),
   config/`UiPrefs` serde round-trips + tolerant-load,
   stylesheet generation (contains expected token hex; only `Scheme`
   input), progress-fraction math (clamp, zero-size), proxy display
   formatting, filename/limit/quality option assembly.
9. **Battery honesty:** local sandbox has NO `libgtk-3-dev` and NO
   sudo — the ONLY local steps are `cargo generate-lockfile` (inside
   `gtk3/`) and `cargo fmt --manifest-path gtk3/Cargo.toml --all -- --check`.
   Compile/clippy/test verification happens ONLY in CI
   (`gtk3-frontend`). No record may claim a local check/build/clippy/
   test of this tree. Root workspace battery (fmt/clippy/scoped tests)
   runs as usual and is unaffected by this stage.
10. **Docs deliverables at close:** parity-matrix gtk3 column (§1.1–
    §1.5, honest ✓/✗/~ including the total-speed, empty-state,
    Enter-to-add, add-error rows and the "in-process (shares core)"
    stale-rows cell, plus the annotation that egui's filename-override
    ✓ is a dead field — no widget binds `add_filename`);
    `docs/BUILDING.md` GTK3 section (apt deps + manifest-path build +
    the no-local-GTK honesty note); `docs/DESIGN-MATERIAL-YOU.md`
    stack-matrix row flip + consumers-table note;
    `harness/ROADMAP.md` S12-lin2 status flip to "DONE (preview)" at
    the CLOSE commit only; `harness/RUNBOOK.md` §"gtk3 preview
    (S12-lin2)" (~8 lines: local limits, CI witness, the failure-
    policy distinction — a red `gtk3-frontend` does NOT block tag cuts
    because nothing ships, unlike `android-frontend`; rollback =
    revert of the implementation commit); `gtk3/README.md` (win7/
    README precedent: build recipe, honest preview note);
    `harness/MEMORY.md` session entry; issue #16 close comment with
    the verbatim wording "CI 12/12 green at \<sha\> (8 required +
    4 preview); zero dispatches, zero release.yml edits, v-tag ruleset
    untouched, push-main-only". §3 owner-verification checklist and §2
    packaging-plan row for gtk3 are OWED before the gtk3 promotion
    gate is queued (recorded in the gate record, not owed this stage).
11. **Commit split:** C1 gate record + this spec (before any
    implementing commit) → C2 implementation, the single revertable
    commit (`gtk3/**` + additive ci.yml job + `.gitignore` line +
    BUILDING.md section; revert byte-restores main) → C3 close (docs
    flips + matrix + records) → C4 backup LAST with zero dirty files
    at manifest time.
