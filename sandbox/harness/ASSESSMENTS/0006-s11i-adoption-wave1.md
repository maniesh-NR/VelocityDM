# Assessment 0006 — S11-i adoption wave 1 (gate record)

**Date:** 2026-11-21 (UTC). **Change under review:** the S11-i
(incorporation) wave-1 execution plan — adopting the first three items of
the `docs/RESEARCH-IDM-STUDY.md` backlog into the engine: **B1**
(cross-connection content audit with single-connection recovery), **B2**
(optional connections: mid-download spawn/retire), **B9** (assembly/file
lock conflict classification and retry). All three are T2-provenance
findings (static RE of the reference product, 2026-11-21) and therefore
carry the standing stricter-spec-review bar: behavior specs in our own
words, committed BEFORE the implementing commit
(`docs/SPECS/S11-ADOPTIONS.md`), clean-room barrier restated there.
**Panel:** the five-role panel per PROTOCOL.md (ATKR / FALS / REQ / INT /
REL), dispatched independently with the same evidence pack (the study
backlog + the drafted wave-1 specs + repository access). This is the
activation gate for the incorporation stage the ROADMAP S11 brief
requires ("any incorporation work: five-role gate + spec-first + A/B
bench per brief").

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | B1 audit thread must read `.part` blocks UNDER the shared file mutex — a torn read (worker writing concurrently) would false-positive a mismatch and trigger a needless full-file re-download, an amplification of a local race into ~100% wasted bandwidth; recovery loop against a hostile alternating-content server must be hard-capped (spec says 2 — enforce + test) or the engine can be made to loop re-downloading forever; `notice` text must be static/ours (never server-controlled bytes) so no injection rides the JSON-RPC surface; B2 raising connection counts touches the server-blacklisting risk the reference product itself warns about — absolute cap 32 and extra cap 8 must be enforced, and spawned workers must inherit the existing stagger; token-bucket-limited tasks must NEVER spawn optional connections (a rate-limited task reads as "unsaturated" and would spawn-storm); `no_audit` is a local RPC knob only, document it as such |
| FALS | **PROCEED-WITH-CONDITIONS** | the B1 value claim is falsifiable and must be DEMONSTRATED: an inconsistent-edge test (server serving different bytes for the same range across requests) must show detect → recover → byte-identical final file, else the feature is untested code shipping as "stability"; the B2 win must be measured in a spawn-triggering scenario (base `--conn 2` under the 40 ms-RTT bench server), otherwise "no regression" is vacuous for the feature; audit overhead must be MEASURED (bench `http_requests`/bytes delta), not asserted ≤3.1%; anchor pick must be deterministic (lowest-index unaudited fully-written block) or tests flake on scheduling; the full-file-reset response to ONE mismatched 256 KiB block is expensive — the spec must argue why partial recovery (re-fetch from the mismatching block onward) is insufficient (it is: the audit samples 1/32 of bytes, so earlier unaudited regions of inconsistent edges are suspect too) rather than silently copying the reference behavior |
| REQ | **PROCEED-WITH-CONDITIONS** | spec-first barrier is honored only if the specs commit precedes the implementation commit in the same push — enforce the commit split; wave-2 deferrals (B3 HLS BYTERANGE, B4 NMH resilience, B5 cookie forwarding, B6 takeover UX, B7 detector hardening, B8 MIME table) must be recorded explicitly in ROADMAP with target areas, not silently dropped; N1–N6 stay out and B10–B16 park with the observe-first note per the study; every spec must restate the T2 provenance tier and the no-verbatim-copy barrier; UX surfaces wave-1 owes: `notice` reaches the RPC snapshot (frontends render later waves), B9 wording is ours; no release/tag actions, version unchanged, the 8 required checks must stay green |
| INT | **PROCEED-WITH-CONDITIONS** | `TaskSnapshot` gains `notice: Option<String>` — BOTH literal construction sites (engine/mod.rs insert_snapshot, events.rs test) must be updated (compiler catches), and every status-JSON consumer must tolerate an added key: CLI (Value-based, tolerant), desktop frontends (serde without deny_unknown_fields — verify by grep), Swift Codable (tolerant by design); CRITICAL: android/shared `Vdmctl.kt` parses the `.vdmctl` file with `ignoreUnknownKeys = false` — the wave must NOT add fields to `ResumeState` (spec must state this as a hard constraint); win7 tree is a near-copy of the main tree (6-line MSRV-syntax delta) — the patch must be ported to `win7/crates/vdm-core` with 1.75-compatible syntax and verified with a locally installed 1.75 toolchain, not just "CI will catch it"; the monitor-loop restructure (dynamic worker spawning for B2 + reset for B1) must preserve the existing pause/stop responsiveness (≤~250 ms under rate limits) — the paced-chunk re-check points stay; e2e/extension batteries are untouched (no JS changes in wave 1) |
| REL | **PROCEED-WITH-CONDITIONS** | one revertable implementation commit (engine + tests + win7 port; docs/gate commit separate before it) so `git revert` byte-restores behavior; A/B bench evidence must be recorded in MEMORY/RUNBOOK (not left in /tmp only) with the pre/post labels and the spawn-scenario run; CI witness required before close: all 8 required checks green at the pushed HEAD (plus observe the non-required frontend jobs); failure policy: red CI ⇒ fix-forward or revert, never close with red main; backup closes the plan (Law 1) with zero dirty files (BACKUPS manifest tripwire); no tag cuts — the v-tag ruleset is active anyway |

## Consensus

Zero `BLOCK` → **the gate closes with this record.** All conditions were
folded into the wave-1 plan below and into
`docs/SPECS/S11-ADOPTIONS.md` before the implementation commit; the
condition→plan mapping is itemized below.

## Conditions and how each was folded

1. **File-locked audit reads** (ATKR-1) → spec B1 §procedure step 2:
   the audit thread preads the anchor block while holding the same
   `Arc<Mutex<File>>` the workers write under.
2. **Recovery cap enforced + tested** (ATKR-2, FALS-5) → spec B1
   §recovery: at most 2 recoveries per task; third mismatch fails the
   task with our own message; the inconsistent-edge e2e covers one
   recovery; a pathological-server unit covers the cap by construction
   (shared recovery entry point, counter checked before every recovery).
3. **Static notice text** (ATKR-3) → spec B1 §surfaces: the notice and
   failure strings are fixed literals in our words; the only dynamic part
   is the numeric block range.
4. **Connection-count safety** (ATKR-4, REQ) → spec B2 §caps: absolute
   32, extra ≤ 8, size-based scale cap honored; spawned workers inherit
   the stagger pattern (wid-scaled sleep).
5. **No optional connections under rate limits** (ATKR-5) → spec B2
   §eligibility: disabled whenever the task rate limit or the global
   bucket rate is non-zero.
6. **Falsifiable demonstrations** (FALS-1, FALS-2) → test obligations in
   the spec: (a) inconsistent-edge e2e: detection, recovery, single
   connection observable, byte-identical output; (b) spawn-scenario bench
   run (base `--conn 2`) recorded alongside the standard 8-connection
   run.
7. **Measured overhead** (FALS-3) → the bench note reports
   `http_requests` and elapsed deltas pre/post; the ≤3.1% bound is a
   claim to verify, not assume.
8. **Deterministic anchors** (FALS-4) → spec B1 §ledger: anchor =
   lowest-index block present in `written` and absent from `audited`.
9. **Full-reset rationale, not imitation** (FALS-6) → spec B1 §recovery
   records the sampling argument: audits cover 1/32 of bytes, so a
   mismatch indicts the whole multi-connection region, hence full-file
   re-download on one connection (cap 2).
10. **Commit split / spec-first** (REQ-1) → this gate + the spec doc +
    ROADMAP/MEMORY records commit FIRST; implementation is a second
    commit in the same push.
11. **Wave-2 recorded, not dropped** (REQ-2) → ROADMAP S11-i brief lists
    wave 2 (B3, B4, B5, B6, B7, B8) and the parked candidates; N1–N6 stay
    rejected.
12. **Provenance + clean-room restated** (REQ-3) → spec header + each
    item carries the T2 provenance flag and the no-copy barrier.
13. **No ResumeState change** (INT-3) → spec B1 §non-goals: the android
    strict `.vdmctl` reader constrains us — audit ledger is in-memory
    only; recovery works through the existing pool/state machinery.
14. **win7 port + local 1.75 witness** (INT-4) → plan: install the 1.75
    toolchain locally, port with conservative syntax, `cargo +1.75
    check` in win7/ as the pre-CI witness.
15. **Monitor restructure safety** (INT-5) → the worker keeps its
    control re-check points (loop top + paced chunks); the reset flag is
    checked at the same points; spec states pause responsiveness is
    unchanged.
16. **Bench evidence recorded** (REL-2) → results summarized in the
    MEMORY close entry (labels, deltas, spawn-scenario numbers).
17. **CI witness + failure policy + clean backup close** (REL-3/4/5) →
    standard: poll-checks.sh witness, fix-forward-or-revert, zero dirty
    files at backup.

## Recorded (not fixed this session)

- Wave-2 items (B3–B8) are gated sub-stages; their own five-role gates
  run at activation, per stage discipline.
- The link-class connection presets (the B2 "presets" half) are deferred
  to wave 2 — wave 1 delivers the dynamic spawn/retire core only.
- Frontends rendering `notice` (GUI banners for the recovery event) is
  cosmetic follow-up; wave 1 guarantees the RPC snapshot carries it.
- The audit stride (8 MiB) and block (256 KiB) constants are
  deliberately conservative; tuning is a later data-driven decision,
  not this gate's.
