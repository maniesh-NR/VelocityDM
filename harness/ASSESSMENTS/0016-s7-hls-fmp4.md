# Assessment 0016 — S7 HLS fMP4 + SAMPLE-AES (gate record, round 1 — NO consensus)

**Date:** 2026-11-24 (UTC). **Stage under review:** S7 "HLS: SAMPLE-AES
+ fMP4 EXT-X-MAP" (ROADMAP row S7, PENDING). **Change under review:**
the behavior spec `docs/SPECS/S7-HLS-FMP4-SAMPLEAES.md` (round-1
draft).

**Panel:** five-role per PROTOCOL.md (ATKR / FALS / REQ / INT / REL),
dispatched independently with the identical evidence pack. Agents saw
no peer verdicts. FALS fetched the public RFC 8216 text during the
review.

## Verdicts (round 1)

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | Unknown METHOD values pass through unencrypted (`SAMPLE-AES-CTR`/`AES-256` → ciphertext written as success) — parse-time METHOD allowlist {NONE, AES-128, SAMPLE-AES}. The three promised loud-fail reasons are unreachable from the worker path (silent returns; generic message) — raise classifications parse-time/main-thread. Zero-key fallback (`unwrap_or([0u8;16])`) decrypts with an all-zero key; MAP-captured keys + SAMPLE-AES keys miss the prefetch loop. `aes128_cbc_decrypt`'s PKCS7 is wrong for sub-sample regions (needs raw-CBC + an ADTS sync-word integrity gate; also the only viable video detector — CODECS is discarded). Empty MAP URI joins to the playlist itself. Byterange arithmetic (cursor, Range end, exact-total) unchecked u64. Init allocation bound unstated (512 MiB cap must ride init; parse-time refusal for absurd BYTERANGE lengths). Error strings interpolate attacker-controlled attributes (KEYFORMAT etc.) — sanitize_error_detail + never interpolate URIs. IV present-but-invalid silently falls back to seq_iv. Spec contradicts its own helper ("hex-free" vs the hex-tolerant wave-2 helper). KEYFORMAT must be an allowlist (absent/identity), not a DRM denylist. Test encrypt helper: `#[cfg(test)]`-only, mirrored win7, no shipped surface. |
| FALS | **BLOCK** | **The SAMPLE-AES section inverts the actual sub-sample layout and cites a nonexistent RFC section.** RFC 8216 §4.3.2.4.1 does not exist; RFC 8216 delegates TS-with-AAC/AVC encryption to [SampleEnc] (Apple). The real rule ([SampleEnc] §2.3.1.1, confirmed against ffmpeg's reference decoder): ADTS header + 16-byte CLEAR leader, then a contiguous encrypted section (possibly ZERO blocks), then a 0–15-byte clear trailer — the draft's "leading 16 bytes encrypted" would decrypt clear bytes and leave ciphertext intact = 100% corrupt audio with zero loud failures. CBC chains WITHIN each frame's encrypted region with the IV RESET per frame ([SampleEnc] §2.1), not across frames. The "bounded slice" hides a required TS demuxer (PAT/PMT/PES, ITU-T H.222.0) — the bulk of the stage. A new crypto primitive (raw-CBC, no PKCS7) is required but unstated; a CBC-encrypt test helper cannot be built "over the existing decrypt primitives". MAP: the implicit-offset rule is not in RFC 8216 §4.3.2.5 (the bis/Apple rule) and §4.3.2.2's chain rule REQUIRES previous history else MUST fail — wave-2 implements exactly that fail; the "reuse verbatim" claim is false. AES-128-encrypted init REQUIRES the IV attribute (RFC §4.3.2.5) — the seq_iv fallback is wrong for init. Loud-fail completeness: AC-3/E-AC-3 + LATM + unknown types must loud-fail (PMT classification or CODECS plumbing needed); ≤16-byte frames = zero encrypted blocks. Bench has no signal for S7 (exercises no HLS path). "2nd edition" does not exist. |
| REQ | **PROCEED-WITH-CONDITIONS** | MAP implicit-offset rule mis-cited (RFC has no such rule; source is bis/Apple authoring spec) — name the real source or transcribe it. §4.3.2.4.1 does not exist — cite [SampleEnc]. AES-128 init-IV requirement missing (loud-fail, + test). Silent-passthrough holes: non-identity/non-DRM KEYFORMATs unpinned; codec detection grammar unpinned (PMT stream types) — pin the allowlist + classification. Loud-fail surfacing mechanism unspecified (parse-time vs fetch-time + propagation). Win7 MSRV guard weaker than precedent (is_multiple_of in the exact region being touched; 1.75 check witness owed). Init accounting edges (ranged exact vs unranged estimate) + test rows. "hex-free" parenthetical contradicts the helper. Honest-unsupported list incomplete + the wave-2 B3 non-goal goes stale (supersession note). |
| INT | **PROCEED-WITH-CONDITIONS** | Loud-fail surfacing: today workers communicate only `(idx, data)` and signal errors by silent return — the collector emits the generic message; pin the propagation (Result<_, String> over mpsc) + which checks fire where. Mirror mechanics: the two load-bearing divergence lines (`is_multiple_of` ↔ `% 16`, GenericArray Copy ↔ clone) must be enumerated as the ONLY permitted diff; `% 16` in the mirror vs clippy `manual_is_multiple_of` in the main tree — idiom split pinned; the CI win7 witness builds release binaries only (win7 test code compiled by NO gate; win7 hls.rs already contains repeat_n (1.82) — test parity is review-enforced). Test replacement: main-tree intent clear; mirror copy + the two doc rows silent — name both. e2e feasible with the existing TestServer (exact 206 ranges proven) — but the B17 identity claim mismatches the HLS path (identity lives in task.rs, not hls fetch) — pin whether ranged init gains identity. Encrypt helper: lift/mirror tests/hls_e2e.rs's existing aes128_cbc_encrypt, not a third copy; aes already a dep+dev-dep; clippy --all-targets tolerates test helpers by precedent. Progress accounting: exact-total + unranged init edge unpinned. |
| REL | **PROCEED-WITH-CONDITIONS** | No single-revertable-commit shape (C1→C4 split; C2 = the implementation revert unit). Revert-unit file scope incomplete (tests without paths: tests/hls_e2e.rs; inline cfg(test) modules; the REPLACED test's file). Close-out chain absent (ROADMAP S7 flip, MEMORY, 0016 record, BACKUPS row, tracker-issue comment, backup LAST). Bench-record artifact path missing (scripts/s7-bench-record.txt precedent). Push-main-only/no-tag language missing (release.yml triggers on any v* push). No rollback sentence. Release-neutral otherwise (verified: zero release.yml/tags/assets contact). Process note: the S5 spec cited as the rollback-shape precedent contains no such section — the actual carrier is S12-LIN2 §11 + REL columns of 0006–0013; S5 needs its own section at its gate (recorded — S5's revised spec has it). |

## Consensus (round 1)

**One `BLOCK` (FALS) → NO consensus.** Per PROTOCOL.md: the blocking
findings are fixed in the spec, then the whole panel re-runs FRESH on
the revised text.

## Blocking + condition → plan mapping (all folded into the revision)

1. **BLOCK resolution:** §2 rewritten — [SampleEnc] named as the
   normative source (RFC 8216 §4.3.2.4 has no sub-sample layouts);
   layout corrected (ADTS header clear + 16-byte clear leader +
   contiguous encrypted section possibly zero blocks + 0–15 clear
   trailer); CBC within each frame with per-frame IV reset; the
   minimal TS demux (PAT/PMT classification, PES reassembly, ITU-T
   H.222.0) added as declared scope with stream-type loud-fails
   (0x1b/0x24/0x81/0x87/0x11/unknown/no-audio); raw-CBC primitive
   declared; ADTS sync-word integrity gate added; zero-block edge
   pinned.
2. MAP rules re-sourced (FALS #5 / REQ #1): first-MAP-offset-0 +
   chaining declared as the bis/Apple interop choice, named;
   chain-without-history fails; AES-128 init requires the key IV
   (loud-fail; no seq_iv for init).
3. Parse-time hardening (ATKR #1/#3/#4/#6): METHOD allowlist,
   zero-key fallback removed + prefetch extended to MAP/SAMPLE-AES
   keys, empty-URI guards, checked byterange arithmetic, absurd-length
   parse refusal, 512 MiB init cap, sanitize_error_detail + no-URI
   interpolation, invalid-IV loud-fail.
4. KEYFORMAT allowlist (absent/identity) (ATKR #11 / REQ #4).
5. Surfacing mechanism pinned (ATKR #2 / INT #1 / REQ #5): parse-time
   vs fetch-time checks + classified-reason worker channel.
6. Grammar wording fixed (ATKR #10 / REQ #8): helper stays
   hex-tolerant; "2nd edition" dropped.
7. Test/encrypt-helper mechanics (INT #4/#5): lift the existing
   e2e encrypt helper; identity on ranged init pinned; unranged-init
   estimate pinned; init pre-pass keeps totals exact.
8. Win7 mirror mechanics (INT #2 / REQ #6): only-permitted-diff
   enumeration, banned post-1.75 APIs, 1.75 --all-targets witness,
   mirror test-list parity review-enforced (stated honestly).
9. Bench honesty (FALS #10 / REL #4): s7-pre/s7-post = general-path
   guard only, scripts/s7-bench-record.txt; the battery is the real
   witness.
10. Release posture + close chain (REL #1-6): C1→C4 split, C2 revert
    unit (hls.rs both trees + tests/hls_e2e.rs both trees), push-main-
    only, doc flips + wave-2 supersession note, backup LAST.

## Round 2

The revised spec (committed with this record) was re-gated by a FRESH
five-role panel: see `harness/ASSESSMENTS/0016b-s7-fresh-panel.md`.
S7's implementation commit occurs only after that record shows
consensus.
