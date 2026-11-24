# S7 spec — HLS: fMP4 `#EXT-X-MAP` + SAMPLE-AES AAC (bounded slice)

Behavior spec for roadmap stage S7. **From-scratch conformance
feature** (not an S11 adoption — no T1/T2/T3 provenance tier applies;
the standing spec-first + gate discipline does). Gates:
`harness/ASSESSMENTS/0016-s7-hls-fmp4.md` (round 1: 4×
PROCEED-WITH-CONDITIONS + **1× BLOCK (FALS)** — no consensus; the
BLOCK's findings and every condition are folded into this revision)
and `harness/ASSESSMENTS/0016b-s7-fresh-panel.md` (the required fresh
re-run on this revised text). Primary public sources, read during
implementation and followed verbatim as behavior: **IETF RFC 8216**
(HTTP Live Streaming, Aug 2017 — single edition; no "2nd edition"
exists) and, for the SAMPLE-AES sub-sample grammar, **[SampleEnc] —
Apple's "MPEG-2 Stream Encryption Format for HLS"**, which RFC 8216
§4.3.2.4 names as the normative reference (RFC 8216 itself defines NO
sub-sample byte layouts). TS container grammar: ITU-T H.222.0
(ISO/IEC 13818-1), public. Target:
`crates/vdm-core/src/engine/hls.rs` (+ mirror
`win7/crates/vdm-core/src/engine/hls.rs`, MSRV-1.75-compatible
syntax). Today `#EXT-X-MAP` loud-fails (wave-2 B3 preserved S7 scope
with a loud-fail test) and `SAMPLE-AES` keys loud-fail only
generically (unknown METHODs silently pass through — fixed here).

**Hard constraints:** no new crate dependencies (the `aes`/`hex`
crates already in the tree suffice, including for the test encrypt
helper); no unsafe code; no `ResumeState` change; no RPC schema
change; the loud-fail discipline survives for everything out of the
slice — a DRM-fenced or shape-mismatched stream fails with an
explicit reason, never a corrupt output.

## 1. `#EXT-X-MAP` (fMP4 init segments) — RFC 8216 §4.3.2.5

- Parse `#EXT-X-MAP:` attributes: `URI` (required — present and
  non-empty after quote-stripping, else classified error), `BYTERANGE`
  (optional, `"length[@offset]"`). The attribute grammar is the
  wave-2 `parse_byterange_attr` helper AS-IS — which tolerantly
  accepts `0x` hex via `parse_hhmm_num` (wave-2 behavior unchanged;
  the earlier "hex-free" parenthetical was wrong and is dropped).
- **BYTERANGE offset rules (pinned, with sources):** RFC 8216
  §4.3.2.5 defines NO implicit-offset rule and §4.3.2.2's
  continuation rule (previous sub-range of the same resource, else
  the client MUST fail) is scoped to media segments. Our rule,
  declared as a deliberate interop choice sourced from the
  **rfc8216bis draft and Apple's HLS authoring spec** (both public,
  named here as the source): the FIRST MAP in a playlist with a
  BYTERANGE lacking an offset has offset 0; a SUBSEQUENT MAP's
  BYTERANGE lacking an offset continues from the end of the
  previous init segment; a subsequent MAP whose BYTERANGE lacks an
  offset AND has no previous init to chain from ⇒ classified error
  (mirror of the wave-2 "no previous range" rule).
- A MAP applies to the media segments that FOLLOW it, until a new
  MAP tag appears (RFC-consistent scope). Segments before the first
  MAP are unchanged. The current MAP — and the key state snapshot
  AT the MAP line, not the final playlist key — is per-segment
  state (mirror of the wave-2 per-segment key/byterange state).
- **Fetch + compose:** the init segment is fetched in a **pre-pass**
  (before workers spawn — required for ordering and accounting),
  with its BYTERANGE applied; it rides the same fetch invariants as
  media segments (512 MiB body cap, exact-length check, and
  `Accept-Encoding: identity` on ranged init fetches — the wave-3
  B17 identity discipline, which the HLS path gains here for init).
  An init declaring a BYTERANGE length > 512 MiB fails at parse
  time (defense in depth).
- **Encryption composition:** an init section encrypted with
  `METHOD=AES-128` decrypts with the key active at the MAP's
  position — and **RFC 8216 §4.3.2.5 makes the EXT-X-KEY `IV`
  attribute REQUIRED for AES-128-encrypted init sections**:
  a missing/absent IV there ⇒ classified loud-fail (the media
  segment `seq_iv` fallback does NOT apply to init — a MAP has no
  unambiguous media sequence number).
- Init bytes are written BEFORE the first segment they cover (and
  before the segments a later MAP covers). **Exact-total
  accounting:** ranged init lengths are known pre-fetch and add to
  the exact total (exact = Σ init_len + Σ seg_len); an UNRANGED
  init (whole-resource MAP URI) has unknown length until fetched —
  the total degrades to an estimate for that playlist, stated in
  the stage record (pinned behavior, both cases).
- **Arithmetic discipline (extends the wave-2 math):** all byterange
  cursor arithmetic (implicit-offset cursor, `offset+len-1` Range
  ends, exact-total sums INCLUDING init lengths) uses checked
  addition — overflow ⇒ classified `"absurd byterange"` error; a
  `length=0` BYTERANGE ⇒ the same classified error. (The negative-
  offset case is already refused by the shared number parser —
  parity confirmed, no change.)
- The wave-2 `map_playlist_fails_loudly` test is REPLACED (not
  appended) by the working-behavior tests below, in BOTH trees.

## 2. SAMPLE-AES — [SampleEnc] AAC slice over a minimal TS demux

**Normative source: [SampleEnc]** (Apple "MPEG-2 Stream Encryption
Format for HLS", referenced by RFC 8216 §4.3.2.4; RFC 8216 defines
no sub-sample layout). The implementer reads [SampleEnc] and follows
it verbatim. The correct sub-sample layout for AAC in ADTS is:

- Per audio frame: the **ADTS header (7 or 9 bytes) is CLEAR**, the
  next **16 bytes of the frame payload are CLEAR (the unencrypted
  leader)**, then a **contiguous encrypted section of 0 or more
  CBC-encrypted 16-byte blocks**, then a **0–15-byte clear
  trailer**. The encrypted section length is
  `floor((payload_len − header − leader) / 16) × 16` — it may be
  ZERO for a short frame (treated as fully clear, no error).
- **CBC discipline:** AES-CBC decryption runs within each frame's
  encrypted section, and the **IV is reset to the segment IV at the
  start of every frame** ([SampleEnc] §2.1 — per-block IV reset;
  there is NO cross-frame chaining). The segment IV is the explicit
  `IV` attribute when present, otherwise the media sequence number
  IV (RFC 8216 §5.2 — the existing `seq_iv` helper is correct for
  this). **An IV that is present but invalid** (empty, > 16 bytes,
  non-hex) ⇒ classified loud-fail — only a truly absent IV uses
  `seq_iv` (the current silent fallback-to-seq_iv for invalid IVs
  is fixed here; `iv_parsing` tests gain the invalid-IV cases).
- **New crypto primitive (declared):** a raw CBC decrypt helper —
  CBC with NO PKCS7 unpad, arbitrary block counts, caller-supplied
  IV — built on the existing `aes` crate's `BlockDecrypt`. The
  existing `aes128_cbc_decrypt` (whole-buffer + strict PKCS7) is
  NOT reused for sub-sample regions (its padding check would fail
  on arbitrary audio bytes). Only the AES-128 init-segment path
  (§1) keeps the whole-buffer PKCS7 primitive.
- **Anti-corruption gate:** after decryption, every frame's ADTS
  sync word (0xFFF) is validated — a mismatch ⇒ classified
  loud-fail `"decrypted stream failed the ADTS sync check"`. This
  gate is the correctness witness for the whole §2 slice.

### 2a. Minimal TS demux (required for §2 — declared scope)

SAMPLE-AES AAC lives inside MPEG-TS: locating frames requires a
bounded TS/PES walk (ITU-T H.222.0):

- 188-byte packet walk with sync-byte validation; malformed or
  unrecoverable packet streams ⇒ classified error.
- PAT parse → PMT selection → **stream-type classification**:
  `0x0F` (AAC, ADTS) = the supported slice; **everything else
  loud-fails with a named reason**: `0x1b`/`0x24` (AVC/HEVC video)
  ⇒ `"SAMPLE-AES video sub-sample decryption is not supported"`,
  `0x81`/`0x87` (AC-3/E-AC-3) ⇒ `"SAMPLE-AES AC-3 family is not
  supported"`, `0x11` (LATM) ⇒ `"SAMPLE-AES LATM is not
  supported"`, any other/unknown type ⇒ `"SAMPLE-AES stream type
  0xNN is not supported"`. No PES audio stream found ⇒
  `"no decodable audio stream in SAMPLE-AES segment"`.
- PES reassembly for the selected audio PID (payload-start
  indicators, PES header length skip) yields the contiguous ADTS
  frame stream the per-frame decryptor consumes. In-place TS
  container is PRESERVED (decrypt in place, no ADTS-extraction
  output change).

### 2b. KEYFORMAT allowlist (parse time)

`EXT-X-KEY` with `METHOD=SAMPLE-AES` is supported **only** when
`KEYFORMAT` is absent or `"identity"` (case-insensitive). Every
other value — DRM schemes (`com.apple.streamingkeydelivery`), vendor
formats, `urn:uuid:…` — ⇒ parse-time classified loud-fail naming the
(sanitized) format: `"SAMPLE-AES stream uses unsupported keyformat
<fmt>"`. An allowlist, never a DRM denylist. `EXT-X-MAP` combined
with a `METHOD=SAMPLE-AES` key ⇒ parse-time loud-fail
`"SAMPLE-AES fMP4 is not supported"`.

### 2c. METHOD allowlist (parse time, fixes a silent-corruption hole)

`parse_media` enforces the METHOD set **{NONE, AES-128, SAMPLE-AES}**
(case-insensitive). Any other value (e.g. `SAMPLE-AES-CTR`,
`AES-256`) ⇒ parse-time classified error — today unknown METHODs
silently pass ciphertext through as a "successful" download. A
missing key at decrypt time ⇒ classified error (the
`unwrap_or([0u8;16])` zero-key fallback is removed); the key
prefetch loop extends to MAP-captured keys and SAMPLE-AES keys (same
cache, same size bounds). An empty `URI` on a MAP or KEY tag ⇒
classified error (never fetch the playlist itself).

### 2d. Loud-fail surfacing (pinned mechanism)

- **Parse-time** (playlist-visible): DRM keyformat, MAP+SAMPLE-AES,
  unknown METHOD, MAP+AES-128-without-IV, invalid IV, absurd
  byterange — raised in `parse_media`/the pre-pass, surfacing
  naturally.
- **Fetch-time** (stream-shape: video/AC-3/LATM/unknown PMT types,
  sync-word failure, missing key): workers propagate a classified
  reason (worker error channel `Result<_, String>` over the
  existing mpsc pattern) and the collector surfaces the FIRST
  classified reason instead of the generic `"segment download
  failed before completion"` — the reason strings reach the task
  error line.
- **Sanitization:** a `sanitize_error_detail()` helper (strip
  C0/C1/DEL, modeled on the engine's existing control-char
  mapping) is applied to every playlist-derived substring before
  any `format!`; key/init URIs are NEVER interpolated into error
  strings (auth-token leakage).

## 3. Tests

- MAP parse: URI-only, URI+BYTERANGE, first-MAP offset-0, subsequent
  MAP chain, chain-without-history error, malformed BYTERANGE error,
  empty-URI error, length>512 MiB parse error.
- Composition: init bytes precede segments; multiple-MAP switch
  writes the new init before the segments it covers; ranged-init +
  ranged-segments exact totals; unranged-init estimate behavior;
  AES-128-encrypted init with explicit IV composes; **AES-128 init
  without IV ⇒ loud-fail**.
- e2e (loopback, `crates/vdm-core/tests/hls_e2e.rs` — the existing
  TestServer already serves exact 206 ranges; no server changes): a
  playlist with MAP + BYTERANGE init + two ranged media segments;
  output bytes == init + seg1 + seg2.
- SAMPLE-AES AAC: unit tests build encrypted ADTS frame pairs with
  the `#[cfg(test)]` CBC-encrypt helper (lifted/mirrored from
  `tests/hls_e2e.rs`'s existing `aes128_cbc_encrypt` — `BlockEncrypt`
  over the existing `aes` dep, no third copy, never shipped outside
  `#[cfg(test)]`); decrypt via the new per-frame path; assert byte
  equality with clear frames; zero-encrypted-block frames (short
  payload) treated clear; explicit-IV and seq-IV default both
  covered; invalid-IV loud-fail; sync-word gate (corrupt key ⇒
  loud-fail, not garbage output).
- TS demux: crafted minimal TS fixtures (PAT/PMT/audio-PES with
  0x0F; a video-PMT 0x1b fixture; AC-3 and LATM fixtures) —
  each loud-fail exercised with its named reason string.
- KEYFORMAT: DRM format ⇒ named loud-fail; `urn:uuid:` ⇒ loud-fail;
  absent/identity ⇒ proceeds; MAP+SAMPLE-AES ⇒ loud-fail.
- Unknown METHOD (`SAMPLE-AES-CTR`) ⇒ parse error; missing key at
  decrypt ⇒ classified error (zero-key fallback gone).
- Mirror discipline: the ONLY permitted win7 hls.rs diffs remain the
  two existing divergence lines (`is_multiple_of` ↔ `% 16 != 0`,
  GenericArray `Copy` ↔ `.clone()`); new code uses no post-1.75 APIs
  (`repeat_n`/`LazyLock`/`trim_ascii` banned in the mirror); the
  mirror's test list is updated identically (parity review-enforced
  — the CI win7 job is build-only, stated honestly). MSRV witness:
  the direct 1.75 toolchain runs `cargo check --manifest-path
  win7/Cargo.toml --workspace --all-targets --locked` inside
  `win7/`.

## 4. CI / bench / release posture

- No new CI job (vdm-core covered; win7 payload build-witnesses the
  mirror).
- Bench honesty: `scripts/bench-perf.sh` exercises the direct-HTTP
  segmented path, not HLS — the `s7-pre`/`s7-post` A/B pair (label
  arguments, recorded to `scripts/s7-bench-record.txt`) is a
  general-path no-regression guard ONLY; the real S7 witness is the
  test battery above. Stated in the record.
- **Release posture:** push-main-only; no tags (v* ruleset); no
  `release.yml` / `packaging/**` edits; locks untouched.
- **Commit split:** C1 = activation (spec + gate records 0016 /
  0016b); C2 = the single revertable implementation commit scoped to
  `crates/vdm-core/src/engine/hls.rs` +
  `crates/vdm-core/tests/hls_e2e.rs` +
  `win7/crates/vdm-core/src/engine/hls.rs` +
  `win7/crates/vdm-core/tests/hls_e2e.rs`; C3 = close docs; C4 =
  backup LAST, zero dirty files. Rollback = `git revert` of C2.
- **Close-out chain:** ROADMAP S7 row flip, MEMORY entry,
  BACKUPS.md row, tracker-issue comment, backup LAST; doc flips:
  `docs/REQUIREMENTS-COVERAGE.md` + `docs/TEST-REPORT.md`
  ("SAMPLE-AES and fMP4 EXT-X-MAP not supported" rows), and a
  supersession note on the wave-2 spec's B3 non-goal line (EXT-X-MAP
  loud-fail → supported since S7, loud-fail retained for the
  out-of-slice cases).

## 5. Unsupported after S7 (honest-unsupported list)

AVC/HEVC video SAMPLE-AES sub-sample decryption; AC-3/E-AC-3 and
LATM SAMPLE-AES; every non-identity KEYFORMAT (FairPlay/FPS family
and others); SAMPLE-AES over fMP4 (sample-table sub-sample boxes);
UDP-of-nothing else HLS-related that silently passes: none — every
unsupported shape above loud-fails with a named reason.
