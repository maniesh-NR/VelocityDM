# Assessment 0001 — S1: Material You + CI fixes + harness (gating S2)

**Stage:** S1 (see ROADMAP.md) — gates S2 (publish/refresh v1.0.0).
**Date:** 2026-11-13 (UTC) — three rounds, per PROTOCOL.md.

## Round 1 — FAILED (zero-BLOCK rule violated)

Verdicts: RED=BLOCK, BLUE=PROCEED-WITH-CONDITIONS, AUDITOR=PROCEED-WITH-CONDITIONS.

Blocking findings (all verified, all fixed in the session):
1. The `Windows 7 payload (Rust 1.75)` required check built the ROOT
   workspace whose lockfile is format v4 (cargo 1.75 cannot parse) and whose
   deps resolve edition-2024 crates → jobs now build the pinned
   `win7/Cargo.toml` workspace.
2. `win7/Cargo.lock` had drifted (zeroize 1.9.1, edition-2024) → pinned
   `=1.8.1`.
3. Android module was unbuildable (missing catalog alias, missing compose
   compiler plugin, 10 Kotlin errors, inert themeMode, dead progress
   bridge) → fixed; local `gradle :app:assembleDebug` verified.
4. Stale tests (`tests/engine.rs` referencing a nonexistent Manager/tokio
   API) and dead files (`rpc_impl.rs`, `client.rs`, 10 undeclared modules)
   → deleted.
5. e2e_bin rewritten against the real wire surface; RPC `daemon.shutdown`
   now terminates the daemon.

## Round 2 — FAILED again (ship-path facts, verified by the panel)

Verdicts: RED=BLOCK, BLUE=PROCEED-WITH-CONDITIONS, AUDITOR=PROCEED-WITH-CONDITIONS.

New facts surfaced by the panel:
1. **The remote had diverged**: `origin/main` = 26343d7, a parallel Nov-08
   line of 17 commits — engine socket/WouldBlock fixes, seeder/tracker/RPC
   accept-loop fixes, osxcross Darwin builds, win7 rustls **0.23.46**
   security pin, Android Compose-plugin + MutableList fixes — with GREEN
   CI (CI + Release runs on 26343d7: success) and the six tags + six live
   releases already published. A plain push would die non-fast-forward;
   pushing tags would be rejected; creating releases would 422.
2. My win7 pins (rustls 0.23.5) regressed their security patch.
3. `backup-sandbox.sh` aborted on first run (unborn HEAD under `set -e`).
4. The local tree carried stale rsync orphans.

**Resolution (recorded decision):** reset the working tree to the
CI-proven `origin/main` line and layer the session's unique, non-overlapping
work on top:
- **Kept from the remote line (CI-proven):** the engine/bt/socket fixes,
  Darwin osxcross release builds, the win7 pinned workspace (rustls 0.23.46,
  zeroize 1.8.1, `--locked`, CARGO_TARGET_DIR pin), the desktop MD3 engine
  (`crates/velocitydm/src/theme.rs`, OKLCH tonal palettes + ui.json prefs +
  seed picker + light/dark toggle), the Android theming on the
  `com.velocitydm.android` tree, their test cleanups.
- **Layered from this session:** the agent harness (`harness/`), the
  event-driven backup workflow (`scripts/backup-sandbox.sh`,
  `.github/workflows/sandbox-backup.yml` — dispatch-only, never cron), and
  the **dynamic** Material You extension engine (`extensions/shared/md3.js`
  + seed pickers + custom seed + auto/light/dark persistence in popup and
  options, superseding the remote's static-token CSS; 12 new tests).
- **Deferred (v1.1.0 candidates, recorded in MEMORY.md):** the richer
  `com.velocitydm.app` Room/DataStore Android app (locally built and
  verified this session, but the CI-proven simpler tree ships 1.0.0); the
  local win7 i686 framestub (obsolete — the local failure was a
  cargo-zigbuild 0.19.8 bug; 0.24.1, what install-action ships today, links
  i686 cleanly with the remote's lock).
- Fixed: backup script unborn-HEAD (`|| echo new`), orphans deleted,
  extension floor assets rebuilt (md3.js inside all four zips).

## Verification state after reconciliation (all green)

- `cargo fmt --check` root + win7 ✓; `cargo clippy --workspace
  --all-targets -- -D warnings` ✓; `cargo test --workspace` 124 passed
  (their line's suite; the session's fuller e2e_bin was superseded by the
  reset — scripts/e2e.sh covers the full stack in CI).
- Both exact CI win7 zigbuild commands (x86_64 + i686, `--locked`) succeed
  locally with cargo-zigbuild 0.24.1 + zig 0.13.0.
- `node extensions/tests/run-tests.mjs` = 76 assertions (64 + 12 MD3);
  `node extension/tests/run.mjs` = 78 passed.
- Remote CI + Release pipelines were already green on this exact base
  (26343d7); the overlay touches only extensions/, harness/, README, and
  two new workflow/script files.

## Round 3 — consensus record

See `0001b-round3-consensus.md`.
