# Assessment 0003 — S11 roadmap amendment review

**Date:** 2026-11-14 (UTC). **Change under review:** owner-directed
addition of stage S11 (IDM competitive study — RE-assisted review of
the reference product's download-management and browser-integration
methods) to `ROADMAP.md`, plus its milestone issue. **Panel:** two roles
(RED — Adversary, REQ — Requirements Guardian) dispatched independently,
identical evidence, no peer verdicts shared. A roadmap amendment is not
a stage transition, so the full five-role gate is reserved for S11
**activation** (`PROTOCOL.md`); this two-role review is the amendment
gate.

## Owner directive (verbatim)

> Add to roadmap: decompile and reverse engineer the IDM install file
> (https://download.internetdownloadmanager.com/idman643build18.exe) to
> study if there are any methods of download management or browser
> integration that can be incorporated in to this project to improve
> performance, stability, and resource use.

## Verdicts

| Agent | Role | Verdict | Core findings |
| --- | --- | --- | --- |
| RED | Adversary | **PROCEED-WITH-CONDITIONS** | An agent panel cannot certify the legal basis for EULA-protected decompilation, and one gate ambiguously unlocking both activation and T2/T3; same-agent clean-room lacked spec-first barriers and a home/wipe rule for T2/T3 artifacts; provenance lacked the tier tag; no fail-closed re-pin policy for vendor rotation; deliverables omitted the CLEAN_ROOM.md/README disclosure; "unambiguous legal footing" overclaimed T1; sandbox reality (no Windows VM, no decompiler — only `objdump`; ~8 GB disk) contradicted implied capability; T3 trace/credential hygiene unspecified |
| REQ | Requirements Guardian | **PROCEED-WITH-CONDITIONS** | All six decomposed owner requirements satisfied (artifact fingerprint re-verified byte-exact against the live official URL; decompilation survives as a real named method; genuine open question; both domains; incorporation path; three improvement axes with objective criteria); but ROADMAP claimed milestone tracking that did not exist yet; T2/T3 clearance had no criteria and no owner-escalation path (a declined clearance could silently downgrade the owner's directive); "measured win" lacked a baseline mechanism; CLEAN_ROOM.md provenance row planned |

## Consensus

Zero `BLOCK` verdicts → **the amendment passes with conditions** (all
conditions were addressed in this same execution plan, before commit —
see below).

## Conditions and their resolution

1. T2/T3 clearance separated from the activation gate; owner-signed,
   dated clearance in the stage record; declined clearance escalates to
   the owner, never silently dropped (RED-1, REQ-2). → **Fixed in
   ROADMAP S11 T2 bullet.**
2. Spec-first clean-room barriers; T2/T3 artifacts only in a wiped
   throwaway directory outside the repo/backups; provenance carries
   tier + tool + build hash prefix + date; incorporation gate diffs
   implementation vs spec (RED-2, RED-3). → **Fixed in the
   clean-room-hygiene bullet.**
3. Fail-closed re-pin policy; official-URL-only re-downloads;
   activation-gate hash re-verification (RED-4). → **Fixed in the
   re-pin policy paragraph.**
4. Honest prerequisites: T1/T3 need a Windows-capable VM
   (owner-provisioned; absent in this sandbox), T2 needs a PE RE
   toolchain (e.g. Ghidra); until provisioned the deliverable is the
   study plan + literature-level analysis, never invented
   measurements (RED-7). → **Fixed in the prerequisites paragraph.**
5. CLEAN_ROOM.md disclosure + README wording as deliverables
   (RED-5, REQ-4). → **Added to the deliverables bullet.**
6. Before/after baseline mechanism via `bench-perf.sh`'s A/B pairs
   (REQ-3). → **Added to the deliverables bullet.**
7. T1 wording softened: "lowest-risk tier; license posture still
   recorded" (RED-6). → **Fixed.**
8. T3 trace hygiene: synthetic credentials; traces stay in the VM
   (RED-8). → **Fixed in the T3 bullet.**
9. Milestone issue created BEFORE commit so the ROADMAP tracking claim
   is true at commit time (REQ-1). → **Issue #15 created on the
   `v1.1.0 - roadmap` milestone in this plan.**

## Notes for the S11 activation gate (five-role panel)

- Re-verify the artifact hash + size against a fresh official-URL
  download before any work starts.
- Record the legal posture (license/RE clause, jurisdiction, owner
  authorization, panel-is-not-counsel statement); T2/T3 additionally
  require the separate owner-signed clearance.
- Prerequisites (Windows VM / PE RE toolchain) must exist by then, or
  the stage's deliverable stays at plan + literature level.
