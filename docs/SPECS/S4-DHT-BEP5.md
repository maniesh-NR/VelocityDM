# S4 spec — DHT (BEP-5) trackerless peer discovery

Behavior spec for roadmap stage S4. **This is a from-scratch BEP-5
conformance feature** (not an S11 adoption — no T1/T2/T3 provenance
tier applies; the standing spec-first + gate discipline does). Gate:
`harness/ASSESSMENTS/0008-s4-dht-activation.md` (zero BLOCK; conditions
folded into this text). Target: new module
`crates/vdm-core/src/bt/dht.rs` + wiring in `crates/vdm-core/src/bt/
engine.rs`. Existing surfaces reused: the strict `bt/bencode.rs`
module, `std::net::UdpSocket`, the engine's thread/poll patterns, and
the `max_peers` accounting of `bt/engine.rs`.

**Hard constraints:** no `ResumeState` change; no RPC schema change in
this stage (DHT internals are engine-internal); no new crate
dependencies; no unsafe code.

## Node identity

- A node id is 20 random bytes, generated on first DHT use and
  persisted at `<out_dir>/.vdm/dht-node-id` so the routing table keeps
  its reputation across daemon restarts. The file is atomic-write
  (temp+rename) and tolerant-read (missing/corrupt ⇒ regenerate).
- The engine's local UDP port is bound when the DHT starts; one socket
  per engine run (no fd churn).

## KRPC (BEP-5 §Message encoding)

- Messages are bencode dicts with `y` ∈ {`q`, `r`, `e`} and a 2-byte
  transaction id `t` (monotonic counter, wrapping). Queries carry `q`
  + `a`; replies carry `r`; errors carry `e: [code, message]` with our
  literal messages (201/202/203/204 per BEP-5).
- Queries: `ping` (`id`), `find_node` (`id`, `target`),
  `get_peers` (`id`, `info_hash`), `announce_peer` (`id`,
  `info_hash`, `port`, `token`, implied_port optional).
- Compact encodings per BEP-5: node = 26 bytes (20 id + 4 ip + 2
  port); peer = 6 bytes (4 ip + 2 port). Malformed values are
  dropped silently at parse (never panic).
- Outbound discipline (gate condition): outstanding transactions
  capped (64), single retransmit after 2 s, transaction abandoned
  after the retransmit; no unbounded retry loops.

## Routing table

- 160 buckets by XOR distance between our id and the remote id,
  K = 8 entries per bucket; the split-infinite-table degenerate case
  is avoided by capping total entries (1024) and treating overflow as
  ignore.
- Insertion: a live node (any valid reply) updates its freshness stamp
  and moves within its bucket LRU; a candidate for a full bucket
  replaces the least-fresh incumbent ONLY after that incumbent fails a
  ping (the poisoning guard). Node ids are untrusted input: length
  validated; our own id is never inserted; duplicates merge.
- Persistence: in-memory only (this stage); restart rebuilds via
  bootstrap + lookups. (A persisted table is a recorded deferred
  sub-stage, not smuggled in.)

## Tokens (announce_peer security, gate condition)

- Each node generates a random 64-bit secret rotated every 5 minutes;
  `get_peers` replies carry `token = HMAC-ish(secret, info_hash ||
  source ip)` (our own mix function, 20 bytes).
- `announce_peer` is honored ONLY with a token that (a) matches the
  current OR previous window's secret for that info_hash/source pair
  and (b) has not been used already (single-use ledger, capped). Bad
  tokens are rejected with error 203 (our literal text). Tokens are
  never logged.

## Lookup (peer discovery for an info_hash)

- Iterative `get_peers` with concurrency α = 3 over the closest known
  nodes: collect `values` (compact peers) and `nodes` (closer
  candidates); terminate when a round yields no new closer node, or
  after a depth cap of 20 rounds. Every returned peer lands in the
  engine's existing peer set (subject to `max_peers`; no duplicate
  addresses).
- After the lookup, send `announce_peer` to the K = 8 closest nodes
  THAT returned a token, with our announce port (this is the
  client-side discovery stage's announce; see deferred sub-stages for
  seeder parity).

## Bootstrap (runtime only, never in tests)

- Public bootstrap nodes (`router.bittorrent.com:6881`,
  `dht.transmissionbt.com:6881`, `dht.libtorrent.org:6881`): DNS
  resolved at runtime only; one bootstrap round per engine run, kicked
  lazily when the first BT task needs peers; rate-limited (no more
  than one round per run + one retry). CI and tests use ONLY the
  local mini-DHT (below) — the public network is never touched by the
  test suite (gate condition).

## Engine integration

- `bt/engine.rs` gains a DHT peer source running beside
  `announce_all`: for a magnet with zero resolvable trackers (or when
  all tracker announces fail), the DHT lookup supplies peers; with
  trackers present it runs in parallel (same peer set, dedup).
- **Idle behavior (a stage-level promise, tested):** with no BT task
  active the DHT does NOTHING — no socket churn, no background
  queries, no periodic refresh (the deferred refresh sub-stage below
  is the only thing that would ever change this, and it needs its own
  gate).

## Privacy statement (docs)

- DHT queries inherently reveal the info_hash to queried nodes —
  that is the protocol's design. This is stated in the user docs
  (`README`/`docs/BUILDING.md` notes) when the feature ships;
  nothing beyond the info_hash, our id, and a UDP port leaves the
  process (no names, no paths, no credentials).

## Deferred sub-stages (recorded, each needs its own gate)

- Seeder-side DHT announcing parity (`bt/seeder.rs` integration).
- Background routing-table refresh (find_node on stale buckets).
- Persisted routing table across restarts.

## Ports

- `win7/` tree port owed with MSRV-1.75-compatible syntax + local
  `cargo +1.75.0 check` witness (the module is pure-std: socket +
  bencode + threads — no 2021+edition-only constructs).

## Tests owed

- Unit: KRPC encode/decode roundtrip against literal BEP-5 fixtures
  (query/reply/error samples); token window rotation, single-use
  rejection; bucket ops (insert/LRU/replacement-after-ping-failure/
  own-id rejection/overflow ignore); XOR distance/bucket math; compact
  node/peer parse (malformed dropped).
- e2e (local mini-DHT): two in-process nodes over loopback UDP — node
  A holds the info_hash's peers; the engine, knowing only node A as
  its bootstrap, resolves peers via get_peers and completes a small
  torrent (byte-identical output). Runs on all three CI OSes.
- Idle no-op test: engine with no BT task ⇒ zero DHT sockets/queries
  (asserted by observing the DHT module's activity flag).
- No-regression: standard bench + full batteries green (labels
  `s4-pre`/`s4-post` recorded in the close entry).
