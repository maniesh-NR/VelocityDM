# Assessment 0018 — S8 wave 1: Android ↔ desktop daemon over LAN (gate record)

**Date:** 2026-11-26 (UTC). **Stage under review:** S8 activation, wave 1 —
the LAN half of "Android ↔ desktop daemon over LAN (shared RPC)". Roadmap
S8 also owns the Android engine unification onto the shared Rust core;
this gate splits that into **wave 2** (its own future gate — NDK/JNI +
CI cross-compile), honestly deferred, not silently dropped. The S8 queue
position is the honest remainder of the single-session directive: S5/S6/
S7/S12-lin3 all closed (sessions 20–21), S10/T1/T3/D5:A/D6 are
owner-gated, so S8-W1 → S9 is the live queue.

**Stage type:** engine-adjacent RPC transport change + Android client
addition. **No S11/T2 linkage, no T2 provenance, no CLEAN_ROOM
disclosure row owed** (nothing here derives from the studied reference
binary; the design is our own fail-closed posture on a documented
protocol). The bench A/B bar: run the cheap A/B anyway (REL condition —
`bench-perf.sh` drives through the daemon RPC, so the loopback request
path gains a peer-classification branch; parity expected and recorded).

**Proposed shape (evidence pack):** daemon LAN mode — a non-loopback
bind is FAIL-CLOSED: bearer token (32-byte random hex, persisted
separately from `EngineConfig`, constant-time compare, required on
`/rpc` + `/events` + `/health` from non-loopback peers) + TLS
(self-signed cert via rcgen/rustls; plain HTTP refused for LAN peers;
loopback peers keep today's byte-identical plain path) + connection
cap. Android remote mode: settings (host/port/token), OkHttp JSON-RPC +
SSE consumer, TOFU cert pinning, dual-mode engine swap, explicit
snapshot→`TaskState` mapping. New dep: rcgen (optional, under a
non-default `lan` feature of `vdm-rpc`). Wave 2 (deferred): Android
Rust-core unification.

**Panel:** five-role per `PROTOCOL.md` (ATKR / FALS / REQ / INT / REL),
dispatched independently with the identical evidence pack (proposed
shape + repository access + verified environment facts: JDK 21 + node
present; no Android SDK/NDK in the sandbox → the android-frontend CI
job is the sole Android compile witness, `:shared:jvmTest` the sole
Android test witness; full Rust battery local after toolchain
re-provision; win7/ is a byte-identical-mirror second workspace that
must keep compiling on Rust 1.75 with `--no-default-features --features
tls`). Agents saw no peer verdicts.

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | (1) HIGH: today `--addr 0.0.0.0` already serves full unauthenticated engine control to LAN natives (wildcard branch of `host_allowed` accepts any IP-literal Host; Origin rule only stops browsers) — W1 must check token+TLS readiness BEFORE binding non-loopback (close the live footgun, say so in docs). Local clients (CLI/NMH/GUI/qt/gtk3/Swift, all hardcode 127.0.0.1:36990 plain HTTP) must keep working on a LAN-bound daemon → peer-scoped (not bind-scoped) demux; fail closed on `peer_addr()` errors; canonicalize `::ffff:127.0.0.1` (mapped-v4 `is_loopback()` is false). (2) HIGH: token in `config.json` is silently destroyed — `save_config` round-trips `EngineConfig` (unknown fields dropped) on every `ConfigChanged`, and `fs::write` gives 0644; portable mode would put secrets beside the binary → separate `lan-secrets.json`, 0600 at creation via `OpenOptions`, never inside `EngineConfig`; document portable risk. (3) MED: header flood (line-count/total-size uncapped) defeats a 32-conn cap via 300s read timeout → cap ~128 lines/64KB for non-loopback, enforce Authorization as soon as parsed, exempt loopback. (4) MED: Android `usesCleartextTraffic="true"` — remote mode must reject `http://` in code; token input `textPassword`; redact Authorization from logs; plain SharedPreferences acceptable for W1 with documented caveat. (5) MED: TOFU hardening — persist cert+key across restarts (else pins break and users are trained to accept changes); stored-fingerprint mismatch = hard fail, never silent re-pin; fingerprint printable for out-of-band compare; add rotation. (6) LOW: rcgen MIT/Apache-2.0 GPL-compatible, `default-features=false` + ring backend (no aws-lc-rs); token via `ring::rand::SystemRandom` (already in graph). (7) LOW: docs must state token ≡ full engine control (proxy settings = SSRF, arbitrary outDir). |
| FALS | **PROCEED-WITH-CONDITIONS** | (1) HIGH: "loopback byte-identity" is PEER-scoped, not bind-scoped — bind-wise TLS breaks every local client (gtk3 polls POST /rpc at 200 ms); branch on `peer.is_loopback()`; `handle_conn`/`write_response`/`sse_loop` take `&mut TcpStream` — needs a `Read+Write` generalization to host rustls `StreamOwned`; keep `web_page_request_shapes_are_refused`/`host_allowed_matrix` as byte-identity witnesses. (2) HIGH: unauthenticated pre-handshake LAN SYNs occupy conn-cap slots for 300s → 32 idle sockets lock out the local CLI/GUI → exempt loopback (separate quota), ≤10s pre-auth timeout for non-loopback. (3) HIGH: SSE semantics unstated — `sse_loop` ticks omit removals, `subscribe()` has no replay; Android must replicate the gate-0010 consumer contract (initial `downloads.list`, `task-removed` convergence, resync window, dead-daemon probe); OkHttp SegmentEngine client has 30s read timeout — SSE needs a dedicated client. (4) HIGH: remote actions/notifications silently dead if not routed — `notify` fires only from the local engine listener; pause/resume/remove call SegmentEngine; notif id `t.id.toInt()+1000` collides across stores → route by origin, distinct notif tags, SSE drives notify. (5) MED-HIGH: mapping lies — TaskState lacks `verifying`/`seeding` (adapter gives them no chip/action; seeding unstoppable), `notice` dropped, `userPaused` dropped (stop-window-paused shows Resume that instantly re-stops) → explicit mapping table + adapter coverage. (6) MED: store confusion — remote add must NOT send phone `dirPath` as outDir; never merge local+remote lists (id collision). (7) MED: token persistence (0644 default, EngineConfig round-trip) → separate file, 0600, regenerate only when absent, `Authorization` header (never query string on `/events`). (8) MED: W2 honesty — ROADMAP row must not read DONE; Android BT/HLS are remote-only in W1; CI android-frontend runs no `:app` unit tests → RPC/SSE/mapping logic must land in `:shared` with jvmTest coverage. |
| REQ | **PROCEED-WITH-CONDITIONS** | (1) HIGH scope honesty: split is legitimate (PROMOTION-GATES §5.4 already names the two pieces); ROADMAP flips to "ACTIVE (W1 DONE …; W2 pending its own gate)", never DONE; mDNS discovery + QR pairing must be EXPLICIT deferred non-goals so manual host:port+token isn't a quiet shrink; promotion-grade W1 minimum = Android exercises the same `/rpc` method surface as CLI/NMH + SSE over authenticated TLS with graceful daemon-unreachable handling. (2) HIGH spec-first: pin the bind×client matrix; say W1 CLOSES the live unauthenticated-wildcard footgun; pin wildcard semantics (per-peer demux), token-on-/events+/health, "byte-identical" definition + test, token provisioning UX. (3) HIGH TLS/TOFU lifecycle: cert path/SANs/validity, regen-on-restart → Android re-pin (refuse vs warn), first-connect MITM acceptance stated, hostname-verification-vs-pin choice, rotation mechanism. (4) MED: pin the Android remote method set (full parity or name every deferred method), the dropped-fields table, unreachable/token-rejected/cert-mismatch failure modes with timeout+backoff values, mode-switch semantics for in-flight local tasks. (5) MED deliverables: android/README + ARCHITECTURE rows, S12-PROMOTION-GATES Android "Core binding" cell (goes stale), README daemon-section parity, S12-e release-integrity rules (push-main-only, release.yml + v-tag ruleset untouched, versionCode/versionName frozen, v1.0.0-android keeps the Compose APK). (6) MED test-truth: win7 port decision named (authorized divergence = feature availability only), MSRV ≤1.75 for new deps confirmed, both locks updated as needed. (7) MED: client/mapping/TOFU logic in `:shared` jvmCommon; TOFU re-pin UX, Doze reconnect, real-device LAN, battery = NO test → owner-verification rows (D5:A class); consider a CI non-loopback TLS integration test. (8) LOW: W1 clean of scope creep; no-bench-A/B matches the zero-engine-change precedent but bench-perf drives THROUGH the daemon — cite or run. (9) LOW: commit split C1 gate+spec → C2 → C2b android → C3 close → C4 backup; issue #5 comment, stays open until W2. |
| INT | **PROCEED-WITH-CONDITIONS** | (1) HIGH CI: a non-default `lan` feature compiles in ZERO jobs (all test jobs plain `--workspace`, clippy `--workspace --all-targets`, e2e `cargo build --release`) → add explicit `cargo test -p vdm-rpc --features lan` and `cargo clippy -p vdm-rpc --all-targets --features lan` steps INSIDE existing required jobs (names unchanged → branch protection intact). (2) HIGH locks: gtk3/qt path-dep vdm-rpc with defaults → default-on `lan` would pull rcgen into their graphs and break `--locked` → keep `lan` NON-default; root lock gains rcgen+pem(+transitive) regardless. (3) HIGH win7: mirror src stays byte-identical with `#[cfg(feature = "lan")]` gates; do NOT declare `lan`/rcgen in win7's Cargo.toml (cfg on undeclared feature is inert on 1.75; `unexpected_cfgs` lint is 1.80+); win7 lock untouched; rcgen `optional = true, default-features = false, features = ["ring","pem"]` — NO aws-lc-rs (absent from all 4 locks today; rustls already `default-features=false ["ring","std","tls12"]`); ring 0.17.14 already in all locks; rcgen 0.13 reuses ring 0.17; registry fetch needed for the lock update (network reachable this instance). (4) MED e2e: loopback e2e stays green via byte-identity, but nothing tests non-loopback bind/token/TLS → add a LAN leg under the feature (bind 0.0.0.0, token, TLS client). (5) OK Android: OkHttp 4.12.0 already in `:shared` — manual SSE via source() line reading = zero new deps; okhttp-sse and androidx.security:security-crypto must NOT be added; jvmCommon is the home (jvmTest-covered); ViewModel stays thin in `:app`; minSdk 26 — TLS 1.3 native from API 29, keep rustls `tls12` (it is) so 26–28 handshake works; custom X509TrustManager pins on all API levels; cert needs SANs anyway. (6) LOW: JDK 21 fine for AGP 8.7; no Android SDK → CI-only witness matches the S12-e pattern. |
| REL | **PROCEED-WITH-CONDITIONS** | (1) HIGH CI witness for `lan` (same as INT-1). (2) MED: the 1.75 job is `--no-default-features --features tls --locked`, NOT default features — spec must pin the invariant (`lan` stays non-default and uncoupled from `tls`) and keep rcgen out of win7 manifests+lock (confirmed separate pinned workspace). (3) MED mirror discipline: engine/RPC commit hand-ports cfg-gated LAN code into the mirror IN THE SAME commit, recording the authorized divergence; honest revert unit = the single Rust commit; the android commit reverts independently (remote mode default OFF → harmless dead UI if daemon reverts). (4) MED token at rest: 0600 or documented risk. (5) LOW: serde verified tolerant both ways (no `deny_unknown_fields`, all `#[serde(default)]` — upgrade/downgrade safe; token drops on downgrade-save → re-pair, runbook note); Android no persisted settings today → default-OFF forces no migration; loopback/NMH surface untouched (scripts/e2e.sh exercises NMH+SSE+daemon = the regression net). (6) LOW: bench claim imprecise — bench-perf drives through the daemon RPC; run the cheap A/B or state coverage. (7) Runbook owed: token recovery/rotation; cert identity stability across restarts; LAN bind failure modes; android-frontend witness = compile + jvmTest only (runtime pairing owner-verified, D5:A class); rcgen registry + Maven deltas; MEMORY entry + BACKUPS row after close. (8) Failure policy: red android-frontend blocks stage close (precedent: close at all-green); poll budget 2400 s; evidence-then-fix loop expected. (9) Release integrity clean: no release.yml/packaging/tag contact; docs/RPC.md's current "Wildcard (0.0.0.0) binds accept any IP literal" sentence is exactly the surface being closed — reconcile it. |

## Consensus

Zero `BLOCK` → **the gate closes with this record.** All conditions fold
into `docs/SPECS/S8-LAN-RPC-WAVE1.md` (committed BEFORE any
implementing commit) and the execution plan below.

## Conditions → plan mapping

1. Peer-scoped demux + fail-closed startup (ATKR-1, FALS-1, REQ-2) →
   spec §2: non-loopback bind refuses to start without the `lan`
   feature; with it, per-peer demux (loopback peer = plain byte-identical
   path; non-loopback peer = TLS then token); `peer_addr()` errors drop
   the connection; `::ffff:127.0.0.1` canonicalized; generic
   `Read+Write` stream layer; existing shape tests kept as
   byte-identity witnesses.
2. Secrets storage (ATKR-2, FALS-7, REL-4) → spec §3: separate
   `lan-secrets.json` (token + cert + key), 0600 at creation, never in
   `EngineConfig`, stable identity across restarts, regenerate only when
   absent, rotation via CLI, portable-mode risk documented.
3. DoS bounds (ATKR-3, FALS-2) → spec §4: loopback exempt + unbounded
   (today's behavior); non-loopback: 32 concurrent cap, 10 s pre-auth
   read timeout, 128-line/64 KiB header cap, auth enforced
   post-header-parse pre-body.
4. Auth mechanics (ATKR-6/7) → spec §3: `Authorization: Bearer` header
   only (never query string), constant-time XOR-fold compare, 401 on
   failure + close, token generated via `ring::rand::SystemRandom`.
5. Android consumer contract (FALS-3/4/5/6, REQ-1/4, INT-5) → spec §5:
   gate-0010 SSE contract restated for Android (initial list,
   task-removed convergence, resync window, dead-daemon probe,
   launch-with-dead-daemon idempotency); dedicated OkHttp client (no
   cookie/redirect, SSE client separate with no read timeout); explicit
   snapshot→TaskState mapping table incl. additive `notice` +
   `userPaused`, passthrough states, chip/action coverage for
   verifying/seeding, dropped-fields table (etaSecs, connections,
   peers/seeds, priority, startAt, checksumOk, createdAt) recorded
   honestly; remote add = url+filename only; lists never merged;
   notifications routed by origin with distinct tags; logic in
   `:shared` jvmCommon with jvmTest coverage; http:// rejected in code.
6. TOFU lifecycle (ATKR-5, REQ-3) → spec §3/§5: cert persisted (stable
   identity), mismatch = hard fail + surfaced error, manual fingerprint
   entry supported, first-connect acceptance stated as the MITM window
   (mitigated by out-of-band fingerprint compare), long validity
   documented, hostname verification replaced by the pin (stated).
7. CI witness (INT-1/2, REL-1/2) → spec §6: `lan` non-default; feature
   steps added inside existing required jobs (Test ubuntu-latest +
   Clippy), job names unchanged; gtk3/qt locks untouched (defaults
   inherited); root lock gains rcgen (+pem) only.
8. win7 mirror (INT-3, REL-2/3) → spec §6: src mirror byte-identical
   under `#[cfg(feature = "lan")]` (inert on win7's undeclared feature,
   cargo 1.75); no win7 manifest/lock change; authorized divergence =
   feature availability only; same-commit port; 1.75 check green.
9. LAN test leg (INT-4, REQ-7) → spec §6: non-loopback integration
   tests in vdm-rpc's feature-scoped suite (bind 0.0.0.0, UDP-trick
   local-IP discovery, rustls client pinned to the generated cert,
   token matrix, plain-HTTP-to-TLS refused, conn-cap behavior);
   scripts/e2e.sh untouched (loopback net already covers byte-identity).
10. Bench (REL-6, REQ-8) → spec §6: run the cheap A/B
    (scripts/s8-bench-record.txt), expect parity (one branch per
    connection on the loopback path), record honestly.
11. Deliverables + honesty rows (REQ-5/7, REL-5/7/9) → spec §7: doc
    flips list (RPC.md incl. the stale wildcard sentence, REQUIREMENTS-
    COVERAGE, android/README + ARCHITECTURE, S12-PROMOTION-GATES
    Android core-binding cell, README daemon parity, RUNBOOK LAN
    section, ROADMAP ACTIVE-not-DONE, MEMORY + BACKUPS, issue #5
    comment); owner-verification rows for the unverified classes; W1
    non-goals (mDNS, QR, keystore encryption, hostname verification,
    SSE beyond Android, W2 unification).
12. Commit split (REL-3, REQ-9) → C1 this record + spec; C2 Rust
    (root crates + win7 mirror + root lock + CI steps, single revert
    unit); C2b Android (independently revertable); C3 doc flips +
    close; C4 backup last. Push-main-only; release.yml, packaging/**,
    tags, ruleset 27298722, versionCode/versionName untouched.

## Execution plan

1. **C1 (this commit):** gate record + `docs/SPECS/S8-LAN-RPC-WAVE1.md`.
2. **C2:** vdm-rpc `lan` module (secrets, TLS accept path, peer demux,
   auth, caps) + `velocitydm lan-token [--rotate]` CLI + win7 mirror
   port + root lock update + CI feature steps; full local battery
   (fmt, clippy both trees, workspace tests, `vdm-rpc --features lan`
   tests incl. the non-loopback leg, 1.75 witness) + bench A/B.
3. **C2b:** Android remote mode (`:shared` jvmCommon client/SSE/TOFU/
   mapping + `:app` settings + dual-mode wiring); local battery =
   source review (CI android-frontend = compile + jvmTest witness).
4. **C3:** doc flips + ROADMAP + MEMORY + issue #5 comment; CI all-green
   witness at the close SHA (14 jobs: 8 required + 6 preview).
5. **C4:** backup run + BACKUPS.md row, zero dirty files.
