# Assessment 0007 — S11-i adoption wave 2 (gate record)

**Date:** 2026-11-21 (UTC). **Change under review:** the S11-i wave-2
activation per the owner's decision round 1 (`harness/DECISIONS.md`,
D1:**C**): adopting **B2-presets** (link-class connection presets, the
half deferred by gate 0006), **B3** (HLS `EXT-X-BYTERANGE`), **B4**
(NM-host resilience), **B5** (cookie-jar forwarding), **B6** (takeover
UX: shelf suppression + filename participation + in-tab confirmation),
**B7** (content-script video detection hardening), **B8** (MIME→extension
normalization table), and the two previously-parked candidates the owner
pulled into scope, **B13** (persisted scheduler state) and **B16**
(browser-proxy propagation). All are T2-provenance findings (static RE of
the reference product, `docs/RESEARCH-IDM-STUDY.md` §5) and carry the
standing stricter-spec-review bar: behavior specs in our own words,
committed BEFORE the implementing commit
(`docs/SPECS/S11-ADOPTIONS-WAVE2.md`), clean-room barrier restated there.
**Panel:** five-role per PROTOCOL.md (ATKR / FALS / REQ / INT / REL),
dispatched independently with the same evidence pack (the study backlog
+ the drafted wave-2 specs + repository access + the decision round
record). **Scope note:** D2 (parallel S4 activation) is a SEPARATE gate
(0008); this record covers wave 2 only.

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | B6's in-tab confirmation page is a new web-accessible resource — the URL travels in a hash fragment and MUST be rendered via `textContent` only (never `innerHTML`/`location` assignment from the fragment); page CSP/hash pinning for the page itself; `web_accessible_resources` must scope the page narrowly. B5 cookie values are secrets: they ride the existing NMH→engine channel and MUST NOT reach logs (`console.log`, NMH stderr, engine debug env-gates) — add an explicit no-logging test. B16's `proxy` manifest permission is a sensitive-permission addition (store review friction + user-visible warning): spec must state the cost and keep reporting best-effort (absent permission ⇒ skip, never fail the add). B4 reconnect must not command-spray: commands queued while disconnected stay bounded (cap) and are dropped-oldest, never retried unboundedly. B7's inline-script scanning and observers run in the PAGE context — budget caps are mandatory (debounce, observer teardown, scan size limit) or the content script becomes the resource hog the study is trying to avoid. B2-presets must flow through the EXISTING cap chain (absolute 32, size-based scale, extra ≤ 8) — a preset may only lower the ceiling, never raise it. |
| FALS | **PROCEED-WITH-CONDITIONS** | Every value claim must be demonstrated, not asserted: B3 needs a byte-identical assembly test for a BYTERANGE playlist INCLUDING the implicit-offset chain (RFC 8216 §4.3.2.2) and an explicit non-goal line for `EXT-X-MAP`/fMP4 (S7's scope — silence there would mis-assemble, not fail); B4 needs a reconnect-path test where the first send fails and the queued command lands after reconnect (else the persistent port is untested plumbing); B5 needs a test that cookies actually reach the built add command on EVERY add path that has page context (context-menu and popup adds currently do NOT forward cookies — the gap the spec claims to close); B6 must show the decline path still resumes the browser download (today's correct behavior preserved under the new flow) and the confirmation page auto-backs without leaving history spam; B7 must be measured for content-script overhead on a heavy DOM (bounded observer budget demonstrated in a test with a synthetic large page); B8's table needs a drift tripwire between the Rust and JS copies (single source + generated copy + test, per the `md3-tokens-baseline.json` precedent) or the two copies WILL diverge; B13 must state the honest delta — tasks are NOT auto-recovered on daemon restart today (no `.vdmctl` rescan exists), so the cheap, true claim is RULES persistence (windows/max_active survive restarts), not "crash recovery" writ large; B16 must show the warn path (browser proxy present + task proxy differs ⇒ visible warning, not a silent override). No invented performance numbers for UX items: wins are behavior tests + honest qualitative notes, engine items get the bench A/B. |
| REQ | **PROCEED-WITH-CONDITIONS** | spec-first barrier honored only by the commit split (this gate + specs + ROADMAP/DECISIONS/MEMORY FIRST, implementation commits after — wave 1's pattern); every spec restates T2 provenance + no-verbatim-copy barrier (header + per-item origin lines); B10/B11/B12/B14/B15 stay parked with the observe-first note (B12 note: loopback trust rules already carry the equivalent boundary; B14 explicitly folds to S7); N1–N6 stay rejected; no `ResumeState` change for ANY item — B13 uses a NEW `.vdmq` file suffix we own, HttpOptions additions are additive serde-default fields (the Android strict-schema reader mirrors ResumeState, not HttpOptions — verified against `engine/state.rs` and the KMP `.vdmctl` schema); win7 tree ports owed for every core-side item (B3, B13 rules load, B8 core table if core-side) with the local cargo +1.75.0 witness; extension battery (`extensions/tests/run-tests.mjs`, node-run, wired into the required Extension-tests check) grows for B4–B8/B16 with the new modules under test; per-item implementation commits (revertability), not one 9-item blob; no release/tag actions; the 8 required checks + the non-required frontend jobs stay green; adoption-status notes land in the study §5 rows as each item closes. |
| INT | **PROCEED-WITH-CONDITIONS** | MV3 realities: `chrome.runtime.connectNative` persistent ports keep service workers alive on current Chrome (documented since ~116) but the spec must keep the one-shot `sendNativeMessage` fallback for browsers where the port dies (Opera/Edge parity) and for MV2 Firefox (its event-page model differs — build-ext.sh builds firefox from the same shared tree, so B4/B5 logic must stay in `extensions/shared/`, browser wiring thin); shelf APIs (`setShelfEnabled`) are Chromium-only — Firefox build must no-op gracefully (shared module feature-detects); `downloads.onDeterminingFilename` requires the `downloads` permission we already hold (verify no NEW permission beyond B16's `proxy`); B3's ranged segment GETs must reuse the existing `Conn`/`Request` machinery (no new client); BYTERANGE playlists have no `Content-Length` total — progress surfaces (snapshot `done`/`total`) must tolerate total=0 as ALREADY possible on the HLS path (verify by grep before assuming; if total is currently asserted non-zero, the spec's non-goal list must say so); B8 table lives in `vdm-core` (Rust, engine-side filename suggestion) with a GENERATED JS copy for the extension suggestion path — the generator script joins the toolbox (scripts/); B13 atomic write = temp+rename on the SAME filesystem; B2-presets validated at add-time (engine) AND at the NMH boundary (camelCase `connPreset`), additive `HttpOptions` field rides the existing serde defaults — both trees. |
| REL | **PROCEED-WITH-CONDITIONS** | one activation docs commit (this gate + specs + ROADMAP + DECISIONS + MEMORY) pushed FIRST, then per-item implementation commits — each `git revert`able alone; A/B evidence recorded in MEMORY close entries per engine-side item (B3: byte-identical + no-regression on the standard bench; B2-presets: knob behavior + no-regression bench; B13: rules-survive-restart witness; extension items: battery + honest qualitative notes — the standing rule: measured wins or honest behavior tests, never invented numbers); CI witness before ANY wave-2 close: all 8 required checks green at the pushed HEAD (observe the non-required frontend + android jobs; a red non-required job blocks ITS OWN item's close, not the whole wave); failure policy: fix-forward or revert the single item's commit, never close with red main; backup closes the activation plan (Law 1) with zero dirty files; no tag cuts (v-ruleset active); the six frozen releases stay frozen until the promotion gates per D5:A. |

## Consensus

Zero `BLOCK` → **the gate closes with this record.** All conditions were
folded into the wave-2 specs (`docs/SPECS/S11-ADOPTIONS-WAVE2.md`) and
the execution plan below before any implementation commit; the
condition→plan mapping is itemized.

## Conditions and how each was folded

1. **Confirmation-page injection safety** (ATKR-1) → B6 spec §page:
   hash fragment read once, rendered with `textContent`, never assigned
   to `location`/`innerHTML`; page is static (no remote resources),
   narrowly-scoped `web_accessible_resources`.
2. **Cookie secrecy** (ATKR-2) → B5 spec §secrecy: cookies never logged
   in extension, NMH stderr, or engine debug output; explicit test.
3. **Proxy permission honesty** (ATKR-3, INT) → B16 spec §availability:
   permission absent/API throwing ⇒ best-effort skip (add proceeds);
   manifest delta documented with its review cost.
4. **Bounded reconnect queue** (ATKR-4) → B4 spec §queueing: cap 32,
   drop-oldest, no unbounded retry; first-send failure re-queues once.
5. **Content-script budget** (ATKR-5, FALS) → B7 spec §budget:
   MutationObserver/ResizeObserver debounced 250 ms, active only while
   detection is on; inline-script scan capped at 128 KiB per script;
   full teardown on disconnect; overhead demonstrated on a synthetic
   heavy page.
6. **Presets under the cap chain** (ATKR-6) → B2-presets spec §caps:
   preset only LOWERS the connection ceiling; auto = today's behavior;
   absolute/size/extra caps unchanged and re-tested.
7. **Falsifiable demonstrations** (FALS) → tests-owed sections in
   every spec (B3 implicit-offset chain + byte-identical; B4
   fail-then-reconnect delivery; B5 all-add-paths cookie reach; B6
   decline-resume preserved + auto-back without history spam; B7
   bounded overhead; B8 drift tripwire; B13 rules-survive-restart;
   B16 warn-not-override).
8. **No ResumeState change** (REQ-3) → hard constraint in every
   engine-side spec; B13 uses the new `.vdmq` suffix.
9. **win7 ports + local 1.75 witness** (REQ-4) → per-item port notes;
   `cargo +1.75.0 check` in `win7/` before each core-side push.
10. **Battery growth wired into the required check** (REQ-5) → new
    shared-module tests land in `extensions/tests/run-tests.mjs` (the
    Extension-tests required context runs it).
11. **Per-item revertability** (REQ-6, REL-1) → implementation lands as
    one commit per backlog item (engine+tests+win7 port together).
12. **MV3/MV2 parity via shared modules** (INT-1/2) → all B4/B5/B6/B7/
    B16 logic lives in `extensions/shared/`; per-browser wiring stays
    thin; shelf API feature-detected, Firefox no-ops it.
13. **No new permissions beyond B16's `proxy`** (INT-3) → verified
    against the current manifest (downloads, webRequest,
    nativeMessaging, storage, contextMenus, cookies, tabs +
    `<all_urls>` hosts); only B16 adds `proxy`.
14. **Ranged GETs reuse `Conn`/`Request`** (INT-4) → B3 spec §fetch:
   no new HTTP client path; 206 validation mandatory.
15. **Progress-total honesty** (INT-5) → B3 spec §surfaces: verify the
    existing unknown-total handling on the HLS path before
    implementing; if `total` is currently assumed >0, the spec's
    non-goals record it and the item fixes only its own assumption.
16. **B8 single-source generation** (INT-6, FALS) → Rust table is the
    source; `scripts/gen-mime-table.py` emits the JS copy; a drift
    test compares generated output to the checked-in copy.
17. **B13 atomic write + honest scope** (INT-7, FALS) → temp+rename;
    scope = scheduler rules persistence (the TRUE gap), task
    auto-recovery stays a stated non-goal (no rescan exists today).
18. **CI witness + clean backup close** (REL) → poll-checks.sh at every
    close; zero dirty files at backup; no tags; frozen releases stay
    frozen until D5:A's promotion gates.

## Recorded (not fixed this session)

- Implementation sequencing (post-activation): engine-side first
  (B3 → B13 → B2-presets → B8-core), then extension-side (B4 → B5 → B6
  → B7 → B16 wiring), each item closing with its own battery+A/B
  evidence before the next; B16's manifest permission lands with B16's
  commit only.
- B10/B11/B12/B14/B15 remain parked (B14 explicitly belongs to S7).
- The S4 DHT activation runs as gate 0008 in parallel (owner D2:B).
- The IDMan.exe bounded deep-dive (owner D7:B) is commissioned and
  runs as a multi-session Ghidra effort alongside; findings enter the
  study doc + backlog the same way T2's did.
