# S6 spec — DASH downloader (.mpd manifests)

Behavior spec for roadmap stage S6. **From-scratch MPEG-DASH
conformance feature** (not an S11 adoption — no T1/T2/T3 provenance
tier applies; the standing spec-first + gate discipline does). Gate:
`harness/ASSESSMENTS/0015-s6-dash.md` (round 1: 5×
PROCEED-WITH-CONDITIONS, zero BLOCK; conditions folded into this
text). Primary source: the public ISO/IEC 23009-1 (MPEG-DASH)
media presentation description format (with ISO/IEC 14496-12 for the
SIDX box). Target: new module `crates/vdm-core/src/engine/dash.rs` +
registration in `crates/vdm-core/src/engine/mod.rs` +
`Error::Dash(String)` in `crates/vdm-core/src/error.rs` + dispatch in
`crates/vdm-core/src/engine/task.rs` + `HttpOptions.quality` reuse.
Win7 tree mirror: `win7/crates/vdm-core/src/engine/{dash.rs,mod.rs,
error.rs}` + the task.rs port, MSRV-1.75-compatible syntax.

**Hard constraints:** no new crate dependencies — including
dev-dependencies (the XML tokenizer is ours, in-module); no unsafe
code; no `ResumeState` change; no RPC schema change; `quality` reused
(no new options field); root and `win7/` `Cargo.lock` untouched.

## Error surface (pinned)

- New `Error::Dash(String)` variant (both trees), Display prefix
  `dash: `. Never reuse `Error::Hls` (would print `hls: …` for DASH
  failures); no string-discriminant threading (the B21b rule:
  never error-string matching). RPC-safe (Display-stringified; no
  schema change).

## Detection and dispatch

- Probe order: HLS first (unchanged), then DASH. A task is DASH when:
  the probe's **final** URL path ends with `.mpd` (the same
  final-URL check the HLS branch uses), or the probe content type is
  `application/dash+xml` / `video/vnd.mpeg.dash.mpd`, or — when
  neither extension nor content-type matched and the probe took the
  GET-fallback path — the probe's retained body prefix (≤ 1 KiB,
  new internal non-RPC `ProbeResult` field) contains an XML
  declaration plus an `MPD` root element. The sniff is bounded and
  fail-open (a long prolog that pushes `<MPD` past the window simply
  misses detection; extension/content-type remain the primary
  signals).
- Dispatch placement: `else if is_dash { run_dash(...); return; }`
  immediately AFTER the HLS dispatch and BEFORE the
  `probe.size == 0 || !probe.accepts_ranges` gate (an `.mpd` on a
  no-ranges server must not fall into `run_single`, which would
  download the manifest as the output file).
- While touching the probe: the HEAD-rejected (405) fallback path
  also refreshes `content_type` from the ranged GET (mirroring the
  existing etag fallback) — a one-line parity fix recorded in the
  stage record.
- Naming: mirror the HLS `.ts` forcing — after the B8 MIME
  suggestion (`application/dash+xml → .mpd`), the DASH branch strips
  `.mpd` and applies `.mp4`; an explicit `filename` option wins up
  to the same extension-forcing semantics `run_hls` applies today
  (pre-existing clobber behavior is mirrored for consistency and
  recorded honestly; fixing it is out of scope).

## MPD parsing (bounded, honest)

- **Static manifests only.** `MPD@type="dynamic"` (or
  `minimumUpdatePeriod` present without `type`) ⇒ loud-fail
  `"dash: live DASH (dynamic manifest) is not supported"`.
- **First Period only.** Multi-period manifests process the first
  Period; a completion notice records
  `"downloaded first period of N"` when N > 1. Cross-period concat
  is a recorded deferred sub-stage.
- **XML tokenizer (ours, in-module)**: minimal non-validating
  scanner — element open/close/self-close, attributes (single/double
  quoted, values capped at 4 KiB), the five named entities plus
  numeric character references (decoded via `char::from_u32`;
  invalid code points ⇒ classified error), comment skip, CDATA
  skip, processing-instruction skip. **DOCTYPE is skipped as
  literal markup; `<!ENTITY>` never defines an expansion** (an
  undefined `&name;` reference ⇒ classified error) — billion-laughs
  is structurally impossible. Namespace prefixes are stripped
  (local names only). Unknown elements are skipped via an ITERATIVE
  depth-counted scanner with a **nesting depth cap of 128**
  (deeper ⇒ classified error — no recursive skipper, no stack
  overflow abort). `xlink:href` on MPD-relevant elements ⇒
  loud-fail (never silently swallowed). Malformed XML ⇒ classified
  error, never a panic.
- **BaseURL resolution**: MPD/BaseURL and ancestor BaseURL elements
  resolve through `crate::http::Url` join semantics; absolute URLs
  override. All manifest-derived URLs flow through `Url::parse/join`
  (non-http(s) schemes are rejected there). **Trust-model decision
  (recorded):** manifest-declared segment hosts are fetched like
  HLS variant/key hosts today (no new SSRF boundary); the existing
  B20 emission-boundary hygiene covers header safety; no
  credential-class header differentiation is added in this stage
  (recorded as an observe-first item).
- **ContentProtection present anywhere in the selected adaptation
  set** ⇒ loud-fail `"dash: DRM-protected DASH (ContentProtection)
  is not supported"`.
- Manifest fetch rides the HLS playlist budget (16 MiB cap,
  `fetch_text` semantics).

## Representation selection

- Adaptation set preference: the first set whose `contentType` or
  `mimeType` contains `video` — **inspecting both the
  AdaptationSet attributes AND its child `ContentComponent`'
  `contentType`** (manifests frequently declare the type only on
  the child); if none, the first audio set by the same rule; if the
  type still cannot be determined ⇒ loud-fail
  `"dash: cannot determine stream type"` (never silently download
  audio for a video manifest).
- Within the set, `quality` mirrors `hls::choose_variant` EXACTLY:
  empty/`best` ⇒ highest `bandwidth`; `worst` ⇒ lowest; numeric ⇒
  the **nearest** bandwidth (`abs_diff`, ties resolve to the earlier
  document position); non-numeric junk ⇒ classified error. Selection
  is on `bandwidth` when present; absent bandwidth falls back to
  document order (first = best).

## Segment addressing (supported forms)

- **SegmentTemplate** may appear at Period, AdaptationSet, or
  Representation level; attributes inherit from the nearest
  ancestor unless overridden (child wins). Supported identifiers:
  `$RepresentationID$` (no format tag; must resolve non-empty),
  `$Bandwidth$`, `$Number$`, `$Time$`, zero-pad forms
  `$Number%0Nd$` / `$Time%0Nd$` (width ≤ 16), `$$` unescaping. A
  template form referencing an identifier the representation cannot
  supply ⇒ loud-fail (never emit a literal `$...$` into a URL).
  The `initialization` template may carry only
  `$RepresentationID$`/`$Bandwidth$` (a `$Number$`/`$Time$` there ⇒
  loud-fail). An absent `initialization` attribute = fetch media
  segments only (legal per 23009-1). A media template with NEITHER
  `@duration` NOR a `SegmentTimeline` ⇒ loud-fail.
- **Timescale**: `SegmentTemplate@timescale`, default 1
  (`presentationTimeOffset` ≠ 0 ⇒ loud-fail
  `"dash: presentationTimeOffset (PTO) != 0 is not supported"` in
  this stage — recorded honestly; PTO = 0 is the common VOD case).
- **Duration-based numbering** (no timeline): `startNumber` (default
  1); segment count = `ceil(period-length × timescale ÷ @duration)`;
  period-length = `Period@duration` else `MPD@mediaPresentationDuration`
  (an ISO-8601 `PTnS` duration parse — ours, bounded); neither
  present ⇒ classified error. `$Time$` for duration-based templates
  = PTO(=0) + (N − startNumber) × @duration, in @timescale units.
- **SegmentTimeline** (S elements with optional `@t`, `@d`, `@r`):
  `@t` absent = previous S end (0 for the first S); `@r` absent = 0;
  `@r ≥ 0` repeats; **`@r = -1` repeats until the Period end**
  (loud-fail when no Period duration is derivable); `@r < -1`
  invalid ⇒ classified error. All of `@t/@d/@r/@duration/@
  startNumber/timescale` parse with checked u64 arithmetic
  (overflow/`@d=0`/`@duration=0`/`timescale=0` ⇒ classified error).
- **Total segment-count cap: 100,000 per representation** —
  enumeration beyond ⇒ loud-fail
  `"dash: manifest enumerates too many segments"` (the zip-bomb
  analog guard; applies to timeline `@r` expansion and
  duration-based counts alike).
- **SegmentList** (Initialization/SegmentURL list, `mediaRange`
  byte-range forms — the attribute is `mediaRange`; no alias):
  fetched as listed.
- **SegmentBase@indexRange**: fetch the declared index range
  (capped at 8 MiB), parse the SIDX box: version, `timescale`
  (reject 0), `earliest_presentation_time`, `first_offset`
  (u32 in version 0, u64 in version 1), `reference_count`
  (capped at 100,000), then one referenced-size/duration entry per
  reference. **The first referenced range starts at (end of the
  sidx box) + first_offset**; each next range continues after the
  previous (`checked_add` on all offset+size math — overflow ⇒
  classified error); parse only from bytes actually fetched (box
  size claims beyond the buffer ⇒ classified error); consumed bytes
  must equal the declared box size. Media fetches are
  206-or-fail byte-range GETs — never degraded to whole-resource
  reads.
- One media segment = one HTTP GET, **sequential over a reused
  keep-alive connection — a deliberate deviation from `run_hls`'s 4
  concurrent workers** (recorded; DASH parallelization is a deferred
  sub-stage). The connection-reuse key extends to
  **scheme + host + port** (the HLS key is host+port only — a
  latent scheme-confusion bug there; the DASH code does not inherit
  it, and the HLS fix is a recorded follow-up, not this stage).

## Output

- Bytes written in order: initialization segment(s) + media
  segments. Progress updates mirror `run_hls`'s **throttled**
  snapshot path exactly (no unthrottled per-segment bus events).
  Byte totals: init length counts toward the total; segment counts
  are exact.
- Resume semantics = `run_hls` today, verbatim: in-process pause
  sleeps the fetch loop and continues mid-stream; a restarted task
  truncates the `.part` and starts from zero (no `.vdmctl` for
  DASH). Documented, not a silent assumption.

## Tests

- Tokenizer unit tests: entities incl. numeric refs, comments,
  CDATA, PI skip, DOCTYPE skip, undefined-entity error, nesting
  depth cap, malformed input error, 4 KiB attribute cap.
- Template expansion: zero-pad, `$$`, missing identifier loud-fail,
  `$Time$` timeline with `@t` gaps and `@r` repeats, `@r=-1` with
  derivable/underivable period end, PTO ≠ 0 loud-fail, init-template
  identifier restriction.
- Segment count: duration-based ceil math, startNumber, count-cap
  loud-fail.
- SIDX parse: crafted box bytes incl. `first_offset ≠ 0`, version 0
  and 1, truncated box error, reference-count cap.
- Representation choice: best/worst/numeric-nearest tie to earlier
  position/junk error; ContentComponent-only typing; audio-only;
  undeterminable loud-fail.
- ContentProtection + dynamic-MPD loud-fail tests.
- e2e (`crates/vdm-core/tests/dash_e2e.rs`, mirroring the
  `hls_e2e.rs` loopback pattern; uses only existing dev-deps):
  static manifest + init + two media segments served from a temp
  dir; produced bytes == init+seg1+seg2; `done == size`; ≥ 1
  intermediate `done` observation (no per-segment event promise).
- Win7 mirror tests are ported from the win7 tree's own
  `hls_e2e.rs` shape (NOT the main tree's — it uses post-1.75
  `std::iter::repeat_n`). **Banned APIs in the mirror:** anything
  newer than 1.75 (`LazyLock`/`trim_ascii` 1.80, `repeat_n` 1.82,
  `is_multiple_of` 1.87). MSRV witness: the direct 1.75 toolchain
  runs `cargo check --manifest-path win7/Cargo.toml --workspace
  --all-targets --locked` inside `win7/` (the CI win7-payload job is
  a build-only witness — stated honestly). Never regenerate
  `win7/Cargo.lock` with a modern cargo (it rewrites to v4).

## CI / bench / release posture

- No new CI job (vdm-core covered by the required Test matrix; the
  win7 payload job build-witnesses the mirror).
- `scripts/bench-perf.sh` A/B with label arguments `s6-pre`/
  `s6-post` recorded to `scripts/s6-bench-record.txt`; no-regression
  on the engine hot paths + full batteries green.
- **Release posture:** push-main-only; no tags (v* ruleset); no
  `release.yml` / `packaging/**` edits; `Cargo.toml` versions and
  both `Cargo.lock`s untouched.
- **Commit split:** C1 = activation (spec + gate record 0015); C2 =
  the single revertable implementation commit scoped to
  `crates/vdm-core/src/engine/{dash.rs,task.rs,mod.rs,error.rs}` +
  `crates/vdm-core/tests/dash_e2e.rs` + the `win7/` mirrors of the
  same four files + `win7/crates/vdm-core/tests/dash_e2e.rs`; C3 =
  close docs; C4 = backup LAST, zero dirty files. Rollback =
  `git revert` of C2.
- **Close-out chain:** ROADMAP S6 row flip (PENDING → DONE with
  gate/CI/bench evidence), MEMORY session entry, BACKUPS.md row,
  the DASH capability rows in `docs/REQUIREMENTS-COVERAGE.md` /
  `docs/TEST-REPORT.md` / README feature table flip where they say
  "not supported", tracker-issue comment, backup LAST.

## Deferred sub-stages / non-goals (recorded, each needs its own gate)

- Live/dynamic DASH; multi-period concat; audio+video muxing
  (a video+audio manifest yields a video-only file with the
  completion notice — a text/subtitle-track gate is likewise
  deferred); multi-representation download; DASH parallelization;
  PTO ≠ 0; a credential-header differentiation policy for
  manifest-declared hosts (observe-first).
