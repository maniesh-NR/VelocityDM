# S5 spec — UDP trackers (BEP-15)

Behavior spec for roadmap stage S5. **From-scratch BEP-15 conformance
feature** (not an S11 adoption — no T1/T2/T3 provenance tier applies;
the standing spec-first + gate discipline does). Gate:
`harness/ASSESSMENTS/0014-s5-udp-trackers.md` (round 1: 5×
PROCEED-WITH-CONDITIONS, zero BLOCK; conditions folded into this
text). Primary source: the public BEP-15 "UDP Tracker Protocol for
BitTorrent" specification (bittorrent.org). Target:
`crates/vdm-core/src/bt/tracker.rs` (new UDP section) + dispatch
changes in `crates/vdm-core/src/bt/engine.rs`. Win7 tree mirror:
`win7/crates/vdm-core/src/bt/tracker.rs` (+ engine.rs) ported with
MSRV-1.75-compatible syntax (plain std UDP code; no new language
features required). `magnet.rs` is verified NO-OP for this stage
(see Integration).

**Hard constraints:** no new crate dependencies; no unsafe code; no
`ResumeState` change; no RPC schema change; no new config knob.

## URL parsing

- Tracker URLs with the `udp://` scheme dispatch to the UDP announce
  path inside `announce_all` (today they are rejected with
  "UDP trackers not supported" — that branch is replaced per this
  spec).
- Shape: `udp://host:port[/path]`. The path component is irrelevant
  to the protocol (BEP-15 routes by port); `/announce` and `/scrape`
  path suffixes are accepted and ignored. An absent port, a
  non-numeric port, or an unresolvable host is a classified
  `Error::Torrent` failure (`"udp tracker url invalid: <reason>"`).
- **Address selection (pinned):** resolve the host; filter the
  candidate set to IPv4; iterate V4 candidates in resolution order —
  the first V4 candidate is used. When the resolved set contains no
  IPv4 address (v6-only resolution, or a literal `udp://[::1]:…`
  URL), return the classified error
  `"udp tracker: ipv6 announce not supported"` BEFORE any datagram
  is sent (no socket is ever connected to an IPv6 target). The
  error string is a message, not a protocol claim (BEP-07 governs
  IPv6 *HTTP* announces; we simply do not do UDP-over-v6 yet).
- **Non-unicast destinations are refused at parse/connect time:**
  multicast (224.0.0.0/4, IPv6 ff00::/8), `255.255.255.255`,
  `0.0.0.0`, `::` ⇒ `"udp tracker url invalid: non-unicast target"`.
  Unicast loopback/private targets stay permitted (HTTP-path parity;
  tests need loopback).
- **Round cap (new bound, applies to both schemes):**
  `announce_all` processes at most the **first 32** tracker URLs per
  announce round; excess entries are skipped with a failure note
  `"<url> (excess trackers skipped: round cap)"`. Rationale: the
  pre-HTTP path had no cap and an 8 MiB magnet body can carry
  thousands of `tr=` entries; 32 × 20 s worst case ≤ the unbounded
  HTTP worst case for the same magnet. This is an engine-behavior
  change recorded in the stage record (the HTTP announce path
  itself is byte-identical below the cap).

## Protocol (BEP-15)

All integers are network byte order (big-endian); the spec text
below states sizes, the implementer follows BEP-15.

- **Connected-socket source pinning (primary authenticity control):**
  the client binds an ephemeral UDP socket and `connect()`s it to
  the resolved tracker address; connect AND announce responses are
  received ONLY on that connected 5-tuple (the OS drops datagrams
  from any other source). Transaction-id randomness is
  defense-in-depth, not the primary control.
- **Transaction ids:** 4 bytes from `crate::util::random_bytes` per
  request. Explicitly NOT the DHT module's sequential counter
  (`alloc_tid`). The connect retransmit reuses the ORIGINAL
  transaction id and the identical datagram (BEP-15 practice);
  late/duplicate replies are idempotent (the first valid response
  wins, subsequent duplicates are ignored).
- **Connect handshake**: 16-byte request = 8-byte protocol constant
  `0x0000041727101980`, action `0`, transaction id. Response = action
  echo (0), transaction echo, 8-byte connection id (minimum length
  16 bytes). Mismatched action or transaction id ⇒ classified error.
- **Announce**: action `1`; body = connection id, info_hash (20),
  peer_id (20), downloaded u64, left u64, uploaded u64, event i32
  (0 none / 1 completed / 2 started / 3 stopped — mapped from the
  existing `TrackerEvent` exactly as the HTTP path maps it), source
  ip u32 (0), key u32 (0), num_want i32 (-1), port u16 (the existing
  `announce_port`). Response (minimum length 20 bytes) = action
  echo (1), transaction echo, interval i32, leechers i32, seeders
  i32, then the compact 6-byte peer list (4 ip + 2 port each).
  **Field mapping:** seeders → `AnnounceResp.complete`, leechers →
  `.incomplete` (negative ⇒ `None`), interval i32 → u64 saturating
  (missing ⇒ 1800, matching the HTTP default). interval/leechers/
  seeders are parsed but UNUSED for scheduling this stage (parity
  with the HTTP path, which also parses then drops them). Peer list
  parsing mirrors the HTTP compact path exactly: length-multiple-of-6
  checked BEFORE any indexing; a truncated peer tail (not a multiple
  of 6) is a classified error, never silently accepted.
- **Error replies**: action `3`, transaction echo, then the text
  message (there is NO error-code field in BEP-15 — the message
  starts at byte 8). Surfaced as
  `Error::Torrent("udp tracker error: <message>")`.
- **Timeouts (pinned, gate condition):** per-tracker wall clock
  ≤ 20 s: connect wait 1 = 5 s; on timeout exactly ONE retransmit
  (same datagram, same transaction id), connect wait 2 = 5 s;
  announce wait = 10 s. No unbounded retry loops; a tracker that
  misses the bound is failed for this round with a classified
  message. A connection id lives for the duration of one announce
  round only (no caching across rounds — recorded deferred
  sub-stage, not smuggled in).
- **Receive bound:** the response recv buffer is **2048 bytes**
  (the DHT loop precedent), ⇒ ≤ 341 peers per response; larger
  swarm replies truncate at the buffer and the peer tail lands in
  the classified truncated-tail error path.
- **Windows realities:** a connected UDP socket that sent to an
  unbound port surfaces `WSAECONNRESET` on the next recv —
  classified as "no reply" (retransmit/failure path), never a
  panic. Tests bind/connect literal `127.0.0.1`, never `localhost`
  (which can resolve to `::1`).
- **Scrape (action 2)**: not implemented (recorded non-goal;
  nothing sends scrape requests).

## Input hygiene (untrusted tracker data)

- **Minimum datagram lengths before any slicing:** connect ≥ 16 B,
  announce ≥ 20 B, error ≥ 8 B; all field extraction uses
  length-checked slicing (`get()`, checked chunks — the
  modulo-check-BEFORE-indexing pattern the HTTP compact path uses).
  A short datagram ⇒ classified error, never a panic.
- **Control-character sanitization:** both the tracker-supplied
  message AND the tracker URL are sanitized (C0/C1 + DEL stripped,
  modeled on the engine's existing control-char helpers) before
  they enter any `Error`/failure string (which reaches RPC replies,
  task notices, and logs). The URL portion is additionally capped
  at 256 rendered bytes. Truncation happens on BYTES before
  `from_utf8_lossy` (never slice a `String` mid-char).
- **Entropy note:** `util::random_bytes`' Windows fallback is
  clock-seeded — acceptable here because source pinning (not tid
  unpredictability) is the authenticity control.

## Integration

- `announce_all` dispatches by scheme: `udp://` ⇒ the new UDP
  announce, `http(s)://` ⇒ the existing HTTP announce unchanged,
  `dht:` ⇒ not a tracker — its failure string is updated to an
  accurate `"dht:... (not a tracker; DHT handled separately)"`
  (the old string claimed UDP is unsupported, which becomes false).
- The engine's pre-announce no-tracker message (bt/engine.rs,
  currently "no HTTP trackers available (UDP/DHT not supported)")
  fires only when `trackers.is_empty() && dht.is_none()` — reworded
  to `"no trackers configured"` (that site cannot know "failed";
  the failed-trackers message already exists at the empty-peers
  site via `failures.join`).
- `magnet.rs`: **verified no change needed** — parse keeps `udp://`
  entries unfiltered (the existing `parse_full` test asserts a
  `udp://` tracker survives), so the implementing commit carries no
  speculative widening and no magnet.rs diff at all.
- `velocitydm seed`'s TrackerServer registration is untouched (the
  in-process HTTP tracker keeps serving tests/seed).

## Tests (loopback only, no external network)

- In-process UDP tracker test server (mirrors the existing
  `TrackerServer` harness pattern): one thread + one `UdpSocket`
  bound to literal `127.0.0.1:0`, speaking minimal BEP-15 with modes:
  (a) drop-first-datagram (retransmit witness), (b) short reply
  (malformed witness), (c) action-3 error reply (error witness),
  (d) wrong-transaction-id reply (mismatch witness).
- Cases: connect+announce happy path (peers + interval + complete/
  incomplete mapped); retransmit-after-drop succeeds within the
  bound; short response ⇒ classified error; error reply ⇒ message
  surfaced (sanitized); mismatched transaction id ⇒ error;
  truncated peer tail ⇒ classified error; URL-invalid cases
  (portless URL `udp://host/ann`, non-numeric port, non-unicast
  target) fail with zero network I/O; v6-only resolution ⇒ the
  ipv6-not-supported error; `announce_all` mixed list (one UDP + one
  HTTP loopback tracker) merges peers with both counted OK; the
  round cap skips excess entries.
- Test assertions are classification + distinctive-substring checks,
  not exact full-text matches.
- Existing-test fix: `announce_all_aggregates`' deterministic
  failure entry `udp://x:80/ann` becomes a portless `udp://x/ann`
  (parse-stage failure, zero DNS/network I/O) in BOTH trees.
- Win7 witness: the direct 1.75 toolchain (no rustup proxy exists —
  RUNBOOK) runs `cargo check --manifest-path win7/Cargo.toml
  --workspace --all-targets --locked` inside `win7/` (lib+bins+tests
  compile; the CI win7-payload job builds release binaries only —
  compile witness, no test execution, stated honestly). No
  whole-file copies into the win7 tree (MSRV trap: the main tree
  already carries post-1.75 APIs like `is_multiple_of`); the mirror
  is a hand port of the tracker/engine deltas only.

## CI / bench / release posture

- No new CI job (covered by the required Test matrix).
- `scripts/bench-perf.sh` A/B with the **label arguments**
  `s5-pre`/`s5-post` (not git tags) recorded to
  `scripts/s5-bench-record.txt`; no-regression = no
  elapsed/connections/requests regression + full batteries green.
  (The bench exercises the HTTP segmented engine; the announce
  paths are witnessed by the loopback battery — stated honestly.)
- **Release posture:** push-main-only; NO git tags created or moved
  (v* ruleset; the six frozen v1.0.0-family releases stay frozen);
  no `release.yml` / `packaging/**` edits; root `Cargo.lock`
  untouched (no-new-deps); `Cargo.toml` versions untouched.
- **Commit split:** C1 = activation (spec + gate record 0014);
  C2 = the single revertable implementation commit scoped to
  `crates/vdm-core/src/bt/{tracker.rs,engine.rs}` +
  `win7/crates/vdm-core/src/bt/{tracker.rs,engine.rs}` (+ the
  existing-test fix inside those files); C3 = close docs (ROADMAP
  S5 flip, MEMORY entry, BACKUPS row, tracker-issue comment);
  C4 = backup LAST (`scripts/backup-sandbox.sh`), zero dirty files.
  Rollback = `git revert` of C2 (revert-only; no data/config/schema
  migration exists).
- **Doc truth flips at close:** `docs/REQUIREMENTS-COVERAGE.md`
  (UDP trackers row), `docs/ALGORITHMS.md` (UDP future-work row —
  already stale re DHT), `docs/IMPROVEMENTS.md` (skipped-trackers
  row), `docs/TEST-REPORT.md` (detected-then-skipped row), and the
  README BT row gains "UDP (BEP-15)".

## Deferred sub-stages / non-goals (recorded, each needs its own gate)

- UDP scrape (action 2).
- BEP-07 / UDP-over-IPv6 announce.
- Connection-id caching across announce rounds.
- Round-level stop-flag responsiveness inside `announce_all`
  (the 32-tracker cap bounds the exposure; a stop-aware budget is a
  separate change).
