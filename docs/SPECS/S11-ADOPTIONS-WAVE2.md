# S11 adoption specs — wave 2 (B2-presets, B3, B4, B5, B6, B7, B8, B13, B16)

Behavior specs for the techniques adopted from the S11 reference-product
study (`docs/RESEARCH-IDM-STUDY.md` §5). **Provenance tier: T2** (static
reverse engineering of the reference build, 2026-11-21, Ghidra 12.2 /
binutils / direct JS read; fingerprint
`7346128f…d62ee`). **Clean-room barrier, restated per the standing S11
brief:** nothing in this file or its implementation is copied code, data
structures chosen for implementation reasons, resources, string tables,
or UI from the reference product. Every spec below is a functional
description in our own words; mechanism names, constants and thresholds
are chosen by us. Gate: `harness/ASSESSMENTS/0007-s11i-adoption-wave2.md`
(zero BLOCK; all conditions folded into this text). These specs are
committed BEFORE any implementing commit (spec-first barrier); the
incorporation review diffs the implementation against this document.
Scope source: owner decision round 1, D1:C (`harness/DECISIONS.md`).

**Hard constraint applying to every engine-side item:** no
`ResumeState` change — the Android `.vdmctl` reader is strict-schema.
`HttpOptions` additions are additive serde-default fields (the Android
reader mirrors `ResumeState`, not `HttpOptions`). B13 introduces a NEW
`.vdmq` file suffix whose schema we own.

---

## B2p — Link-class connection presets

**Origin:** study §4 "Dynamic connection scaling — presets" (T2).
**Target:** `vdm-core/src/engine/task.rs` (`HttpOptions`) + NMH add
path. **Goal:** one knob for link class instead of hand-tuning
`connections` per link.

### Behavior

- Additive field `HttpOptions.conn_preset: String`, serialized
  camelCase `connPreset`, default `"auto"`. Values: `auto`, `high`,
  `medium`, `low`.
- Mapping (ours): `high` → ceiling 8, `medium` → ceiling 4,
  `low` → ceiling 2, `auto` → today's behavior unchanged (the existing
  `connections` value and heuristics).
- A preset is a **ceiling, never a raise**: effective connections =
  `min(preset ceiling, existing size-based scale cap, absolute cap 32)`.
  The B2 optional-connection machinery (spawn/retire, extra ≤ 8) is
  untouched — presets only set the BASE ceiling.
- Validation at add-time (engine) and at the NMH boundary: any other
  string is rejected with our message ("unknown connection preset").
  The field is not persisted in `ResumeState` (options-time knob).
- Honest note surfaced in `docs/BUILDING.md`/RPC docs: high connection
  counts can antagonize servers (the reference product itself warns
  this) — presets default to `auto`.

### Tests owed

- Unit: mapping + ceiling interaction (preset lower than scale cap
  wins; preset never raises above the size-based cap); invalid preset
  rejection at both boundaries.
- No-regression bench at `auto` (identical to pre-change build).

---

## B3 — HLS `EXT-X-BYTERANGE` parsing and assembly

**Origin:** study §4 "Adaptive media — EXT-X-BYTERANGE" (T2).
**Target:** `vdm-core/src/engine/hls.rs`. **Goal:** correctness/range
coverage on playlists whose media is byte ranges of one resource.

### Behavior

- `Segment` gains `byterange: Option<(u64 len, u64 offset)>`.
- `parse_media` parses `#EXT-X-BYTERANGE:<n>[@<o>]` for the URI on the
  next non-comment line, per RFC 8216 §4.3.2.2: when `@o` is absent,
  the offset is the **end of the previous byte range applied to the same
  URI** within the playlist instance (the "no byterange line = reset to
  whole resource" rule included). A segment WITHOUT a byterange line,
  following byterange segments of the same URI, addresses the whole
  resource (offset 0).
- `fetch_segment` with a byterange issues a `Range:
  bytes=<offset>-<offset+len-1>` GET on the (resolved) segment URI via
  the existing `Conn`/`Request` machinery — no new client path; a
  non-206 response or short body fails the segment (existing retry
  semantics).
- BYTERANGE composes with the existing AES-128 path: decrypt the FETCHED
  ranged bytes (the key applies to the media bytes) with the segment's
  IV rules; assembly order is unchanged.
- Unknown total: a BYTERANGE playlist has no single Content-Length.
  **Pre-implementation verification owed:** confirm how the HLS path
  surfaces `total == 0` today (grep `snapshot`/progress in hls.rs); if
  the path asserts a known total, this item's non-goals record it and
  the fix is limited to the byterange-aware accounting.

### Non-goals (stated honestly)

- `EXT-X-MAP` / fMP4 playlists remain S7 scope; encountering a playlist
  that uses EXT-X-MAP **fails loudly** (our error, never a silent
  mis-assembly).
- SAMPLE-AES unchanged (S7).

### Tests owed

- Unit: explicit offsets; implicit-offset chain across same-URI
  segments; reset after a whole-resource segment; BYTERANGE + AES-128
  roundtrip.
- e2e (local test server): a BYTERANGE playlist (≥ 3 segments, mixed
  explicit/implicit offsets, one shared URI) assembles
  **byte-identical** to the resource bytes.
- win7 port + `cargo +1.75.0 check` witness.

---

## B4 — NM host resilience (persistent port + bounded reconnect)

**Origin:** study §3 "NM host link — persistent port, reconnect timer,
host-absence graceful degradation" (T2). **Target:**
`extensions/shared/messaging.js` (+ thin per-browser wiring).
**Goal:** stability of the browser↔core link across host restarts and
service-worker churn.

### Behavior (ours)

- New shared module (or section of `messaging.js`): a port wrapper over
  `chrome.runtime.connectNative('com.velocitydm.nmh')` with a request/
  reply correlation by a monotonically increasing id echoed by the host
  — wait: the NMH protocol is one-frame-per-command on stdio. Keep the
  existing one-shot `sendNativeMessage` shape as the transport for
  single commands; the wrapper adds: **persistent port preferred, one-shot
  fallback** (feature-detect; Opera/Edge parity, Firefox MV2 event
  pages use the one-shot path).
- Reconnect: on port disconnect, exponential backoff **500 ms → 8 s
  cap, doubling**, reset to 500 ms after 30 s of stable connection.
  Reconnect attempts stop after a 10-minute budget; the popup surfaces
  "engine unreachable" (our words) while in that state.
- Host-absence matching: our list of disconnect/lastError patterns
  ("not found", "not installed", "access is denied", "native host has
  exited") → graceful degradation: the extension keeps working, browser
  downloads resume normally (the existing takeover decline path stays
  correct — a test pins it).
- **Bounded command queue:** commands issued while disconnected are
  queued, cap **32, drop-oldest** (a console note records the drop);
  the first send failure re-queues its command exactly once. Never an
  unbounded retry loop.
- Host side (`velocitydm-nmh`): the stdio loop already handles exact
  framing with interrupt-retry; each port connection spawns a fresh host
  process (native-messaging semantics) — no host change required beyond
  keeping the loop tolerant of abrupt EOF (verify + test the
  clean-exit-on-EOF behavior).

### Tests owed

- Unit: backoff sequence math (500 ms … 8 s cap, reset rule); queue cap
  + drop-oldest; host-absence pattern matcher (positive/negative).
- Integration-style unit with a mock port: first send fails → command
  re-queued → delivered after reconnect.
- Battery: the decline path still resumes the browser download.

---

## B5 — Cookie-jar forwarding on every add path

**Origin:** study §3 "Cookie/session forwarding" (T2). **Target:**
`extensions/shared/messaging.js` + background/popup wiring.
**Goal:** authenticated downloads don't fail because only SOME add
paths forward cookies.

### Behavior (ours — the delta from today)

- Today only the takeover path (`handOffDownload`) forwards cookies.
  Wave 2: **every add path with an http(s) URL** forwards the cookie
  jar: context-menu link/image/video/magnet-adjacent adds, popup manual
  adds, batch adds (per-URL, capped at the first 32 URLs of a batch),
  and the takeover path (unchanged).
- Jar read: `cookies.getAll({url})` — **session AND persistent cookies
  (our choice: auth commonly rides session cookies; the reference's
  `session:false` is NOT adopted)** — serialized `name=value` joined
  with `; `; empty jar forwards an empty string (today's shape).
- Firefox: the shared module passes the URL only; unpartitioned
  first-party cookies are what the API returns there; partitioned
  (CHIPS) jars are an honest documented limitation this wave.
- Incognito: when the extension lacks incognito access
  (`isAllowedIncognitoAccess` false), incognito-context adds forward no
  cookies (the API returns nothing) — documented, not worked around.
- **Secrecy (gate condition):** cookie values never reach
  `console.log`, NMH stderr, or engine debug output — an explicit test
  asserts no cookie substring appears in any captured log channel.

### Tests owed

- Unit: serializer shape (matches today's takeover output);
  empty-jar tolerance.
- Battery: every add path builds a command carrying cookies (context
  menu, popup, batch cap 32, takeover).
- No-cookie-leak test (the secrecy condition).

---

## B6 — Takeover UX (shelf, filename participation, in-tab confirmation)

**Origin:** study §3 "Downloads pipeline — shelf suppression,
onDeterminingFilename, cancel-then-resume, captured.html" (T2).
**Target:** `extensions/chromium/background.js` + a new web-accessible
page + shared wiring. **Goal:** UX clarity, fewer double-downloads,
no dead browser downloads.

### Behavior (ours)

- **Shelf suppression** (Chromium-only, feature-detected; Firefox
  no-ops): while takeover is enabled in settings, call
  `chrome.downloads.setShelfEnabled(false)`; when takeover is disabled
  (settings change), `setShelfEnabled(true)`. Trade-off documented:
  the shelf is off for ALL downloads while takeover is on (the
  reference's behavior; accepted at this scope).
- **Filename participation:** subscribe to
  `downloads.onDeterminingFilename`; when `shouldTakeOver` says yes,
  suggest our filename (`DownloadItem.suggest`) so the browser item
  matches the engine's naming — never fighting a user rename (we only
  suggest once, on our takeover).
- **Decline path preserved:** when the host declines/fails, the browser
  download resumes (today's correct behavior) — now with the reason
  surfaced to the popup's error note (our static words).
- **In-tab confirmation (`taken.html`):** a static, self-contained
  web-accessible page (no remote resources; narrowly-scoped
  `web_accessible_resources`). The takeover flow navigates the
  CURRENT tab to `taken.html#<encodeURIComponent(url)>` ONLY on
  content-script video takeovers (not on the downloads-pipeline path);
  the page reads the hash ONCE, renders the URL via **textContent
  only** (never `innerHTML`/`location` assignment from the fragment),
  shows our confirmation text, then calls `history.back()` immediately
  (no history spam, no navigation side effects).

### Tests owed

- Battery: decline → resume (pinned); suggest-filename only on takeover;
  hash rendering via textContent (an XSS-attempt fixture in the hash
  renders inert).
- Manual-verification note owed (owner, at the D5:A gate): shelf
  behavior + confirmation page on a real browser.

---

## B7 — Content-script video detection hardening

**Origin:** study §3 "Page-side detection — geometry gating, overlay
detection, observers, inline-script scanning, site packs" (T2).
**Target:** `extensions/shared/detector.js` + content script.
**Goal:** fewer false positives with a bounded footprint.

### Behavior (ours — thresholds chosen by us)

- DOM detection pass (triggered on demand: popup open, context-menu
  query, media play events) collects `<video>/<audio>` candidates:
  - **Geometry gate:** element is `document.contains`-attached AND
    CSS footprint ≥ **200×120 px** (ours; the reference's 15×10
    exists-gate is NOT adopted) for the popup list;
    a prominence sub-tier (≥ 640×360) feeds the "large video" hint.
  - **State gate:** `readyState ≥ 2`, or (`videoWidth > 0` &&
    `networkState === 2` while `readyState < 1`).
  - **Overlay filter:** computed effective visibility — an element (or
    an ancestor overlay) with fully transparent background covering it
    and no painted content is invisible → excluded. Implemented as:
    `getBoundingClientRect` area ≥ gate AND not clipped to zero AND
    `visibility !== 'hidden'`/`display !== 'none'` on the chain.
- **Observers (bounded):** one `MutationObserver` (childList+subtree
  filtered to media-related tags, debounced **250 ms**) + a
  `ResizeObserver` on current candidates only; active ONLY during a
  detection window; **full teardown** (disconnect) when the window
  closes.
- **Inline-script scan:** for `<script>` bodies, first **128 KiB** each,
  a URL regex for `http(s)://…\.(mp4|webm|m3u8|mpd|m4s|ts)[^a-z]` —
  matches become candidates only if the geometry gate passes on the
  media element that plays them.
- **Site packs (data-driven, off by default):** a pack = `{selectors:
  string[], urlPatterns: string[]}` loaded from settings; none ship
  enabled. No site-specific code paths in the module.

### Tests owed

- Unit (jsdom-style in the node battery): geometry/state gating table;
  overlay exclusion; inline-scan capped size; pack loading off-by-
  default.
- Overhead demonstration: a synthetic heavy DOM (≈5k nodes) pass
  completes under a stated budget in the node battery (the honest
  measured number lands in the close entry, not invented here).
- Observer teardown asserted (no leak across two windows).

---

## B8 — MIME→extension normalization table

**Origin:** study §3 "MIME→extension normalization table" (T2 — the
reference's ~100-entry table is NOT copied; ours is compiled from the
public IANA media-type registry). **Target:** `vdm-core/src/http.rs`
(or new `mime.rs`) + generated JS copy + `scripts/gen-mime-table.py`.
**Goal:** consistent filename choices when the URL has no usable
extension.

### Behavior

- A Rust const table (~100 entries, ours from IANA): MIME type →
  preferred extension (`video/mp2t` → `ts`, `application/vnd.apple.
  mpegurl` → `m3u8`, `audio/mp4` → `m4a`, …). Multiple MIME aliases may
  map to one extension. Parameters (`; charset=…`) and case are
  normalized away.
- Applied ONLY when: the URL path yields no extension (existing
  `extOf`-equivalent in the engine) AND a Content-Type is known —
  the suggestion NEVER overrides an explicit user filename or an
  existing extension.
- Engine side: filename suggestion at task creation (the name the
  snapshot shows); extension side: the sniffed-video list + the
  `onDeterminingFilename` suggestion use the generated JS copy.
- **Single source (gate condition):** the Rust table is the source of
  truth; `scripts/gen-mime-table.py` emits
  `extensions/shared/mime-table.js`; a drift test regenerates and
  compares with the checked-in copy (the `md3-tokens-baseline.json`
  precedent).

### Tests owed

- Unit: lookup with parameters/case; no-override rule; unknown MIME →
  no suggestion.
- Drift tripwire test (generated == checked-in).
- win7 port + 1.75 witness.

---

## B13 — Persisted scheduler rules (`.vdmq`)

**Origin:** study §4 "Queue/scheduler — per-queue persisted scheduler
state" (T2). **Target:** `vdm-core/src/engine/scheduler.rs` + state
loading at daemon start. **Goal:** scheduler RULES survive a daemon
restart (the honest gap: today `max_active`, stop windows, and the
drained-actions are daemon-memory only).

### Behavior

- (Amendment 2026-11-22, post-CI: the OS-temp DEFAULT download dir is
  volatile scratch shared by unrelated engine instances — the store is
  skipped there both ways; a configured dir persists + loads normally.
  Found live by the e2e-cross suite: a stop-window saved by one daemon
  leaked into a second daemon's startup.)
- New file `<out_dir>/.vdm/scheduler.vdmq` — JSON, schema we own:
  `{version: 1, max_active, windows: [[start,end]…],
  on_queue_drained, exit_when_drained}`. Atomic write (temp file +
  rename, same directory).
- Saved on every rule change (the RPC `queue.set` path) and at clean
  shutdown; **loaded at daemon start** (unknown keys or a version
  mismatch ⇒ defaults, our log line on stderr; never fails startup).
- **Honest non-goals:** task auto-recovery (there is no `.vdmctl`
  rescan today — tasks are re-added by the UI; adding a rescan is S8-
  era state work, recorded, not smuggled in here); named multi-queues
  (our scheduler is single-queue; the reference's multi-queue is a
  feature addition, parked); `ResumeState` untouched (different file
  suffix, we own the schema — the Android reader never sees `.vdmq`).

### Tests owed

- Unit: serialize/roundtrip; unknown-key tolerance; atomic rename
  semantics (no partial file observable).
- Witness: rule change → process restart → rules intact (driven at the
  library level in the engine battery).
- win7 port + 1.75 witness.

---

## B16 — Browser-proxy propagation (report + suggest + warn)

**Origin:** study §4 "Browser proxy parity" + §3 permission posture
(T2). **Target:** `extensions/shared/*` + `crates/velocitydm-nmh` add
passthrough + `vdm-core/src/proxy.rs` consumer. **Goal:** engine
requests ride the browser's effective proxy instead of failing against
VPN-extension setups.

### Behavior

- **Report:** when the `proxy` permission exists and the API is
  available (`chrome.proxy.settings.get`), the extension resolves the
  effective proxy: `mode: system` → report `"system"` (no explicit
  endpoint); `fixed_servers`/`single_proxy` → report
  `{mode: "fixed", scheme: "http"|"https"|"socks5", host, port}`;
  other modes → report the mode name only. Absent permission or a
  thrown API ⇒ skip silently (best-effort; the add proceeds) — the
  manifest delta (adding `proxy`) lands with this item's commit only,
  documented with its review cost.
- **Transport:** additive add-command field `browserProxy`; the NMH add
  path validates the shape (scheme ∈ {http, https, socks5}, host ≤ 253
  chars printable, port 1–65535) and forwards it as an
  `HttpOptions.browser_proxy` (additive, camelCase `browserProxy`).
- **Use (suggest, never override):** at task creation, if the task has
  NO explicit `proxy`, the engine adopts the reported proxy as the
  task proxy and **probes reachability** (one TCP connect, 2 s
  timeout): reachable → use; unreachable → drop it with the warning.
- **Warn (visible, never silent):** (a) task proxy was explicitly set
  AND differs from the browser proxy → warning string on the add
  reply + snapshot `notice` (the wave-1 field): "task proxy differs
  from the browser's proxy; downloads may not follow the browser's
  route"; (b) reported proxy is a loopback address that failed the
  probe → "the browser's proxy is not reachable from the engine; a
  VPN-extension proxy is likely the cause". Static strings, ours.

### Tests owed

- Unit: spec resolution (system/fixed/unavailable); NMH shape
  validation (bad scheme/host/port rejected); use/suggest precedence
  (explicit task proxy wins + warns); probe-failure drop + warning.
- Battery: report path with a mock settings getter.
- No-regression bench (default: no proxy present → byte-identical
  behavior).

---

## Measurement plan (A/B evidence owed per gate 0007)

1. Engine-side items (B3, B13, B2p, B8-core, B16-core): standard
   `bench-perf.sh` A/B (labels `s11i2-pre`/`-post`) — no regression;
   each item's specific witness is its tests-owed section (byte-identical
   assembly, rules-survive-restart, mapping tables, warn paths).
2. Extension-side items (B4, B5, B6, B7, B8-js): the node battery grows
   the tests above; wins are behavior demonstrations + honest
   qualitative notes — no invented performance numbers.
3. Deltas and witnesses land in the MEMORY close entry per item; the
   study §5 rows get adoption-status notes as items close.
