# S8-W1 — LAN mode: authenticated TLS RPC for non-loopback peers + Android remote mode (behavior spec)

**Status:** activation gate 0018 (`harness/ASSESSMENTS/0018-s8-lan-rpc.md`,
5× PROCEED-WITH-CONDITIONS, zero BLOCK). This spec is committed BEFORE any
implementing commit (spec-first barrier). Stage type: RPC transport +
Android client addition — **no S11/T2 linkage, no T2 provenance**; no
CLEAN_ROOM row owed (explicitly recorded). Wave 2 of S8 (Android engine
unification onto the shared Rust core via NDK/JNI) is a **separate future
gate** and is NOT covered here. The work is push-main-only; `release.yml`,
`packaging/**`, tags, the `v*` ruleset (id 27298722), Android
versionCode/versionName are untouched; the frozen v1.0.0-family releases
and the v1.0.0-android Compose APK stay frozen.

## 0. Problem being closed

Today `velocitydm daemon --addr 0.0.0.0:36990` serves **full
unauthenticated engine control to any LAN native client** — the wildcard
branch of `host_allowed` (crates/vdm-rpc/src/lib.rs) accepts any
IP-literal `Host`, and the Origin/Referer rule only stops browsers. S8-W1
closes that live footgun: a non-loopback bind becomes fail-closed
(token + TLS), while every loopback client (CLI, NMH, egui GUI, slint,
gtk3, qt, Swift frontends — all hardcode `127.0.0.1:36990` plain HTTP)
keeps today's behavior **byte-identically**.

## 1. Scope

**IN:**

1. `vdm-rpc` gains a non-default `lan` feature (implies `tls`; adds
   optional direct deps `rcgen` (ring backend, NO aws-lc-rs, no
   x509-parser), `rustls`, `ring`).
2. Startup posture: `RpcServer::start` with a non-loopback bind
   (explicit LAN IP or wildcard) REFUSES to start when the `lan`
   feature is off (clear error naming the feature and the refusal
   reason); with the feature on it loads-or-generates LAN secrets and
   serves with per-peer demux.
3. Per-peer demux on a single listener: loopback peer → today's plain
   HTTP path, unauthenticated, byte-identical (Host/Origin/Referer/
   Content-Type rules unchanged, 300 s timeout, no caps); non-loopback
   peer → TLS (rustls `ServerConnection` around the accepted stream;
   plain-HTTP bytes to this path fail the handshake and the connection
   is dropped — no HTTP error, fail-closed) and, after the handshake,
   `Authorization: Bearer <token>` REQUIRED on `/rpc`, `/events` AND
   `/health`; failure → `401` + connection close.
4. LAN secrets: separate file (`velocitydm-lan-secrets.json` beside the
   config — portable → exe dir, else `$XDG_CONFIG_HOME|$HOME/.config/
   velocitydm/`), NEVER inside `EngineConfig`/`config.json` (which
   `save_config` round-trips and rewrites on every `ConfigChanged`).
   Content: `token` (32 bytes → 64 hex, from `ring::rand::
   SystemRandom`), `cert` (PEM), `key` (PEM). Created 0600 on Unix
   (`OpenOptionsExt::mode`); Windows default ACLs documented. Stable
   identity across restarts (loaded, not regenerated) so TOFU pins
   survive; regenerated only when absent or on explicit rotation.
5. CLI: `velocitydm lan-token` prints the current token (local file
   read, never over RPC); `velocitydm lan-token --rotate` replaces the
   token (cert unchanged) and prints the new one. The daemon prints the
   token to stdout when it generates one. Docs state the token ≡ full
   engine control (settings incl. proxy = SSRF surface; arbitrary
   outDir writes).
6. Resource bounds (non-loopback peers only; loopback exempt — today's
   behavior): max 32 concurrent connections (beyond → immediate close),
   10 s read timeout until the first authorized request completes (then
   300 s), header caps of 128 lines / 64 KiB total (`431` + close),
   `Authorization` enforced as soon as the header block is parsed
   (before the body is read).
7. Android remote mode (default OFF; local mode unchanged):
   `:shared` jvmCommon gains `remote/RemoteRpc.kt` (dedicated OkHttp
   client: no cookies, `followRedirects(false)`, call timeout 10 s,
   read 20 s; `Authorization` interceptor; `https://` only — `http://`
   hosts rejected in code because the manifest keeps
   `usesCleartextTraffic="true"` for local engine dev URLs),
   `remote/SseClient.kt` (separate OkHttp client with no read timeout;
   buffered line reader; 1 MiB frame-buffer DROP cap; lenient
   `data:`-only parse; reconnect with 2 s backoff; `stop()`), a pure
   `SseFrameParser` (testable), `remote/TofuTrustManager.kt` (accept +
   store the leaf SHA-256 fingerprint on first connect; stored
   mismatch = hard fail surfaced as an explicit cert-changed error;
   manual fingerprint entry supported; hostname verification replaced
   by the pin, stated), and `remote/RemoteEngine.kt` implementing the
   SegmentEngine-shaped contract (tasks list + listener) via the
   gate-0010 consumer contract: initial `downloads.list`; ticks merge
   in place keyed by id; `task-removed`/terminal metadata drift →
   coalesced 150 ms re-list; dead stream → stale banner state + frozen
   rows + 2 s probe; launch-with-dead-daemon idempotent.
8. Android snapshot→`TaskState` mapping (explicit; `TaskState` gains
   additive `notice: String? = null` and `userPaused: Boolean? = null`
   — Kotlin defaults keep local construction and the strict `.vdmctl`
   schema untouched): `id,url,name→filename,dir→dirPath,size,done,speed`
   direct; `state,error,notice` passthrough strings (states
   `verifying`/`seeding` render as distinct chips with honest action
   availability — seeding has no pause, remove works, resume only when
   `userPaused != false`); DROPPED for W1 and recorded: `etaSecs,
   connections, peers, seeds, priority, startAt, checksumOk,
   createdAt` (the Android card renders none of them today; BT p/s is
   a remote-side limitation stated in android docs).
9. Remote add: `url` + `filename` only — never `outDir` (the daemon's
   `downloadDir` owns placement); magnet URLs route to
   `downloads.addTorrent` (magnet param). Mode switch replaces the
   task list wholesale — local and remote lists are never merged (id
   namespaces are independent). Notifications route by origin with a
   distinct tag namespace (`remote-<id>`) so ids cannot collide with
   local notification ids.
10. `:app` remote settings (plain `SharedPreferences` — app-private
    storage; no `androidx.security:security-crypto` dep in W1,
    caveat documented): enabled (default false), host, port (default
    36990), token (`textPassword` input, redacted from any logs),
    fingerprint (display + optional manual entry), a Test-connection
    button (`daemon.ping`) with an honest result line. Errors
    unreachable/token-rejected/cert-mismatch are distinct strings.

**OUT (explicit non-goals of W1):** mDNS/service discovery; QR pairing;
TLS hostname verification (the fingerprint pin replaces it); keystore-
encrypted Android token storage; SSE consumers beyond this Android
surface (gtk3/qt stay 200 ms poll — their own later sub-stage); any
change to the loopback trust boundary, the NMH/extension surface, or
`docs/EXTENSIONS.md`; wave 2 (Android Rust-core unification, NDK/JNI);
`release.yml`/packaging/tag contact; new Android dependencies.

## 2. Rules (gate-0018 conditions folded)

1. **Byte-identity is peer-scoped, never bind-scoped** (ATKR-1/FALS-1):
   the loopback path through a wildcard listener is identical to the
   loopback-listener path (same Host/Origin/Referer/Content-Type
   handling, same timeouts, no new caps). The existing shape tests
   (`web_page_request_shapes_are_refused`, `host_allowed` matrix) stay
   green unchanged and a wildcard-listener loopback test is added.
   `peer_addr()` errors drop the connection (fail closed);
   `::ffff:127.0.0.1` is canonicalized to v4 before the loopback check.
   The stream layer generalizes `handle_conn`/`write_response`/`sse_loop`
   over `Read + Write` (plain `TcpStream` and rustls `StreamOwned` are
   the two instantiations).
2. **Auth mechanics** (ATKR-6/7): token only ever in the
   `Authorization: Bearer` header (never query string — `/events`
   included); constant-time compare (XOR-fold over the fixed-length
   hex token; the length check leaks only length); checked immediately
   after the header block; `401` + close on failure; token generated
   with `ring::rand::SystemRandom`; never logged, never returned over
   RPC, never written with group/other read bits.
3. **TLS posture** (ATKR-5/REQ-3): self-signed cert, CN
   `velocitydm-daemon`, SANs `DNS:localhost, IP:127.0.0.1, IP:::1`
   (informational — clients verify by fingerprint pin, stated);
   validity 3650 days (rationale: identity is the pinned fingerprint,
   expiry would force re-pins with no security gain in the LAN TOFU
   model; rotation is available via `--rotate`/file deletion);
   rustls `tls12` stays enabled so Android API 26–28 handshakes work;
   the first-connect acceptance is the documented MITM window,
   mitigated by out-of-band fingerprint compare (`velocitydm
   lan-token` prints the token; the fingerprint is shown in the
   Android settings row).
4. **Resource bounds** (ATKR-3/FALS-2): non-loopback-only; loopback
   exempt and unbounded (today's behavior). Pre-auth timeout 10 s;
   after the first authorized request the connection adopts the
   existing 300 s read timeout.
5. **Android consumer contract** (FALS-3/4/5/6, INT-5): logic lives in
   `:shared` jvmCommon with `:shared:jvmTest` coverage (frame split,
   mapping table, TOFU decision, failure strings, http-rejection);
   the ViewModel stays thin in `:app`; the remote engine's lifecycle
   matches the local engine's (same scope — no new lifetime claims);
   `DownloadService.notify` is driven by the remote listener for
   remote tasks.
6. **Feature/lock discipline** (INT-1/2/3, REL-1/2): `lan` is
   non-default and uncoupled from `tls`; gtk3/qt path-deps inherit
   defaults → their committed locks stay valid; the root lock gains
   `rcgen` (+ its `pem` dependency, ring reused); win7 manifests and
   `win7/Cargo.lock` are untouched; the win7 src mirror stays
   byte-identical under `#[cfg(feature = "lan")]` (inert on 1.75's
   undeclared feature); the authorized win7 divergence is feature
   availability only, recorded in the port commit.
7. **CI witness** (INT-1, REL-1): the Test (ubuntu-latest) job gains
   `cargo test -p vdm-rpc --features lan --locked`; the Clippy job
   gains `cargo clippy -p vdm-rpc --all-targets --features lan
   --locked -- -D warnings`; job display names unchanged (branch
   protection intact); the non-loopback LAN leg (bind wildcard, local
   non-loopback IP discovery via the UDP-connect trick, rustls client
   pinned to the generated cert, token matrix, plain-HTTP-to-TLS
   refusal, conn-cap behavior) lives in that feature-scoped suite;
   `scripts/e2e.sh`/`e2e-cross.sh` untouched (the loopback net already
   covers byte-identity).
8. **Bench** (REL-6): run the cheap A/B (`scripts/s8-bench-record.txt`,
   warm-pair rule) — bench-perf drives through the daemon RPC, so the
   loopback path gains one peer-classification branch per connection;
   parity expected, recorded honestly either way.
9. **Honesty rows** (REQ-5/7, REL-7/9): docs/RPC.md gains the LAN
   section AND reconciles the now-false "Wildcard (0.0.0.0) binds
   accept any IP literal" sentence; REQUIREMENTS-COVERAGE gains the
   Android remote row; android/README + android/docs/ARCHITECTURE gain
   the remote-mode section (incl. the dropped-fields table and the
   BT p/s limitation); S12-PROMOTION-GATES §1 Android "Core binding"
   cell is refreshed; README daemon section gains the LAN sentence;
   RUNBOOK gains "LAN mode" (token recovery/rotation, cert identity,
   portable-mode risk, bind failure modes, android-frontend witness
   policy); ROADMAP S8 flips to ACTIVE (W1 DONE; W2 pending its own
   gate) — never DONE; issue #5 is commented and stays open until W2.
   Owner-verification rows (D5:A class, NO test claimed): real-device
   TOFU flow, Doze/background reconnect, real-LAN pairing, on-device
   notification routing, API 26–28 TLS handshake.
10. **Local battery honesty:** Rust side — full local battery (fmt both
    trees, clippy `--workspace --all-targets -D warnings` scoped as
    established, workspace tests, `vdm-rpc --features lan` tests, 1.75
    witness `check --workspace --locked` in win7/, bench A/B). Android
    side — source review ONLY (no SDK in the dev sandbox; the
    android-frontend CI job is the sole compile + `:shared:jvmTest`
    witness). No record may claim a local Android build.
