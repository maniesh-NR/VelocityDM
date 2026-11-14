# Assessment 0002b — S1R gate CLOSED (rounds 2+3 consensus)

**Date:** 2026-11-13 (UTC). **Panel:** the five adversarial roles of
assessment 0002 (ATKR / FALS / REQ / INT / REL), dispatched independently
per round. Per `PROTOCOL.md` (zero-OBJECT rule), the gate closes with this
record.

## Round 2 (remediation commit 4231e0c re-reviewed)

| Agent | Verdict | Notes |
| --- | --- | --- |
| ATKR | **PASS** | Security fix survived a 40-case raw-socket attack matrix (25 browser-reachable shapes refused; parser tricks unreachable from browsers); win7 parity byte-verified; tests assert what they claim. Note-level: request-line/Host CRLF (tracker-announce path) — closed in round-2 remediation commit. |
| FALS | **OBJECT — minor** | Residual doc wire-format falsehoods (snapshot fields, headers shape, addBatch/torrents.files params, heartbeat claim, extension pipeline mechanics, count 17→14). |
| REQ | **PASS** | Android deliverable + CI wiring verified intact on the reconciled line; extension/ cut verified; meta-requirements (backups law, harness, consensus) verified real; Material You verified on all three UI surfaces. Conditions: doc hygiene items. |
| INT | **OBJECT — blocker** | 4231e0c regenerated BOTH lockfiles to v4 (cargo 1.75 hard-fails under `--locked`; reproduced with a real 1.75 install); publisher pushed stale local tags (silent no-op, no workflow fires). |
| REL | **OBJECT — blocker** | Same retag defect (tags local==remote, no-ops); poll-runs.sh unanchored window exits 1 on 17 historical failures — ALL DONE unreachable. |

## Round 2 remediation (commit 759f9bb)

- Both `Cargo.lock`s restored to `version = 3` (diff was header + one
  sha2 line); win7 re-verified with `cargo +1.75.0 check --locked` — clean.
- Publisher: re-cuts all six tags at HEAD (`git tag -fa`) before the
  one-per-command force-push; verifies the remote tag OBJECT sha.
- `poll-runs.sh` window anchored to the pushed commit's `head_sha`.
- FALS's doc falsehoods and REQ's conditions fixed (see the commit).
- ATKR's note closed: request line + Host line CRLF/NUL-refused (both trees).

## Round 3 (commit 759f9bb re-reviewed by the remaining objectors)

| Agent | Verdict | Notes |
| --- | --- | --- |
| INT | **PASS** | Lockfiles v3 verified + `cargo +1.75.0 check --locked --workspace` finished clean (10.66s); publisher retag/verify mechanics validated; http.rs hardening present in both trees; fmt/clippy/tests clean (126/126 on touched crates). |
| REL | **PASS** | Retag loop dry-run-verified in a scratch clone: all six tag objects genuinely move (fires workflows; no-op structurally impossible); tag-object↔tag-object compare validated; anchored poller live-replayed (0 anchored, 30 legacy reds as context — round-2 false-failure dead). Note (minor, applied after this record): empty-anchored-set would green — guard added (green requires >= 1 anchored run). |
| FALS | **PASS** | All eight doc items verified against code (20/20 snapshot fields; headers array shape; addBatch/torrents.files params; no heartbeat claim; /health claim; extension pipeline one-shot + cap 50 + Firefox NMH id + 8 seeds; 14 assertions); no regressions in previously verified material. Note: RPC.md wording about per-field userAgent passthrough — clarified after this record. |

## Consensus

**REACHED** — five PASS, zero outstanding objections (rounds 2+3, per the
protocol: every OBJECT was remediated and re-verified by its own objector).
The two post-consensus touches (REL's >=1-anchored-run guard, FALS's
wording clarification) are tightening-only, documented here.

The gate opens S1R's terminal step: publish (fast-forward main, re-cut and
force-move the six tags one-per-command, delete cron 514177, poll the
anchored runs to green, verify all six releases carry assets that postdate
the pushed HEAD, confirm branch protection, close with the event-driven
backup).
