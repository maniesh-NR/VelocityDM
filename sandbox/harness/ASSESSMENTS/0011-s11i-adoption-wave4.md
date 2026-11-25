# Assessment 0011 — S11-i adoption wave 4 (gate record)

**Date:** 2026-11-24 (UTC). **Owner directive under review** (verbatim,
recorded as ledger round 4): "proceed down the remaining adoption backlog
with a fresh gate". **Change under review:** S11-i wave-4 adoption from the
remaining study backlog (§4d session-16 observations + the parked
candidates), executed under the standing five-role gate + spec-first
barrier. Proposed scope presented to the panel: **B20** (outbound header
hygiene), **B21** (no-resume honesty + bounded Retry-After), **B22** (TLS
minimum-version policy knob), **B23** (keep-alive reuse witness), **B24**
(CocCoc NMH registration), with the parked items B10–B15 and the §4d
unmapped observations (request-builder shape, FTP PASV machine, config
hot-reload/helper launch, legacy RTMP family) up for pull-in decisions.

**Panel:** five-role per PROTOCOL.md (ATKR / FALS / REQ / INT / REL),
dispatched independently with the identical evidence pack (study §4d/§5 +
current engine facts with file:line + the proposed scope + repository
access + the standing barriers). Agents saw no peer verdicts.

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | B20's replace-with-`_` is injection-safe by construction ONLY if CR/LF/NUL keep the hard refusal (http.rs:357); replacement applies to other C0 controls + DEL. Header NAMES are not a token check today (space/colon/CR/LF/NUL only) — B20 must add the RFC 9110 §5.6.2 tchar hard refusal (a tightening). Proxy-Authorization is emitted outside `push_header` (http.rs:348-349; proxy.rs:149-150 CONNECT) — route through the hygiene helper. Replacement logs at debug; refusal messages keep `{:?}` escaping; cookie values never reach log channels (B5 contract). B21 Retry-After bounded 60 s, delta-seconds only, stop-responsive ticks, worker jitter so 32 workers don't reconnect in one burst; 429 is not retryable today — the spec must classify 429-with/without-Retry-After explicitly and fix that `run_single`'s `"server returned {}"` string never matches `is_retryable` (run_single does not retry 5xx at all). No-resume notice must be generic, never embed URL/query/path. B22 is structurally a floor-raising knob (rustls+ring supports only TLS 1.2/1.3); fail-closed at tls.rs wrap; tolerant config load; no env knob. B24: verify the CocCoc dir name empirically or it is a silent no-op. Parked items stay parked; FTP gate must pre-constrain the PASV data target (SSRF/bounce primitive). |
| FALS | **PROCEED-WITH-CONDITIONS** | The B21 "silent restart" is real AND worse than proposed: `run_single` truncates the `.part` (`File::create`) while a validator-matching `.vdmctl` survives untouched — a later resume against a range-capable probe loads the stale state and skips ranges whose bytes were truncated → a completed file with zero-holes. B21 must upgrade to a coherency rule (run_single invalidates the matching sidecar when it truncates) with a SHA-256 falsification test, and the notice must fire exactly when valid state was discarded. B21 Retry-After must pin loop (worker `!ok_status` is where non-206/200 statuses retry), statuses (429+503), bound, and resolve-or-scope the run_single 5xx inconsistency. B20 is either a regression (full charset whitelist breaks RFC 9110 obs-text values that work today, e.g. UTF-8 cookies from B5) or a near-no-op (CR/LF/NUL-only replacement masks caller bugs as opaque 401/403s) — narrow to refusal-pinning tests + controls-only replacement surfaced by a notice, claimed as robustness-not-security. B22's wire-level claim is docs-only (no TLS test server in-sandbox); unit test via rustls 0.23's public `protocol_versions` field. B23 is already true (`keepalive_reuse_across_leases` pins it) — record as witness, not adoption. B20-B24 rows do not exist in study §5 yet — the spec commit must create them. |
| REQ | **PROCEED-WITH-CONDITIONS** | Scope honesty: §4d's request-builder slot-machine shape, FTP PASV machine, and config hot-reload/privileged-helper-launch have no §5/N rows — commit 1 must add explicit rows (N7 hot-reload/helper-launch N/A; N8 format-string request builder — our typed `Request`/`ClientCfg` is the deliberate shape) so nothing is silently ignored. The "all frontends render `notice`" premise is FALSE (verified): only the CLI renders it; macos `TaskSnapshot` decode drops it, win32/slint render `error` only, the popup renders neither — B21's user-visible honesty must ride the `error` classification (B19 marker style) with `notice` as the additive hint, and the false comment at task.rs:1554-1556 must be corrected while the file is open. B18/B21 precedence pinned: on an accept-ranges-none server a 200 is no-resume, not "resource changed". B22 additive path confirmed (EngineConfig serde default → TaskEnv/ClientCfg → single tls.rs site → settings.get/set → config.json tolerant load); ride settings, not env. Toolchain: no rustup proxy this instance — `cargo +1.75.0` errors; the working shape is the direct 1.75.0 toolchain path on PATH (verified 16.11 s green on win7); registry cache empty → first build fetches; disk 4.1 GB → clean target/ between trees. Ledger round-4 note quoting the directive verbatim belongs in commit 1. Adopt B24 decided, not dangling. |
| INT | **PROCEED-WITH-CONDITIONS** | `cargo +1.75.0` is not invokable (shims reject `+`); direct-toolchain `PATH=…/1.75.0…/bin cargo check --workspace --locked` verified green (16.11 s, one pre-existing dead_code warning) — MEMORY's `+1.75.0` instruction is stale for this instance and the working shape must be recorded. B22 needs NO new crate (both locks pin rustls 0.23.46; `with_protocol_versions` exists); B21 must declare delta-seconds-only to keep a date-parsing crate out (lock parity). CI blind spots enumerable: B20's flip is invisible to e2e → unit tests in tests/http_engine.rs; B21 timing tests must be parse-only/delay-computation (no wall-clock sleep assertions on the 3 OS jobs; Linux-gate precedent http_engine.rs:572-575); B22 is CI-blind end-to-end → unit-testable selection helper + the local 1.75 witness; B24 shell scripts have no CI job → `bash -n` + dry-run witness. Cross-tree drift clean (task.rs/tls.rs byte-identical; http.rs differs only in 1.75-compat forms; win7 tls.rs has never been ported — B22 is its first delta). B20×B17 coupling: no double-write risk; edge — a mangled non-ASCII Accept-Encoding still counts as caller-chosen, B17's Compressed guard backstops; engine-generated headers pass through, pinned by test. Android has no daemon-snapshot parsing at all (evidence-pack premise false, no risk). Disk budget: sequential single builds, clean target/ between, bench builds `-p velocitydm` only. |
| REL | **PROCEED-WITH-CONDITIONS** | Release posture verified: v-tag ruleset active, wave 4 push-main-only, zero dispatches, zero release.yml edits — restate verbatim in this record. Git state green now; re-run fetch-before-push immediately before the push (three-proofs lesson). Commit split: docs commit first, then per-item revertable commits each carrying both trees + tests; **B21 split into two commits** (no-resume state vs Retry-After — different risk classes, independent reverts). B23 needs no commit. Bench A/B pair is MANDATORY (CLEAN_ROOM.md:53-56 bar; wave 4 may not self-amend it): warm-pair s11i4-pre/s11i4-post, parity is the pass criterion (12 conns / 37 reqs shape), deviation investigated never explained away; record committed (pattern scripts/s11i3-bench-record.txt) + MEMORY. CI witness: poll-checks.sh counts dynamically; honest close = "11/11 — 8 required + 3 preview". docs/BUILDING.md gains the tlsMin row; docs/RPC.md gains the missing `notice` row in the snapshot enumeration (pre-existing gap). CLEAN_ROOM.md needs a new session-16/wave-4 disclosure row. Backup last, zero dirty files; the backup script's post-manifest BACKUPS.md append is a known intentional dirt carried by the next commit. B24 packaging effect is inert until the promotion gate regardless. |

## Consensus

Zero `BLOCK` → **the gate closes with this record.** Conditions folded into
`docs/SPECS/S11-ADOPTIONS-WAVE4.md` and the execution plan below.

## Conditions → plan mapping

1. B20 narrowing (FALS #3 / ATKR #1-2 / INT #5) → spec rules 1-6: tchar
   name hard-refusal (tightening), CR/LF/NUL value hard-refusal unchanged,
   controls+DEL replace-with-`_` surfaced by notice + debug log, obs-text
   passes (UTF-8 cookies keep working), Proxy-Auth paths routed through the
   helper, engine-generated headers pass through (test-pinned).
2. B21 upgrade (FALS #1 / REQ #2-3 / ATKR #5-7) → spec rules 7-12: the
   `.vdmctl`/`.part` coherency rule + SHA-256 falsification test; notice
   rides `error` classification + `notice` hint (rendering matrix stated
   honestly); no-resume precedence over B18's changed-resource; Retry-After
   delta-seconds only, 429/503, worker `!ok_status` + probe/run_single
   ladders, 60 s cap, stop-responsive RA waits, parse-only deterministic
   tests; the run_single 5xx non-retry inconsistency resolved structurally.
3. B22 shape (ATKR #8 / REQ #5 / INT #2-3 / FALS #4) → spec rules 13-17:
   additive `tlsMin` settings knob (`default|tls12|tls13`, default =
   zero change), settings.get/set + config.json tolerant load, fail-closed
   at wrap, unit-testable selection helper asserting rustls
   `protocol_versions`, wire claim docs-only, no env knob, no new crates,
   docs/BUILDING.md row.
4. B23 witness (FALS #5 / REQ #8 / REL #3) → no behavior spec, no commit;
   witness = the existing `keepalive_reuse_across_leases` + the bench
   request-shape parity, recorded in the bench record + MEMORY; the drain-
   failure recycle edge re-verified during the bench.
5. B24 deferral (ATKR #9 / FALS #6 / REQ #9) → CocCoc NMH paths could not
   be empirically verified in this sandbox (AUR bot-blocked, official deb
   URL 404); an unverified path is a silent no-op honesty bug per the
   panel's own condition. B24 is DEFERRED with the verification
   pre-condition recorded in study §5 (not adopted, not silently dropped).
6. Scope honesty rows (REQ #1) → study §5 gains N7 (registry hot-reload +
   token-duplicated helper launch — Windows-registry-era, N/A for a GPL
   cross-platform user-space product) and N8 (format-string request
   builder — our typed `Request`/`ClientCfg` builder is the deliberate
   shape); the FTP row gains the PASV-target pre-condition.
7. Ledger + docs truth (REQ #11 / REL #8-9) → commit 1 carries DECISIONS
   round 4 (directive quoted verbatim), CLEAN_ROOM session-16/wave-4 row,
   and the docs/RPC.md `notice` row.
8. Toolchain truth (INT #1 / REQ #6) → the wave uses the direct 1.75.0
   toolchain path (`PATH=/home/z/my-project/tools/rustup/toolchains/
   1.75.0-x86_64-unknown-linux-gnu/bin:$PATH cargo check --workspace
   --locked` inside `win7/`), recorded in MEMORY; registry cache re-fetch
   on first build; clean `target/` between tree builds; bench builds
   `-p velocitydm` only.
9. CI witness + close (REL #6-7) → push main; `scripts/poll-checks.sh`;
   close states 11/11 (8 required + 3 preview); windows-latest flake
   policy: evidence-then-rerun; issue #15 close comment; MEMORY entry;
   ROADMAP status flip; close commit; backup last; throwaway dirs wiped.

## Execution plan

1. Commit 1 (docs): this gate + `docs/SPECS/S11-ADOPTIONS-WAVE4.md` +
   study §5 rows (B20-B24 adopted/deferred, N7/N8, FTP pre-condition) +
   ledger round 4 + CLEAN_ROOM row + docs/RPC.md `notice` row.
2. Bench pre: build release `-p velocitydm`, `bench-perf.sh s11i4-pre`
   (warm pair), record committed.
3. B20 implementation (root + win7): tchar name refusal + value hygiene
   helper + Proxy-Auth routing + unit matrix (names, CR/LF/NUL refusal,
   controls replacement, obs-text pass-through, engine headers untouched).
4. B21a implementation (root + win7): no-resume classification at the
   `accepts_ranges` branch — `error` marker + `notice` hint when partial
   state existed; `run_single` invalidates the matching `.vdmctl` on
   truncate; SHA-256 coherency test on the no-range testserver.
5. B21b implementation (root + win7): `parse_retry_after` (delta-seconds
   only) + `retry_delay_for` (bounded 60 s) + structural status threading
   into the worker `!ok_status` branch and the probe/run_single ladders;
   stop-responsive RA waits; parse/delay unit tests (no wall-clock CI).
6. B22 implementation (root + win7 — tls.rs's first win7 port): additive
   `tlsMin` knob end-to-end (EngineConfig → ClientCfg → wrap), selection
   helper + `protocol_versions` unit tests, settings.get/set, docs/
   BUILDING.md row.
7. B24: no implementation (deferred per condition 5).
8. Battery: fmt both trees, clippy `-D warnings` (root), vdm-core lib +
   http_engine + hls_e2e + bt suites, direct-1.75.0 win7 check per item
   (accumulated), extension node battery (unaffected, run anyway), bench
   post warm pair `s11i4-post` with request-shape parity check.
9. Push main; poll-checks.sh; comment issue #15 with the wave-4 close
   evidence; MEMORY entry; ROADMAP S11-i row update; close commit; backup;
   wipe throwaway dirs.
