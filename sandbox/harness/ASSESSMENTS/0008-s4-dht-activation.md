# Assessment 0008 — S4 activation: DHT (BEP-5) (gate record)

**Date:** 2026-11-21 (UTC). **Change under review:** activation of
roadmap stage **S4 — DHT (BEP-5) so trackerless magnets resolve**,
activated in PARALLEL with S11-i wave 2 per the owner's decision round 1,
D2:**B** (`harness/DECISIONS.md`). This is a from-scratch BEP-5
conformance feature (NOT a study adoption — no S11 provenance tier
applies; the standing spec-first + gate discipline does). **Panel:**
five-role per PROTOCOL.md (ATKR / FALS / REQ / INT / REL), dispatched
independently with the same evidence pack (the BEP-5 spec draft +
repository access + the engine's existing BT architecture:
`bt/tracker.rs` HTTP announce, `bt/engine.rs` announce_all wiring,
`bt/magnet.rs` parse, `bt/bencode.rs`, `bt/peer.rs`, `bt/seeder.rs`).

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | Token discipline is the security core: `announce_peer` MUST require a token previously returned by that same node in a `get_peers` reply, tokens expire (5-minute window) and are single-check (reject replays), and the node regenerates its token secret per window; without this any third party can forge announces to our node or OUR node can be used to spam others. Routing-table poisoning: never insert/replace a bucket entry without a live ping verification (candidate replacement only after the incumbent fails a ping); node ids are untrusted input — validate 20-byte length, reject our own id. Query amplification: bound outstanding KRPC transactions (cap), retransmit at most once, 2 s timeout; bootstrap is rate-limited (one round per engine run + on-demand only when a BT task actually needs peers). Info-hash privacy: DHT queries inherently reveal the info_hash to queried nodes (protocol design) — this MUST be stated in the user docs, not discovered by the user; nothing beyond the hash leaves the process (no names, no paths). UDP socket: bind one local socket per engine; no unsafe code. |
| FALS | **PROCEED-WITH-CONDITIONS** | The stage claim "trackerless magnets resolve" must be DEMONSTRATED in-repo, not asserted: a two-node local mini-DHT e2e over loopback UDP (node A holds the info_hash's peers; the engine, knowing only node A as bootstrap, resolves peers and completes a small torrent) — the CI battery runs ONLY against the local mini-DHT; the public DHT and public bootstrap nodes are NEVER touched by tests (flaky, external, and it would pollute shared infrastructure — an honesty rule, documented in the test). Falsifiability of conformance: KRPC byte-shapes must roundtrip against literal BEP-5 fixtures (query/ reply/error samples in the tests). The idle claim must be measured: with no BT task active the DHT does NOTHING (no socket churn, no background queries) — asserted by a test, because "resource use" is a stage-level promise. |
| REQ | **PROCEED-WITH-CONDITIONS** | spec-first: `docs/SPECS/S4-DHT-BEP5.md` commits BEFORE any implementing commit (this activation commit); scope honesty: this stage is PEER DISCOVERY only (KRPC + routing table + integration as a peer source beside `announce_all`); seeder-side announcing and long-run routing-table maintenance beyond the lookup path are explicitly deferred sub-stages recorded in the spec; win7 port owed (bt tree exists in `win7/`, 1.75-compatible syntax, local `cargo +1.75.0 check` witness); no `ResumeState` change; no RPC schema change in this stage (DHT internals are engine-internal; a status surface can be additive LATER); tracked as a `v1.1.0 - roadmap` milestone issue (#17 if not yet filed — file it with the activation commit so "tracked as issue" is true at commit time); docs (`docs/ARCHITECTURE.md`/`RPC.md` untouched; `README` feature list gains the trackerless-magnet line ONLY when the implementation ships). |
| INT | **PROCEED-WITH-CONDITIONS** | No new dependencies: KRPC rides the existing strict `bencode` module and `std::net::UdpSocket` (nonblocking + the engine's existing thread/poll patterns — no tokio); node id = 20 random bytes (existing rng usage in bt engine peer_id — reuse it); bootstrap hostnames (`router.bittorrent.com:6881`, `dht.transmissionbt.com:6881`, `dht.libtorrent.org:6881`) resolve at RUNTIME only, never in CI (CI uses the local mini-DHT exclusively); the CI matrix runs the mini-DHT e2e on all three OSes (loopback UDP is portable); the daemon currently announces a port to trackers — the DHT equivalent uses the same local UDP port concept; keep the engine's single-socket-per-run shape so no fd leaks; the two trees (root + win7) build green on the required checks — fmt/clippy `-D warnings` across both. |
| REL | **PROCEED-WITH-CONDITIONS** | activation docs commit (this gate + spec + ROADMAP + DECISIONS + MEMORY) pushed FIRST; implementation = ONE revertable commit set (module + tests + win7 port) with its own CI witness (all 8 required checks green at the pushed HEAD; non-required frontend jobs observed); A/B bench: a NEW capability has no pre/post A/B meaning — the honest witnesses are the mini-DHT e2e, the idle-no-op test, and NO-REGRESSION on the existing batteries + standard bench (labels `s4-pre`/`s4-post`); failure policy: fix-forward or revert; no tag cuts; no release actions; backup closes the activation (and separately the implementation close) per Law 1. |

## Consensus

Zero `BLOCK` → **the gate closes with this record.** Conditions folded
into `docs/SPECS/S4-DHT-BEP5.md` and the activation plan below.

## Conditions and how each was folded

1. **Token discipline** (ATKR-1) → spec §tokens: per-node random secret,
   5-minute rotating window, single-use check, required on
   `announce_peer` from the same node.
2. **Bucket hygiene** (ATKR-2) → spec §routing: replacement only after
   incumbent ping failure; id length validated; own id never inserted.
3. **Query budget** (ATKR-3) → spec §transport: transaction cap,
   single retransmit, 2 s timeout; bootstrap one round per run,
   on-demand.
4. **Privacy statement** (ATKR-4) → spec §privacy + docs note: queries
   reveal the info_hash by protocol design; nothing else leaves.
5. **Local mini-DHT only in CI** (FALS-1) → spec §tests: two-node
   loopback e2e; public network never touched by tests.
6. **BEP-5 fixtures** (FALS-2) → spec §tests: KRPC roundtrip against
   literal fixtures.
7. **Idle no-op demonstrated** (FALS-3) → spec §idle + test: no BT
   task ⇒ no DHT activity.
8. **Spec-first + issue + docs truth** (REQ) → this activation commit
   includes the spec; issue filed; README line deferred to
   implementation ship.
9. **win7 port + witness** (REQ) → spec §ports.
10. **No new deps; loopback CI on all OSes** (INT) → spec §transport +
    §tests.
11. **Revertability + witnesses + honest A/B framing** (REL) → plan
    below.

## Recorded (not fixed this session)

- Implementation runs as the parallel track to S11-i wave 2 (owner
  D2:B), same session cadence: spec-first (this commit) → module +
  tests + win7 port commit → batteries + bench witness → CI witness →
  issue comment → backup close.
- Seeder-side DHT announcing and routing-table background refresh are
  deferred sub-stages (recorded in the spec), not silently dropped.
- S5 (UDP trackers, BEP-15) remains PENDING with its own future gate.
