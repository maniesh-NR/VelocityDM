# Assessment 0009 — S11-i adoption wave 3 (gate record)

**Date:** 2026-11-24 (UTC). **Change under review:** S11-i wave-3 adoption
— **B17** (ranged-request coding discipline + ranged-response invariants),
**B18** (If-Range validator on in-run ranged requests; definitive
changed-resource classification), **B19** (expired-link guidance:
refreshable-failure classification + extension popup hint). All three are
T2-provenance pointers from the D7:B/D8:now deep-dive SESSION 2
(`docs/RESEARCH-IDM-STUDY.md` §4c, executed 2026-11-24 with Ghidra 12.2 +
REA 6.10.1 under the standing fail-closed/clean-room barriers; the pinned
artifact was re-verified byte-exact at session start). Activation authority:
the owner's round-3 directive recorded in `harness/DECISIONS.md`
("Use reverse engineer anything … decompile, reverse engineer and analyse
IDM … and incorporate any methods/techniques to improve performance and
stability of the download engine and browser integration") — the ledger
binds it to this wave's scope. **Spec-first:** the behavior specs were
committed in `docs/SPECS/S11-ADOPTIONS-WAVE3.md` BEFORE any implementing
commit; implementation sources are the public specifications (RFC 9110
§8.4/§12.5.3/§13.1.5/§14) — the study only pointed at our gaps.

**Panel:** five-role per PROTOCOL.md (ATKR / FALS / REQ / INT / REL),
dispatched independently with the same evidence pack (study §4c + the
staged wave-3 specs + repository access + the decision round 3 record).

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | B17's header rule must not fight user intent: if the caller supplied `Accept-Encoding` via options.headers, the engine MUST NOT add a second value header (duplicate-name ill-formedness) — the spec already says this; test pins it. B17 failure strings are engine-internal and must not be confused with the B19 user-facing marker: keep two distinct string namespaces. B19's guidance text is static and neutral (no URL/cookie/log leakage; the popup already has an error surface — reuse it, no new surface, no new permission). B18's If-Range header carries a server-issued validator (ETag/Last-Modified) — opaque data echoed back to the server only, never to logs. |
| FALS | **PROCEED-WITH-CONDITIONS** | Every guard needs a falsifiable table: B17's invariant matrix (valid 206 / compressed 206 / multipart 206 / shifted start / short end / absent Content-Range) with the classification asserted per row; the audit path asserted INCONCLUSIVE (not conflict) on invalid responses — a false B1 conflict signal would be a regression masquerading as a guard; B18 asserts no further mirror attempts after the changed-resource verdict (fast failure is the claim); B19 asserts the marker prefix stability and that other statuses keep byte-identical strings. Bench A/B expected byte-identical; any deviation is investigated, never explained away. |
| REQ | **PROCEED-WITH-CONDITIONS** | Spec-first honored only by the commit split (gate + specs + study §4c/backlog + ledger + CLEAN_ROOM record FIRST, implementation commits after). No `ResumeState` change anywhere — B18's validator is in-memory, cross-restart persistence recorded as a non-goal. No new permissions, no RPC/option surface change (B19 rides the existing error string). Win7 tree port + `cargo +1.75.0 check --workspace --locked` for B17/B18/B19-engine; per-item revertable commits; adoption-status notes land in study §5 rows; no release/tag actions (v-ruleset active). T2 provenance flagged per item for the stricter spec review — satisfied by the public-spec implementation sources stated per item. |
| INT | **PROCEED-WITH-CONDITIONS** | The ranged paths touched are: probe `ranged_get` (advisory), audit `ranged_get` (advisory, must stay non-fatal), worker segment GET (authoritative). The B17 checks must be applied per-path with the correct severity (probe=ignore, audit=inconclusive, worker=retry-path invalid) — one shared classifier function, three severity mappings, or the paths WILL drift. `Accept-Encoding` suppression rule: only when neither `req.headers` nor `cfg.extra_headers` carries one (case-insensitive check — headers are compared case-insensitively elsewhere, keep that). The HLS BYTERANGE path (wave-2 B3) has its own 206 validation — do NOT couple it to B17's engine checks (different code path, different spec); verify no shared code is accidentally tightened. Popup hint only on the marker prefix — the popup renders errors in an existing element (grep first, no new DOM). |
| REL | **PROCEED-WITH-CONDITIONS** | One activation docs commit (this gate + specs + study + ledger + CLEAN_ROOM) pushed FIRST; then one revertable implementation commit per item (B17, B18, B19-engine, B19-extension may share a commit if atomic — prefer separate). Local battery before push: fmt both trees, clippy `-D warnings`, vdm-core lib + http_engine + hls_e2e suites, win7 `+1.75.0 check --locked`, extension node battery. Bench A/B labels `s11i3-pre`/`s11i3-post` recorded in MEMORY (warm-pair rule), not just /tmp. CI witness via poll-checks.sh before any close; zero dirty files at backup time; throwaway dir wiped at close (the standing deep-dive rule); disk discipline: clean `target/` between tree builds. |

## Consensus

Zero `BLOCK` → **the gate closes with this record.** All conditions were
folded into `docs/SPECS/S11-ADOPTIONS-WAVE3.md` (already written with the
severity mappings, suppression rule, distinct string namespaces, and
non-goals the panel required) and the execution plan below.

## Conditions → plan mapping

1. No duplicate `Accept-Encoding` (ATKR/FALS/INT) → B17 spec rule 1 + unit
   test (user header wins, case-insensitive check).
2. Distinct string namespaces (ATKR) → B17 reason strings (engine-internal)
   vs B19 marker prefix (user-facing); asserted separately in tests.
3. Audit path inconclusive, never conflict (FALS/INT) → B17 spec rule 3 +
   audit-path test.
4. Per-path severity mapping via one classifier (INT) → implementation
   applies one `classify_ranged_response` helper with caller severity.
5. Fast-fail asserted, no retry churn (FALS) → B18 spec rule 3 + test.
6. Marker prefix stability (FALS) → B19 spec rule 1 + test.
7. Spec-first commit split + per-item commits + CI witness + backup close
   (REQ/REL) → the execution plan below.
8. Win7 ports + 1.75 witness (REQ) → per-item ports, checked before push.

## Execution plan

1. Commit 1 (docs): gate + specs + study §4c/backlog + ledger round 3 +
   CLEAN_ROOM record.
2. B17 implementation (root + win7): `classify_ranged_response` helper +
   identity-encoding rule + worker/audit/probe wiring + unit matrix.
3. B18 implementation (root + win7): validator plumbing (ProbeResult →
   task state) + If-Range header + changed-resource fast-fail + tests.
4. B19 implementation: engine classification strings (root + win7) +
   popup hint (extensions/chromium) + extension battery test.
5. Battery: fmt/clippy/test both trees + `+1.75.0 check` + extension
   battery; bench A/B (labels `s11i3-pre`/`s11i3-post`, warm-pair rule).
6. Push main; poll CI (11 checks); comment issue #15 with the wave-3
   close evidence; MEMORY entry; backup; wipe the throwaway dir.
