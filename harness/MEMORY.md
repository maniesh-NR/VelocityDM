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
