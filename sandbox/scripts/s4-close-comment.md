**S4 (DHT, BEP-5) — DONE.** Close-out record for the trackerless-magnet stage.

**Implementation** (per gate 0008 + the committed behavior spec `docs/SPECS/S4-DHT-BEP5.md`, spec-first):

- `crates/vdm-core/src/bt/dht.rs` (+ win7 port, byte-identical): KRPC codec (query/reply/error, 201–204 literals), 160-bucket XOR routing table (K=8, 1024-entry cap, LRU + replace-after-ping-failure poisoning guard), 5-minute-window token secrets with a single-use ledger, iterative `get_peers` (α=3, depth cap 20), `announce_peer` to the K closest token-holders, persisted node id (`.vdm/dht-node-id`, atomic write, tolerant read).
- Engine wiring: DHT peer source runs beside trackers — trackerless magnets resolve peers + BEP-9 metadata; idle behavior is a tested no-op (no socket churn with no BT task).
- e2e: two-node loopback mini-DHT — the engine, knowing only node A as bootstrap, completes a trackerless magnet byte-identically; idle no-socket witness. Public bootstrap touches the real network at RUNTIME only (tests use the local mini-DHT only; `VDM_DHT_BOOTSTRAP=off` guards the e2e harnesses).
- Docs: README trackerless-magnet line, BUILDING privacy note (DHT queries reveal the info_hash to queried nodes — the protocol's design; nothing else leaves the process).

**CI fix-forward (honest record):** the first push (126de3a) went RED on `Test (windows-latest)` — `token_rotation_single_use` failed. Root cause: `util::random_bytes`'s xorshift fallback (no `/dev/urandom` on Windows) seeded with `now_ms ^ pid ^ heap-addr` — the Windows heap re-serves the same address for same-size blocks and two token-secret draws landed in one millisecond ⇒ identical secrets ⇒ a spent token re-matched after rotation. Fix (9fc8e42, both trees): per-process atomic call counter in the fallback seed + regression tripwire; verified by an arithmetic worst-case proof (old seed collides, new seed distinct).

**Witnesses:** CI **11/11 green at 9fc8e42** (windows-latest Test now passes). Local: fmt + clippy `-D warnings` both trees, 137 vdm-core lib tests + integration suites, win7 `cargo +1.75.0 check --workspace --locked`. **Bench A/B (spec-owed, labels s4-pre/s4-post):** warm-pair parity — pre 0.81 s / 159.0 MBps vs post 0.81 s / 158.9 MBps, request shape identical (12 conns / 37 reqs), byte-identical output — **no regression**.

Deferred sub-stages (each needs its own gate): seeder-side DHT announce parity, background routing-table refresh, persisted routing table. ROADMAP S4 flipped to DONE at b34eb4d; `harness/MEMORY.md` session-13 entry records the close.
