# S11-i adoption specs — wave 3 (B17, B18, B19)

**Provenance tier: T2** — pointer findings from `docs/RESEARCH-IDM-STUDY.md`
§4c (the D7:B/D8:now deep-dive session 2, executed 2026-11-24 with Ghidra
12.2 headless + REA 6.10.1 as the evidence layer), fingerprint
`7346128f…d62ee` re-verified byte-exact at session start (fail-closed).

**Clean-room barrier, restated:** nothing in this spec or its implementation
is copied code, data structures chosen for implementation reasons,
resources, string tables, or UI from the reference product. Where the
underlying MECHANISM is a public specification (RFC 9110 Range/If-Range/
Content-Range/Content-Encoding semantics, the `identity` media-coding rule),
the implementation source is the public spec — the T2 study only pointed at
the gap in our engine. All constants, error strings, schedules, and
decision thresholds below are ours.

**Standing constraints honored:** no `ResumeState` change (the Android
strict-schema reader mirrors it); additive-only RPC/option surfaces;
win7 tree port + `cargo +1.75.0 check --locked` witness for every
engine-side item; per-item revertable commits; no release/tag actions;
the 8 required checks stay green.

---

## B17 — Ranged-request encoding discipline + ranged-response invariants

**Origin:** study §4c (worker request builder forces `identity` coding on
ranged requests; a dedicated response validator detects
`multipart/byteranges`, `chunked`, and compressed ranged replies before any
byte is trusted). **Public-spec source:** RFC 9110 §12.5.3 (`identity`),
§8.4 (Content-Encoding), §14.1.2/§14.4 (Content-Range, Range, If-Range) —
NOT the reference binary.

**Target:** `vdm-core/src/engine/task.rs` (probe `ranged_get`, audit
`ranged_get`, worker ranged GET) + `http.rs` only if a shared helper is
cleaner; win7 tree port. **Goal:** eliminate the silent-corruption class
where a misbehaving server returns a compressed or malformed body for a
byte-range request and the engine writes it verbatim into the output file.

### Spec

1. **`Accept-Encoding: identity` on ranged requests.** Every request that
   carries a `Range` header (worker segment GET, probe range GET, audit
   range GET) carries `Accept-Encoding: identity` UNLESS the caller already
   supplied an `Accept-Encoding` header via `options.headers` (user intent
   wins; never emit two value headers for the same name).
2. **Ranged-response invariants.** For a response to a ranged request that
   will be written into the output file (the worker path), all of:
   - status is 206 (or the pre-existing 200-at-offset-0 rule);
   - NO `Content-Encoding` header (a coded body invalidates byte-offset
     arithmetic — fail the response, not the download);
   - `Content-Type` does not announce `multipart/byteranges` (the engine
     asks for exactly one range; a multipart reply is a server confusion —
     fail the response);
   - when status is 206, a parseable `Content-Range: bytes <s>-<e>/<t|*>`
     MUST be present with `<s>` equal to the requested start and `<e>`
     equal to the requested end (a short or shifted range is treated as an
     invalid response — fail-closed; we do not shrink leases in-flight).
3. **Failure semantics.** An invalid ranged response rides the worker's
   EXISTING bounded retry/mirror-rotation path with a distinct reason
   string (ours): e.g. `compressed ranged response`,
   `multipart ranged response`, `content-range mismatch` — it never
   corrupts the file, never widens the lease, and never triggers the B1
   audit-conflict machinery (the audit path treats invalid ranged
   responses as INCONCLUSIVE, not as a mismatch).
4. The probe path treats an invalid 206 as "no usable size from this
   response" (keeps its other size sources) — advisory only.
5. No `ResumeState` change; no RPC/option surface change.

### Tests owed

- Unit (http/task battery): identity-header presence on ranged GETs; user
  `Accept-Encoding` wins; the invariant matrix (compressed 206, multipart
  206, shifted/short/absent Content-Range, valid 206) each classified
  correctly; audit path = inconclusive on invalid responses (no conflict
  flag); probe path ignores an invalid 206's total.
- A/B: bench-perf.sh — expected byte-identical, no regression (guards are
  off the hot path: header add + one parse per response head).

---

## B18 — If-Range validator on in-run ranged requests (stale-resource fast path)

**Origin:** study §4c (an `If-Range:` header appender on the resume path +
`ETag:` parsing in the response validator). **Public-spec source:**
RFC 9110 §13.1.5 (If-Range) and §14.3 (ETag) — NOT the reference binary.

**Target:** `vdm-core/src/engine/task.rs` (probe result plumbing → worker
request builder; the 200-on-ranged classification). win7 port.
**Goal:** a compliant server tells us AUTHORITATIVELY whether the resource
changed since the probe, so a changed resource fails fast with an honest
reason instead of cycling MAX_RETRIES across every mirror, and a compliant
206 is known to graft the SAME bytes the probe saw.

### Spec

1. The engine already captures `ETag` (then `Last-Modified`) in
   `ProbeResult`. The in-memory download task carries the probe's validator
   (etag preferred, else last-modified, else none — our precedence).
2. Every worker ranged GET for a task whose probe captured a validator
   carries `If-Range: <validator>`. No validator → no header (RFC-correct;
   the response then simply must satisfy B17's invariants as today).
3. Classification upgrade for the ranged response: a 200 (full body) when
   the request had a validator and `pos > 0` is now a definitive
   "resource changed since start" signal → the worker returns a distinct
   failure (`resource changed since download started`) IMMEDIATELY — no
   mirror rotation, no retry churn (it cannot heal), no silent graft. The
   pre-existing fail-closed behavior (never write a 200 body at offset>0)
   is preserved; this item only removes the pointless retry loop and names
   the cause.
4. **Non-goal (recorded honestly):** cross-restart If-Range (persisting the
   validator) needs a `ResumeState` schema change — out of scope per the
   standing wave constraint; on restart the task resumes WITHOUT If-Range
   and stays protected by B17 invariants + the B1 audit as today.
5. No RPC/option surface change.

### Tests owed

- Unit: If-Range emitted iff a validator exists (etag precedence over
  last-modified); absent validator → no header; 200@pos>0 with validator →
  immediate distinct failure (no further mirror attempts asserted);
  206 responses continue unchanged.
- A/B: bench-perf.sh — byte-identical, no regression.

---

## B19 — Expired-link guidance (refreshable-failure classification)

**Origin:** study §4c (the reference detects expired/forbidden addresses
and walks an owner-page refresh flow with meta-refresh + Set-Cookie
http-equiv handling and robots `none/nofollow` compliance gating before
re-resolving). **Our adoption is the HONEST MINIMUM for wave 3:** classify
the refreshable failure class in the engine and surface actionable
guidance in the extension; the full owner-page re-scrape machinery is a
recorded non-goal (we do not re-scrape pages in-engine, and the
extension's existing page-context grab-links flow is the user's refresh
path today). Robots/meta-refresh compliance is the design constraint we
record for any future re-scrape feature, learned from the study.

**Target:** `vdm-core/src/engine/task.rs` (probe failure classification);
`extensions/chromium/popup.js` (+ shared text module if one exists) for the
hint; win7 port for the engine string. **Goal:** a user facing HTTP 403/410
gets an actionable next step instead of a bare status code.

### Spec

1. Engine: probe/worker failures with status 403 or 410 classify as
   refreshable — the surfaced reason string is ours:
   `HTTP 403 — link may be expired or forbidden; re-open the source page
   and re-grab the link (see the extension's grab-links action)` (410
   likewise). The string is a classification marker (a stable prefix the
   UI can match); no new RPC fields, no enum changes.
2. Extension (popup): when a task's error text starts with the stable
   marker prefix, render one additional static guidance line (our own
   wording) pointing at the page-context re-grab path. Never on other
   errors; never auto-triggered; no new permissions.
3. Non-goals (recorded): automated owner-page re-fetch, meta-refresh
   following, Set-Cookie http-equiv handling, robots-file/meta parsing.
   Any future re-scrape feature gets its own gate and carries the
   robots-compliance constraint from the study.

### Tests owed

- Unit: the classification matrix (403/410 → marker-prefixed string;
  404/5xx/timeouts → unchanged strings); marker prefix stability.
- Extension battery: popup renders the guidance line only for the marker
  prefix (fixture-based), never for other errors.

---

## Measurement plan

1. Engine (B17/B18): bench-perf.sh A/B (labels `s11i3-pre`/`s11i3-post`) —
   guards live off the hot path; expected byte-identical + no regression on
   the standard profiles; any regression = investigate before close.
2. B19: classification unit matrix + popup fixture test; no bench claim
   (behavior item — honest qualitative note only).
3. Every item: no regression in the existing batteries; the 8 required
   checks stay green; win7 port + 1.75 check for engine items; no
   release/tag actions.
