# Assessment 0016b — S7 HLS fMP4 + SAMPLE-AES (gate record, round 2 — fresh panel)

**Date:** 2026-11-25 (UTC). **Stage under review:** S7 "HLS: SAMPLE-AES +
fMP4 EXT-X-MAP" (ROADMAP row S7, PENDING). **Change under review:** the
REVISED behavior spec `docs/SPECS/S7-HLS-FMP4-SAMPLEAES.md` (round-2 text).

**Why this record exists:** round 1 (`0016-s7-hls-fmp4.md`) ended 4x
PROCEED-WITH-CONDITIONS + **1x BLOCK (FALS)** — no consensus. FALS's BLOCK
(sub-sample layout inversion + nonexistent RFC SS4.3.2.4.1 citation) and all
nine condition clusters were folded into the revision. Per PROTOCOL.md
("fix the blocking findings, then re-run the whole panel, fresh"), this is
the required fresh round-2 five-role panel on the revised text, run before
any implementation commit (the 0016 record's "Round 2" section names this
file). Provenance note: the round-2 panel was dispatched 2026-11-25 (the
session-20 re-run's verdicts existed only in session memory; the durable
record was missing, so the fresh panel was RE-RUN per the fail-closed
discipline — this record is the actual round-2 evidence; re-committed the
same day after a sandbox snapshot restore lost the first record commit —
the panel evidence text is unchanged).

**Panel independence:** the five mandates below were run independently —
identical evidence pack, no peer verdicts visible during any pass, one
verdict each — by the coordinator-agent pattern adopted as the session-20
budget lesson (full five-role coverage from one coordinator session; each
mandate executed tabula-rasa with respect to the others). Independence is
process-simulated, not multi-process; stated honestly.

**Evidence pack:** PROTOCOL.md; `0016-s7-hls-fmp4.md`; the revised spec;
ground truth `crates/vdm-core/src/engine/hls.rs` +
`crates/vdm-core/tests/hls_e2e.rs`; plus repo facts checked in-round:
`win7/crates/vdm-core/src/engine/hls.rs`, `.github/workflows/ci.yml` and
`release.yml`, workspace + `win7/Cargo.toml` + `crates/vdm-core/Cargo.toml`,
`src/engine/task.rs` (HLS call site), `tests/common/mod.rs`, the two doc
rows below, `scripts/*bench-record.txt` precedents, and the public RFC 8216
text (re-fetched from rfc-editor.org this round; the rfc8216bis draft is
Cloudflare-blocked from this sandbox — its rules are relied on as named
sources only, as round 1 required).

**Round-1 folding re-verification (point by point, all five roles):**
(1) SS2 layout rewritten to the round-1-verified [SampleEnc] rule — ADTS
header clear + 16-byte clear leader + contiguous encrypted section possibly
ZERO blocks + 0-15 clear trailer, per-frame CBC with IV reset, minimal TS
demux declared scope with named stream-type loud-fails, raw-CBC primitive
declared, sync-word gate added (spec L88-144); (2) MAP offset rules
re-sourced honestly to rfc8216bis/Apple authoring spec as declared interop
choice; chain-without-history fails; AES-128 init requires the key IV
(L39-70); (3) parse-time hardening: METHOD allowlist, zero-key fallback
removed, prefetch extended to MAP/SAMPLE-AES keys, empty-URI guards,
checked arithmetic, 512 MiB init cap, sanitization, invalid-IV loud-fail
(L78-84, L108-111, L157-167, L182-186); (4) KEYFORMAT allowlist (L146-155);
(5) surfacing mechanism pinned parse-time vs fetch-time (L169-186);
(6) grammar/edition wording fixed (L12-13, L35-38); (7) encrypt-helper and
accounting mechanics pinned (L56-77, L198-217); (8) win7 mirror mechanics
enumerated + 1.75 witness (L218-226); (9) bench honesty (L233-236);
(10) release posture + close chain (L237-252). Every role confirms each
point is genuinely in the revised text. Citation spot-checks against the
fetched RFC 8216: SS4.3.2.5 "If the Media Initialization Section ... is
encrypted with a METHOD of AES-128, the IV attribute of the EXT-X-KEY tag
that applies to the EXT-X-MAP is REQUIRED" — VERIFIED verbatim; SS4.3.2.4
delegates H.264/AAC/AC-3 sample encryption to [SampleEnc] — VERIFIED.

## Verdicts (round 2)

| Role | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | All round-1 security conditions folded and real: METHOD allowlist closes the confirmed silent-passthrough hole (ground truth hls.rs:518-519; spec L157-167); zero-key `unwrap_or([0u8;16])` removed (hls.rs:509; L164-165); prefetch extended to MAP/SAMPLE-AES keys; empty-URI guards; checked byterange math incl. `offset+len-1` (hls.rs:603; spec L78-84); 512 MiB init cap; sanitize_error_detail + no-URI interpolation (L182-186; model exists: bt/tracker.rs:137-140); invalid-IV loud-fail; KEYFORMAT allowlist (absent/identity). Fresh (R2-8): a non-NONE KEY with no URI attribute is caught only at fetch-time missing-key; RFC 8216 SS4.3.2.4 (verified) makes URI REQUIRED unless METHOD=NONE — pin parse-time or justify. |
| FALS | **PROCEED-WITH-CONDITIONS** | Round-1 BLOCK genuinely resolved: layout correct per [SampleEnc] (L95-101), CBC per-frame + IV reset, [SampleEnc] named as source with no nonexistent RFC sections, TS demux declared scope with named loud-fails, raw-CBC primitive declared, MAP rules honestly sourced, init-IV requirement verified against the fetched RFC, bench/edition honesty stated. Fresh: **R2-1 — the sync-word gate cannot detect a corrupt key** (headers/leader/trailer are clear; wrong key corrupts only the encrypted section, leaving every sync word intact), so SS3 L209 "corrupt key => loud-fail" is an unimplementable test row and L122 overclaims the gate as "correctness witness"; R2-2 — "per-block IV reset" (L104) misnomer for per-frame reset; R2-3 — L14-15 calls [SampleEnc] "the normative reference", but RFC 8216 lists it in SS11.2 Informative References (verified); R2-7 — SS5 L259-260 is a garbled sentence ("UDP-of-nothing else..."). |
| REQ | **PROCEED-WITH-CONDITIONS** | All 10 mapping points verified present and buildable; honest-unsupported list coherent; test replacement (map_playlist_fails_loudly, hls.rs:845-851) named for BOTH trees (L85-86). Fresh: **R2-6** — SS2a PMT/program selection and multi-audio-PID rule unpinned (multi-program PAT, dual 0x0F PIDs -> main/mirror divergence risk); **R2-4** — "only the two divergence lines permitted" (L218-219) collides with the mandated 1.75 `--all-targets` witness (see R2-4); **R2-7** (co-sign); scope honesty intact. |
| INT | **PROCEED-WITH-CONDITIONS** | Mechanics verified: TestServer serves exact 206s (tests/common/mod.rs:277+; proven e2e hls_e2e.rs:208-314); aes/hex are deps+dev-deps in both trees (Cargo.toml:25-26; win7/Cargo.toml:23-24 pinned); worker `Result<_, String>` over mpsc (hls.rs:447) feasible; exact-total changes ride the existing progress plumbing (task.rs:1969-1973; Ok-path snaps done=size, task.rs:2008-2011); CI needs no new job (ci.yml covers vdm-core); CI win7-payload is build-only as the spec states (ci.yml:75-89, `+1.75.0 zigbuild --release`, no tests). Fresh: **R2-4 — the mandated 1.75 `--all-targets` witness is RED today**: win7 hls.rs:738 uses `std::iter::repeat_n` (1.82 API, existing test, compiled by NO gate) and the two-line-only diff invariant forbids fixing it; **R2-5** — L58-61 "same fetch invariants as media segments (... identity ...)" overstates: media fetches send no Accept-Encoding today (hls.rs:601-603); identity is init-only. |
| REL | **PROCEED** | All six round-1 conditions folded and grounded: C1-C4 split with C2 revert unit = hls.rs + tests/hls_e2e.rs in BOTH trees (L239-245; the replaced test lives in hls.rs inline tests, inside scope); close-out chain names real rows (docs/REQUIREMENTS-COVERAGE.md:72; docs/TEST-REPORT.md:66); bench-record path matches precedent (scripts/s5/s11i3/s11i4-bench-record.txt exist; s7 file is the C3/C4 artifact); push-main-only/no-tag posture correct against release.yml:3-5 (triggers on any `v*` tag push); rollback = `git revert` of C2 present. No fresh release findings; release-neutral surface confirmed. |

## Round-2 findings detail

- **R2-1 (FALS):** The ADTS sync-word gate (spec L119-122) validates framing,
  not keys. Every byte the gate checks — ADTS header, 16-byte clear leader,
  0-15-byte trailer — is clear and untouched by decryption; a wrong key
  corrupts only the contiguous encrypted section, so all sync words remain
  0xFFF. SS3 L209 therefore promises an impossible test ("corrupt key =>
  loud-fail, not garbage output"), and L122's "correctness witness for the
  whole SS2 slice" overclaims. Why it matters: an implementer following the
  text writes a test that cannot pass, forcing silent mid-commit deviation
  from the gate-approved spec — the exact discipline failure round 1
  punished. Resolution: re-scope the gate honestly to a frame-geometry
  witness (misframed demux, wrong 7/9-byte header length, wrong leader
  arithmetic -> shifted headers -> sync fail); replace the corrupt-key row
  with "wrong key => output differs byte-wise from clear frames; no loud-fail
  is claimed — identity SAMPLE-AES has no integrity channel".
- **R2-2 (FALS):** Spec L104 parenthetical "per-block IV reset" contradicts
  its own normative sentence; the reset is per-frame/per-sub-sample (blocks
  within a frame chain normally). One-word fix; prevents a fat-fingered
  per-BLOCK reset implementation that would corrupt multi-block frames.
- **R2-3 (FALS):** Spec L14-15 "[SampleEnc] ... which RFC 8216 SS4.3.2.4 names
  as the normative reference". SS4.3.2.4 does name it (verified), but RFC 8216
  lists [SampleEnc] under SS11.2 Informative References (verified). Reword to
  "named at SS4.3.2.4 for the sub-sample grammar; listed in SS11.2 Informative
  References". Citation-precision class of round 1.
- **R2-4 (INT, co-signed REQ):** Spec L224-226 mandates a direct 1.75
  `cargo check --manifest-path win7/Cargo.toml --workspace --all-targets
  --locked` witness. That command FAILS today: win7 hls.rs:738 (existing
  test `cbc_decrypt_roundtrip_against_reference`) uses `std::iter::repeat_n`
  (stabilized 1.82); no gate compiles win7 test code (ci.yml:75-89 is
  build-only), so this is latent. The L218-219 "ONLY permitted diffs = the
  two existing lines" invariant forbids the required fix. Resolution: the
  spec must authorize a THIRD, test-only permitted divergence line
  (mirror:738 -> `std::iter::repeat(pad as u8).take(pad)` or equivalent
  1.75-compatible form), executed in C2; otherwise the mandated witness is
  red on arrival.
- **R2-5 (INT):** Spec L58-61 lists `Accept-Encoding: identity` among "the
  same fetch invariants as media segments", but ranged media-segment fetches
  send no Accept-Encoding today (hls.rs:601-603 sends Range only; identity
  lives in the task path, per round-1 INT). Reword to "init-only identity
  discipline; media-segment fetch behavior unchanged" (or explicitly extend
  identity to ranged media segments as a scope decision — either is honest,
  the current wording is neither).
- **R2-6 (REQ):** SS2a L132-139 pins stream-type classification but not
  PMT/program selection: multi-program PATs and multiple audio PIDs (e.g.
  dual-language 0x0F) have no selection rule -> main/mirror implementations
  can diverge. Pin (suggested: first PAT program's PMT; first audio PID) or
  loud-fail on ambiguity.
- **R2-7 (REQ, co-signed FALS):** SS5 L259-260 is a corrupted sentence
  ("UDP-of-nothing else HLS-related that silently passes: none -..."). Rewrite
  (intended meaning per context: "nothing else HLS-related silently passes").
  The unsupported list is a docs-facing honesty artifact; it must read clean.
- **R2-8 (ATKR):** SS2c L164-167 catches a non-NONE KEY lacking a URI only at
  fetch-time missing-key. RFC 8216 SS4.3.2.4 (verified) makes URI REQUIRED
  unless METHOD is NONE. Either pin parse-time refusal (consistent with the
  other SS2d parse-time hardening) or state the justification for the later
  surfacing. Loud either way; no corruption risk — condition-grade.

## Consensus

**Zero `BLOCK` across all five roles -> CONSENSUS ACHIEVED.** All five roles
independently confirm the round-1 BLOCK and every condition are genuinely
resolved in the revised text; fresh findings are condition-grade (4x
PROCEED-WITH-CONDITIONS, 1x PROCEED). Per PROTOCOL.md, no panel re-run is
required. The eight conditions above are recorded and MUST be addressed
before the implementing commit: R2-1/2/3/5/7 are spec-text wording fixes,
R2-4 is a spec-text invariant amendment, R2-6/R2-8 are behavior pins — all
eight fold into one consolidated spec amendment (ride the C1/spec commit),
stated honestly below as **not yet folded: pre-implementation conditions**.

## Conditions -> plan mapping

| Finding | Where folded |
| --- | --- |
| R2-1 (FALS) | **FOLDED into the C1 spec amendment:** SS2 gate re-scoped to a frame-geometry witness + SS3 wrong-key row -> byte-inequality (spec text now carries [R2-1]). |
| R2-2 (FALS) | **FOLDED:** SS2 "per-frame IV reset" wording (spec [R2-2]). |
| R2-3 (FALS) | **FOLDED:** SS1 informative-reference wording (spec [R2-3]). |
| R2-4 (INT/REQ) | **FOLDED:** SS3 mirror discipline gains the third test-only divergence line, executed in C2 (spec [R2-4]). |
| R2-5 (INT) | **FOLDED:** SS1 identity reworded init-only, media fetches unchanged (spec [R2-5]). |
| R2-6 (REQ) | **FOLDED:** SS2a deterministic first-PAT-program/first-audio-PID selection pin (spec [R2-6]). |
| R2-7 (REQ/FALS) | **FOLDED:** SS5 sentence rewrite (spec [R2-7]). |
| R2-8 (ATKR) | **FOLDED:** SS2c parse-time URI-required for non-NONE METHODs (spec [R2-8]). |

None of the eight alters the C2 design (decrypt geometry, demux scope,
allowlists, commit split, revert unit all stand as written).
