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

Reclaim disk when the toolchain is idle: `rm -rf
/home/z/my-project/tools/{ghidra,jdk}` (≈1.23 GB; the runbook above
restores it).
