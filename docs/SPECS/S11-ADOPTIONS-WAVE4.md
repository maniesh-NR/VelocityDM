# S11-i adoption wave 4 — behavior specs (B20, B21, B22)

Gate record: `harness/ASSESSMENTS/0011-s11i-adoption-wave4.md` (five-role,
5× PROCEED-WITH-CONDITIONS, zero BLOCK; conditions folded here).
Provenance: the study (T2, §4d) pointed at our gaps; every behavior below
is re-spec'd from PUBLIC specifications (RFC 9110 §5 field grammar,
§10.2.3 Retry-After, §15.5 status semantics) and our own state model —
never from any binary artifact. No ResumeState schema change; no new
permissions; no RPC schema change (additive settings only); no release or
tag actions.

## B20 — Outbound header hygiene (engine, both trees)

Observed gap (study §4d): header values deserve a charset discipline at
the value level, in addition to the emission-boundary refusal. Our current
emission boundary (crates/vdm-core/src/http.rs `push_header`) hard-refuses
CR/LF/NUL in names and values and space/colon in names; header NAMES are
NOT otherwise token-validated.

Rules (public-spec source: RFC 9110 §5.5 field-value grammar, §5.6.2
token grammar):

1. **Header NAMES gain the tchar hard refusal.** A name containing any
   byte outside the tchar set (`!#$%&'*+-.^_`|~`, digits, ASCII letters)
   is rejected with the existing illegal-header error. This is a
   TIGHTENING: names that passed before (control chars, DEL, non-ASCII)
   now fail fast. Request targets and Host keep their existing refusal.
2. **CR/LF/NUL in header VALUES keep the hard refusal, unchanged.** The
   injection boundary does not move: the request-splitting byte sequence
   never reaches the wire (fail-closed, not neutralized).
3. **Other control characters in VALUES (C0 controls except CR/LF/HTAB,
   plus DEL 0x7F) are replaced with `_`** before emission — a malformed
   caller value (e.g. a cookie jar scraping artifact) degrades to a
   sanitized request instead of aborting the download. SP and HTAB stay
   legal (RFC 9110 field-content).
4. **Non-ASCII bytes (obs-text, ≥ 0x80) PASS THROUGH unchanged.** RFC
   9110 permits obs-text in field values; UTF-8 cookie/referer values
   forwarded by the B5 cookie jar work today and must keep working.
   (This is the panel's explicit narrowing: a full byte whitelist would
   be a regression against working requests.)
5. **Substitution is observable, never silent:** a debug-level log line
   (no header value content) records that a value was sanitized; when
   the sanitized header came from caller options, the task snapshot's
   `notice` hint mentions that an option header value was adjusted.
   Cookie values never reach any log channel (B5 contract unchanged);
   refusal messages keep `{:?}` escaping.
6. **All emission paths route through one helper:** the `Proxy-
   Authorization` lines emitted in absolute-form proxying (http.rs) and
   the proxy CONNECT request (proxy.rs) use the same name/value checks
   (their base64 payloads are unaffected in practice; the point is no
   bypass). Engine-generated headers (`Range`, `Accept-Encoding:
   identity`, `If-Range`) are ASCII by construction and must pass
   through unaffected — pinned by test.
7. **Windows 7 tree ports the same helper and tests** (1.75-compatible
   syntax; no new crates).

Falsification matrix (unit tests, tests/http_engine.rs + http.rs):
- name `X-Foo` → OK; name `X\tFoo`, `X-Fooé`, `X\x01` → Err (new tchar
  refusal); name `Bad Name` → Err (existing).
- value `"a\rb"`, `"a\nb"`, `"a\0b"` → Err (unchanged refusal; request
  never emitted).
- value `"a\x01b"` → emitted `"a_b"`, request succeeds, debug log + notice
  path exercised; value `"café"` (obs-text) → passes through unchanged;
  value `"a\tb"` → passes through unchanged.
- engine `Range: bytes=0-99` + `Accept-Encoding: identity` round-trip
  byte-identical with hygiene active.
- proxy basic-auth emission paths still correct (existing proxy tests).

## B21 — Resume honesty: no-resume state + partial-state coherency
### (engine, both trees)

Observed gap (study §4d): a server that does not support resume after an
interruption deserves an explicit user-facing state instead of a silent
restart. Panel findings upgraded this item: today the engine silently
truncates the `.part` and LEAVES a validator-matching `.vdmctl` sidecar in
place — a later resume against a range-capable server would skip ranges
whose bytes were truncated (silent zero-holes corruption risk).

Rules:

7. **No-resume classification with precedence.** At the task start branch
   (`probe.size == 0 || !probe.accepts_ranges` → single-stream path), when
   partial download state existed (non-empty `.part` or a loadable
   `.vdmctl` for this output) and the task is not fresh, the task fails
   classification to the user-visible error channel with the marker
   `server does not support resume` and the snapshot `notice` hint
   "download restarted from zero because the server does not support
   partial requests". A server that NEVER advertised ranges (probe-time
   knowledge) is classified no-resume — NOT B18's "resource changed"
   (which applies only when a range-capable probe is followed by a
   200-at-offset on the ranged path). The message is generic: it never
   embeds the URL, query, or path.
8. **Rendering honesty (panel-corrected).** The user-visible string rides
   the EXISTING task `error` classification (the B19 marker pattern) —
   the snapshot `notice` is the additive hint. Rendering truth: the CLI
   prints both; win32/slint/macos render `error` (macos decode of the
   snapshot drops `notice` today); the browser popup renders neither
   channel. No new frontend surface is built by this wave; the stale
   "every frontend renders notes" comment is corrected while the file
   is open. No RPC schema change (both fields exist).
9. **Partial-state coherency rule.** When the single-stream path
   truncates the `.part` file for writing, it also invalidates
   (removes) the matching `.vdmctl` sidecar for that output. The
   sidecar is strict-schema and is only ever re-created by the
   segmented path; the Android reader is untouched (no schema change).
10. **Falsification (e2e, deterministic):** testserver variant serving
    `Accept-Ranges: none` (no 206 ever); pre-seeded `.part` + matching
    `.vdmctl`; the run completes single-stream and the final file's
    SHA-256 equals the server content, the stale sidecar no longer
    loads, and the failure/notice classification fired exactly when
    valid state was discarded.
11. **Windows 7 tree ports the same classification + coherency.**

### B21b — Bounded Retry-After (engine, both trees)

Rules (public-spec source: RFC 9110 §10.2.3; delta-seconds form only):

12. **`parse_retry_after(value) -> Option<Duration>`** accepts exactly
    `^\d{1,6}$` delta-seconds; the HTTP-date form and anything invalid
    yield None (NO date-parsing dependency is added — lock parity both
    trees).
13. **`retry_delay_for(status, retry_after, attempt) -> Duration`**:
    on HTTP 429 or 503 with a parsed delta, the wait is
    `min(delta, 60 s)`; without a delta (or any other status) the
    existing per-ladder behavior is byte-identical to today (worker
    250·2ⁿ capped 4 s; probe/run_single 400 ms·(n+1)). The 60 s cap
    bounds the worst case at 60 s × ≤32 workers.
14. **Structural status threading, not string matching.** The worker's
    non-206/200 branch and the probe / run_single ladders receive the
    response status code and Retry-After value structurally from the
    `Response` (today's `is_retryable` string matching stays for
    transport-level errors and gains an explicit 429/503 retryable
    classification via the structural path). This resolves the panel-
    identified inconsistency: `run_single`'s formatted `"server
    returned {}"` string never matched `"HTTP 5"` — run_single now
    retries 429/503 (bounded attempts, same cap as the existing
    ladder's attempt count) and 5xx, matching the probe ladder's
    semantics, instead of failing on first 5xx.
15. **Stop-responsive waits for Retry-After only:** the bounded RA wait
    ticks in short slices re-checking the task's stop/pause flags;
    the existing short backoff sleeps stay as-is (bounded ≤ 4 s).
    Workers add their existing per-worker jitter to RA waits so a
    fleet of workers does not reconnect in one synchronized burst.
16. **Falsification (unit, deterministic, no wall-clock CI):** parse
    matrix (`"2"` → 2 s; `"0"` → 0 s; `"Mon, 02 Jan 2006 15:04:05 GMT"`
    → None; `"abc"`/`""`/overflow → None); delay matrix (429+RA 2 → 2 s;
    503+RA 120 → 60 s; 503 absent → existing ladder value; 404 → ladder).
    The e2e testserver grows a 503+`Retry-After: 2` route exercised by a
    Linux-gated integration test (repo precedent) asserting the retry
    happened, not exact wall-clock timing.
17. **Windows 7 tree ports the same helpers and tests.**

## B22 — TLS minimum-version policy (additive settings knob)

Observed gap (study §4d): protocol-version policy is a first-class,
externally configurable setting of the reference transport. Ours is
hardcoded safe defaults.

Rules (public-spec source: the rustls public API; no new crates — both
trees pin rustls 0.23.46, which supports only TLS 1.2/1.3, so the knob is
structurally floor-raising):

18. **Knob:** `tlsMin` on the engine settings surface — accepted values
    `default` | `tls12` | `tls13`; absent/`default` = today's
    `with_safe_default_protocol_versions()` behavior, zero change.
    Lowering below the rustls floor is unimplementable by construction.
19. **Wiring (additive, tolerant):** EngineConfig gains a serde-default
    field → TaskEnv/ClientCfg → the single TLS site (`tls::wrap`).
    Invalid values in config.json follow the tolerant-load pattern:
    default + warn, never fail startup. Exposed via the existing
    `settings.get`/`settings.set` (validated set path rejects unknown
    values). No env var (repo precedent: settings, not environment).
20. **Fail-closed at the TLS boundary:** an invalid value arriving at
    `wrap()` refuses the TLS connection rather than falling back to a
    weaker policy.
21. **Selection helper + unit tests:** a pure helper maps the knob to
    the rustls `SupportedProtocolVersion` list; unit tests assert the
    built config's `protocol_versions` per knob value (public rustls
    field — no server needed). The wire-level claim (a TLS 1.1-only
    server is refused at tls12) is DOCS-ONLY honesty: the sandbox has
    no TLS test server; the spec and bench record must not claim more.
22. **Docs:** docs/BUILDING.md gains the `tlsMin` row (connPreset
    precedent). Windows 7 tree ports the helper — tls.rs's first win7
    delta; both `Cargo.lock`s unchanged.

## B23 — Keep-alive reuse witness (verification only — no code delta)

The engine already emits `Connection: keep-alive` by default, recycles
conns across worker leases, and `keepalive_reuse_across_leases` pins the
reuse count. The witness for this wave: re-run that test plus the bench
A/B pair; confirm the recycled-conn stale retry and the drain-failure
path (`read_body` before `into_conn`) cannot recycle a partially-drained
connection. Recorded in the bench record + MEMORY; no behavior spec, no
backlog-adoption claim (it is parity confirmation, not new work).

## B24 — CocCoc NMH registration (DEFERRED — recorded, not adopted)

The reference registers its NMH under the CocCoc browser's
NativeMessagingHosts directory too (study §4d). Our packaging registers
Chrome/Chromium/Edge/Opera/Vivaldi/Brave (nmh install script) and
Chrome/Chromium/Edge/Opera (linux install script, which also has an
uninstall-list asymmetry against vivaldi/brave). **Deferred:** the exact
CocCoc per-OS config directory names could not be empirically verified in
this sandbox (the AUR PKGBUILD is bot-blocked; the official deb endpoint
returned 404); an unverified path inside an existence-gated install loop
is a silent no-op — the panel's own honesty condition. Pre-condition for
any future adoption: observe a real CocCoc install's
NativeMessagingHosts paths (Linux user dir; Windows registry hive) and
fix the linux uninstall asymmetry in the same commit. Recorded in study
§5 so it is neither adopted nor silently dropped.

## Non-goals (this wave)

- No ResumeState schema change (rule 9 uses removal only).
- No new crate dependencies (rules 12, 18).
- No new permissions, no manifest changes, no extension JS changes.
- No release/tag actions (v-tag ruleset active; push-main-only).
- No FTP stage, no request-builder redesign, no hot-reload (recorded as
  study §5 rows / §4d pointers instead).
