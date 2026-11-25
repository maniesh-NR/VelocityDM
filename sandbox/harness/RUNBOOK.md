# Operational runbook — publish, recovery, protection

Serves the REL (Release Blocker / SRE) mandate: what happens the day after
publish, and how to recover when things go wrong. Complements
`WORKFLOW.md` (the two laws) — this file is about the mechanics.

## Normal publish flow

1. Local verification battery is green (see `MEMORY.md` checklist).
2. `bash /home/z/my-project/scripts/publish-velocitydm.sh`
   - pushes main, force-moves the six tags ONE PER COMMAND (GitHub
     suppresses push events when more than three tags move at once),
     verifies each tag on the remote, then idempotent repo config.
3. `bash /home/z/my-project/scripts/poll-runs.sh 1800`
   - exit 0 = all workflows for the pushed HEAD completed and green
     (the poll window is anchored to that commit's runs; historical
     failures on older commits are shown as context but never fail the
     gate);
   - exit 1 = failures (see the `X` rows it prints);
   - exit 2 = budget exhausted, still pending — just run it again.
4. Verify each of the six releases has assets and that asset timestamps
   postdate the pushed HEAD (fresh CI builds clobbered the floor).
5. Verify branch protection on `main` (see below).
6. Close the execution plan with a sandbox backup (Law 1).

## Rollback: a release shipped something broken

Releases are built from tags; the fix is always "move the tag and let CI
clobber":

1. Commit + verify the fix locally (full battery).
2. Push main; re-cut ONLY the affected tag(s) at the new HEAD and move
   them — one per command:
   `git tag -fa -m "<msg>" <tag> <HEAD> && gitpush origin -f refs/tags/<tag>:refs/tags/<tag>`
   (never push a stale local tag: an unchanged ref is a silent no-op and
   fires no workflow).
3. The Release workflow re-runs for that tag; its publish job
   `gh release upload --clobber` replaces the stale assets with fresh ones.
4. Re-verify: `gh release view <tag> --json assets` timestamps and the
   SHA256SUMS entry.

If a tag must stop existing (bad content, e.g. a leaked secret):

1. `git push origin :refs/tags/<tag>` (deletes the remote tag).
2. Delete the release via API
   (`DELETE /repos/<repo>/releases/<id>`) — releases keep dangling
   otherwise.
3. Re-cut the tag at the fixed commit and push; the workflow recreates
   the release.

## Partial failure (one platform red, others green)

- The Release `publish` job self-skips when any platform job fails
  (`!contains(needs.*.result, 'failure')`), so the affected release keeps
  its previous assets — nothing is half-clobbered.
- Diagnose the red job, fix on main, re-move that tag only (see rollback).
- The per-platform tags (`-linux`, `-windows`, ...) are independent:
  one being red does not block the others' releases.

## Branch protection on main

Already configured on the remote (verified 2026-11-13): the 8 required
checks — `Rust format`, `Clippy`, `Test (ubuntu-latest)`,
`Test (windows-latest)`, `Test (macos-latest)`,
`Windows 7 payload (Rust 1.75)`, `Extension tests`, `End-to-end smoke` —
with `strict: false` and `enforce_admins: false`.

- Direct pushes to main are NOT blocked by required checks in this
  configuration; CI runs afterwards on every push. That matches how this
  project actually publishes (agent pushes main directly).
- **Changing code after protection is on:** the documented workflow is
  PR-based (`ci.yml` already runs on `pull_request` to main) — branch,
  commit, PR, wait for the 8 checks, merge. For hotfixes pushed directly,
  CI still runs post-hoc on the push; a red run means revert or fix-forward
  immediately.
- To reconfigure protection (e.g. enable strict mode for admins), the
  token holds Administration:write; document any such change here.

## Token / auth failure modes

- Push 403 with a token whose API `permissions` block advertises
  `push: true`: the fine-grained PAT lacks Contents:write in fact — probe
  with a Contents API PUT (create + delete a probe file) before trusting
  the permission list (this cost three publish retries once).
- Non-fast-forward rejection of main: fetch + merge `origin/main` first
  (the reconciliation merge of 2026-11-13 was exactly this scenario).
- Tag push "rejected": the tag exists remotely with a different target —
  force-move it (see rollback).

## Backup / restore posture

- `sandbox-backup` branch carries: `scripts/`, `release-notes/`, the live
  `harness/` tree, `MANIFEST.txt` (repo HEAD, tags, dirty count, asset
  sha256s). Recovery order after a sandbox loss: MEMORY.md ->
  STAGE-STATE/ROADMAP -> latest assessment -> manifest.
- NOT recoverable from backups: the token (by design), binaries
  (checksums only — rebuild from the repo), uncommitted working-tree
  changes (commit before backup; the manifest's dirty-count is a tripwire:
  non-zero at backup time means something was left uncommitted).

## Ghidra headless in the sandbox (S11-T2 toolchain — verified 2026-11-14)

`analyzeHeadless` runs in this sandbox. Verified end-to-end on a 16 KB
ELF: import → auto-analysis → runtime script compilation → headless C
decompilation of a targeted routine (`scripts/ghidra/ProbeS11.java`,
shipped and backed up). Measurements: **~10 s wall, 407 MB peak java
RSS, exit 0** (2 CPUs, 4 GB RAM machine). Persistent footprint
~1.23 GB under `/home/z/my-project/tools/` (excluded from backups by
design — re-install per this section if lost).

### Install (recovery) steps

1. Ghidra 12.2 zip from the official release
   (`NationalSecurityAgency/ghidra`, tag `Ghidra_12.2_build`; SHA-256
   `2c2cd776db3c1f6a097ba418922378ac3649cadd5e844a6f5f088ae628a7be67`
   — verify with `sha256sum`), unzip to
   `/home/z/my-project/tools/ghidra`.
2. **JDK 25 required** (`Ghidra/application.properties`:
   `application.java.min=25` — JDK 21 is rejected). Temurin 25 from
   `https://api.adoptium.net/v3/binary/latest/25/ga/linux/x64/jdk/hotspot/normal/eclipse`,
   untar to `/home/z/my-project/tools/jdk` (the system
   `/usr/bin/java` is a JRE-only 21 — insufficient: script compilation
   needs `javac`).
3. Set `JAVA_HOME_OVERRIDE=/home/z/my-project/tools/jdk` in
   `tools/ghidra/support/launch.properties` — the no-TTY launcher does
   NOT honor the `JAVA_HOME` env var and otherwise dies with "Unable to
   prompt user for JDK path".
4. The headless project's PARENT directory must exist before the run
   (`analyzeHeadless <projdir> <name>` will not create it).

### Usage pattern

```sh
export JAVA_HOME=/home/z/my-project/tools/jdk
tools/ghidra/support/analyzeHeadless <projdir> ProjName \
  -import <binary> -scriptPath /home/z/my-project/scripts/ghidra \
  -postScript <Script>.java -deleteProject
```

Notes: Ghidra's PE loader covers the S11 target format (PE32 i386,
`x86:LE:32:default`) as a built-in capability, but the verified probe
exercised the ELF path only — no local PE test target exists, and the
IDM artifact itself stays untouched pending the T2 clearance. A 12 MB
packed installer will need minutes-to-hours of analysis time and
measured heap tuning on this 4 GB machine; budget accordingly (Ghidra
12 computes the default heap from system RAM; the probe ran with
defaults without OOM).

### Bounded deep-dive pattern for IDMan.exe (D7/D8, verified session 1)

IDMan.exe's 2.97 MB code image cannot be auto-analyzed inside the
10-minute foreground tool budget (the ~47 KB installer stub alone took
~2.5 min). The deep-dive uses a no-analysis import plus two evidence
scripts (all in `scripts/ghidra/`, backed up with the toolbox):

1. `unpack_idm.py <installer> <throwawayOut>` — extracts the container
   (marker `!--END FILE--!` in the last 0x5000 bytes; u32 table offset
   4 bytes before it; 32-byte entry headers `[usize][clen][3×
   FILETIME]` + zlib stream; `<P#="name">` manifest patterns).
2. `analyzeHeadless <proj> P -import IDMan.exe -noanalysis
   -postScript S11DeepDive.java <out>` — raw C-string scan for study
   needles + RTTI names, u32 back-reference scans for code use-sites.
3. `analyzeHeadless <proj> P -process IDMan.exe -postScript
   S11Decompile.java <out> <budgetSecs>` — backward function-start
   discovery (sites are imm32 operands: validate via
   getInstructionContaining, NOT getInstructionAt), corrected MSVC RTTI
   chain (the class name is INLINE at TD+8, so TD = nameAddr-8; refs
   point at the TD, not the name — walking refs to the name finds
   nothing), vtable slot enumeration, bounded decompilation with an
   internal time budget, and a callee crawl iterated over a SNAPSHOT
   (mutating a live set throws ConcurrentModificationException).

Session-1 result: 184 functions decompiled in ~36 s of script time
after a ~40 s project reopen. Keep the Ghidra project inside the
throwaway dir (wiped at close); helper scripts stay in the toolbox.

### rea attach mode (verified session 16, 2026-11-24 — the resource-safe provider lane)

rea (`npm i -g rea-agents`; v6.10.1) orchestrates native analysis through
provider engines; on Linux the provider is our Ghidra 12.2 install
(`rea doctor --provider ghidra --json` must be healthy). **Do NOT let rea
import a large binary itself** — its in-session bridge (2 GB heap) was
SIGKILLed by the 4 GB cgroup on the 6.5 MB engine image. The safe shape is
a caller-prepared project + rea attach mode:

```sh
# 1. caller-prepared project with bounded resources (248 s for 6.5 MB x86)
mkdir -p /tmp/vdm-t2/ghproj
GHIDRA_HEADLESS_MAXMEM=1536M JAVA_TOOL_OPTIONS="-XX:ActiveProcessorCount=2" \
  tools/ghidra/support/analyzeHeadless /tmp/vdm-t2/ghproj idman-study \
  -import /tmp/vdm-t2/IDMan.exe -analysisTimeoutPerFile 420 -max-cpu 2

# 2. attach rea (project marker = the .gpr file; program name WITHOUT path)
export GHIDRA_INSTALL_DIR=/home/z/my-project/tools/ghidra \
       JAVA_HOME=/home/z/my-project/tools/jdk \
       REA_GHIDRA_PROJECT_PATH=/tmp/vdm-t2/ghproj/idman-study.gpr \
       REA_GHIDRA_PROGRAM_NAME=IDMan.exe
rea search    /tmp/vdm-t2/IDMan.exe "Range:" --provider ghidra --json
rea xrefs     /tmp/vdm-t2/IDMan.exe 0x712a18 --provider ghidra --json
rea decompile /tmp/vdm-t2/IDMan.exe 0x58b79d --provider ghidra --json
```

Gotchas: pass `--provider ghidra` on every query; the attach env vars
must point at an EXPANDED `.gpr` (not the directory) and a bare program
name (no path, no wildcards) or rea refuses with `configuration_invalid`.
The attach lane reads a private copy of the project — original analysis
completeness is reported as unknown, so pair it with the bounded
deep-dive pattern above when completeness matters. Cross-check extractor:
`scripts/extract_idm_payload.py` (generic zlib/zip stream-carve over any
container) independently re-derived `unpack_idm.py`'s payload set.
T2 evidence lives ONLY in the throwaway dir — wipe at close.

Reclaim disk when the toolchain is idle: `rm -rf
/home/z/my-project/tools/{ghidra,jdk}` (≈1.23 GB; the runbook above
restores it).

## Rust toolchain in this sandbox (re-verified session 17, 2026-11-24)

The shims are per-instance: `~/.cargo/bin` is rebuilt as symlinks into
`tools/rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/{cargo,rustc,rustfmt,cargo-fmt}`,
with `RUSTUP_HOME=/home/z/my-project/tools/rustup` and
`CARGO_HOME=/home/z/my-project/tools/cargo` (registry cache is wiped with
the sandbox — the first build re-fetches crates, ~5 min).

- **There is NO rustup proxy**: `cargo +1.75.0` errors ("no such command").
  The win7 witness is the direct toolchain on PATH:
  `PATH=/home/z/my-project/tools/rustup/toolchains/1.75.0-x86_64-unknown-linux-gnu/bin:$PATH
  cargo check --workspace --locked` inside `win7/` (add `--features tls -p
  vdm-core` for the TLS-feature state). MEMORY entries saying `+1.75.0`
  predate this discovery.
- **clippy is absent from the minimal-profile toolchains.** If missing
  again: fetch `https://static.rust-lang.org/dist/channel-rust-stable.toml`,
  find `clippy-preview.target.x86_64-unknown-linux-gnu` for the installed
  rustc version, sha256-verify the tarball, copy `clippy-preview/bin/*`
  into the toolchain's `bin/` and `share/*` into its `share/`. `cargo
  clippy` finds the subcommand only when the toolchain bin dir is on PATH.
- Workspace-wide `cargo clippy --all-targets` trips a pre-existing
  `zbus_macros` compile issue — run clippy scoped to the touched crates
  (`-p vdm-core -p vdm-rpc -p velocitydm`), as every wave has.

## Android preview (S12-e — Views + KMP restructure)

The in-tree Android app is the S12-e **preview** (View-based Material 3
+ KMP `:shared` module, gate 0005). The six frozen v1.0.0-family
releases keep their Compose APKs; the Views app ships only via the
Android promotion gate (owner runtime verification + parity +
release.yml packaging decision).

**Enforcement:** a GitHub ruleset (`protect-v-tags-until-android-
promotion`, id 27298722, target=tag, pattern `refs/tags/v*`,
restrict creation+update, active) blocks ALL `v*` tag creation/moves —
including by the admin token (probed: "push declined due to repository
rule violations"). The promotion gate relaxes/edits the ruleset
deliberately. Consequences:

- The "Normal publish flow" above (publish-velocitydm.sh force-moves
  tags) CANNOT run until the promotion gate — and must not be worked
  around by deleting the ruleset ad hoc.
- `release.yml` `workflow_dispatch` is likewise forbidden until the
  promotion gate: a manual dispatch with a `tag_name` builds from the
  branch HEAD and would clobber frozen assets with the Views build.

**CI failure policy:** `android-frontend` (ubuntu-22.04, non-required —
same mechanism as the frozen release job: temurin 17 + Gradle 8.9)
runs `:shared:jvmTest` + `:app:assembleDebug` + `:app:assembleRelease`
(R8). If it is red while the 8 required checks are green: **block all
tag cuts** (moot while the ruleset is active, but the standing rule)
and fix forward on main — a red android-frontend means the Android
tree does not compile, and the next tag would ship a broken APK.

**Rollback:** `git revert` of the S12-e implementation commit
(allow_force_pushes=false on main — revert only). It touches only
`android/**` + the additive ci.yml job, so the revert byte-restores
the Compose app. Device-side rollback: reinstall the v1.0.0 APK (same
`applicationId`/versionCode — overwrite-install works).

**Local Android build (provisioned 2026-11-15, persistent under
`/home/z/my-project/tools/`):** JDK 17 (`tools/jdk17`, Temurin
17.0.21), Gradle 8.9 (`tools/gradle-8.9`), Android SDK
(`tools/android-sdk`: platform-34, build-tools 34.0.0, platform-tools),
Rust (`tools/cargo` + `tools/rustup` — RUSTUP_HOME/CARGO_HOME point
there). Run:

```sh
export JAVA_HOME=/home/z/my-project/tools/jdk17
export ANDROID_HOME=/home/z/my-project/tools/android-sdk
export GRADLE_USER_HOME=/home/z/my-project/tools/gradle-home
export PATH=$JAVA_HOME/bin:/home/z/my-project/tools/gradle-8.9/bin:$PATH
gradle -p /home/z/my-project/velocitydm-repo/android \
  :shared:jvmTest :app:assembleDebug :app:assembleRelease --no-daemon
```

`android/local.properties` (gitignored) pins `sdk.dir`. No Gradle
wrapper is committed — setup-gradle 8.9 in CI is the canonical build
mechanism. The `md3-tokens-baseline.json` drift tripwire regenerates
via `python3 scripts/gen-md3-baseline.py` (needs the Rust toolchain
above).

## GTK3 preview (S12-lin2)

The `gtk3/` tree is the S12-lin2 **preview** (gate 0012): standalone
win32-shaped workspace, in-process core on 127.0.0.1:36990, static
hex-only `GtkCssProvider` from `vdm-md3` (19/27 role subset). Ships in
no release asset until its promotion gate; `release.yml` and the `v*`
ruleset are untouched by the stage.

Two truths future sessions would burn time on:

- **Local work is fmt + generate-lockfile ONLY.** The dev sandbox has
  no sudo and no `libgtk-3-dev` (apt is unusable here), so `cargo
  build/check/clippy/test` against `gtk3/Cargo.toml` cannot compile
  locally — pkg-config/system-deps stop at the first sys probe. The
  canonical compile/test witness is the non-required `Linux frontend
  (GTK3)` CI job (ubuntu-latest + `libgtk-3-dev pkg-config`, fmt →
  clippy → build → test, build/test `--locked`).
- **Failure policy:** a red `gtk3-frontend` job is evidence-then-fix
  forward, but it does NOT block tag cuts or the release train (it
  ships nothing) — unlike a red `android-frontend` job, which blocks
  the frozen-asset refresh path. `git diff --exit-code gtk3/Cargo.lock`
  after any `generate-lockfile` run: the lock is committed and CI
  builds `--locked` (a drifted lock fails CI, not the fix-forward).

## SwiftUI preview (S12-mac2)

The `macos-swiftui/` tree is the S12-mac2 **preview** (gate 0013):
standalone SwiftPM package (`swift-tools-version:5.9`, macOS 12 floor,
zero third-party dependencies — no `Package.resolved` exists or is
owed), SwiftUI `@main` App over the shared token-engine port, remote
JSON-RPC + SSE client of the daemon on 127.0.0.1:36990. Ships in no
release asset until its promotion gate; `release.yml` and the `v*`
ruleset are untouched by the stage. Monterey users keep the AppKit app.

Two truths future sessions would burn time on:

- **Local work is source review ONLY.** The dev sandbox is Linux with
  no Swift toolchain — no local `swift build`, `swift test`, or format
  of this tree is possible, and no record may claim otherwise (spec
  rule 11). The canonical build/test witness is the non-required
  `macOS frontend (SwiftUI)` CI job (macos-latest, `swift build` +
  `swift test`); `swift build` alone does NOT compile test targets, so
  the golden-literal suite only runs under `swift test`.
- **Failure policy:** a red `swiftui-frontend` job is evidence-then-fix
  forward, but it does NOT block tag cuts or the release train (it
  ships nothing) — same class as a red `gtk3-frontend`; a red
  `android-frontend` remains the only CI failure that blocks the
  frozen-asset refresh path. The golden table is GENERATED
  (`scripts/gen-md3-golden-swift.py` from
  `android/shared/src/jvmTest/resources/md3-tokens-baseline.json`) —
  fix the generator, never hand-edit the test file; any color-math/seed
  change patches Rust → Kotlin → BOTH Swift trees in the same commit
  (rule 12).
