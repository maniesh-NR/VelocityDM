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
   - exit 0 = all workflows completed and green;
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
2. Push main; force-move ONLY the affected tag(s):
   `gitpush origin -f refs/tags/v1.0.0:refs/tags/v1.0.0` (one per command).
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
