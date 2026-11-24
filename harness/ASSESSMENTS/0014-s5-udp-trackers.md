# Assessment 0014 — S5 UDP trackers (gate record)

**Date:** 2026-11-24 (UTC). **Stage under review:** S5 "UDP trackers
(BEP-15)" (ROADMAP row S5, PENDING). **Change under review:** the
behavior spec `docs/SPECS/S5-UDP-TRACKERS-BEP15.md` (from-scratch
BEP-15 conformance; no S11 provenance tier applies).

**Panel:** five-role per PROTOCOL.md (ATKR / FALS / REQ / INT / REL),
dispatched independently with the identical evidence pack (the spec +
repository access). Agents saw no peer verdicts.

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | Source-address pinning absent (tid echo alone is not authentication — DHT precedent accepts any tid-matching datagram); mandate `UdpSocket::connect` + recv on the connected 5-tuple; connect retransmit must reuse the original tid. tid entropy wording wrong (DHT tids are a counter; the real helper is `util::random_bytes`). Minimum datagram lengths + length-checked slicing before any indexing. Tracker error text AND the URL reach RPC/logs unsanitized — strip C0/C1, cap URL rendering, byte-truncate before lossy conversion (char-boundary panic class). i32 interval/leechers/seeders semantics unpinned. Recv buffer unpinned (allocation + O(n²) dedup). No round-level budget (8 MiB magnet ⇒ thousands of tr=, hours unresponsive; announce_all has no stop flag). IPv6 dispatch self-contradictory. Non-unicast destinations (multicast/broadcast) accepted — LAN-wide BEP-15 emission primitive. magnet.rs already passes udp:// (record the verification). |
| FALS | **PROCEED-WITH-CONDITIONS** | **The spec fabricated a 4-byte error-code field — BEP-15's error reply is action+txid+message only.** The promised mismatched-tid test has no witness mode to drive it. IPv4-first vs first-connected-wins contradiction. Timeout bound not fully pinned (25 s/tracker worst case inside the synchronous announce loop). `dht:` failure string becomes a false claim post-S5. The engine no-tracker message promises a "trackers failed" distinction that site cannot make. Four docs go false at close (REQUIREMENTS-COVERAGE, IMPROVEMENTS, ALGORITHMS, TEST-REPORT). Verified-good: magic, connect/announce field order+sizes, event mapping, peers parsing, scrape out-of-scope, magnet no-change, win7 mirrors exist, bench script exists. |
| REQ | **PROCEED-WITH-CONDITIONS** | Win7 witness command as written proves nothing (`cargo check` compiles no tests; no rustup proxy exists — use the direct 1.75 toolchain with `--all-targets`). No consolidated deferred/non-goals section. Dual-stack selection rule divergent between two readings. Recv buffer + AnnounceResp field mapping (seeders→complete, negative handling) unpinned. Test matrix gaps: URL-invalid cases, BEP-07 error, silent-tracker timeout path (needs a timeout parameter for tests); existing `announce_all_aggregates` udp dummy becomes a real DNS hazard post-S5. Error literals unpinned (assert classification + substrings, not exact text). |
| INT | **PROCEED-WITH-CONDITIONS** | The win7 witness as written tests nothing (no `--all-targets`; CI win7 job builds binaries only). magnet.rs whole-file-copy mirror trap (main tree uses `is_multiple_of`, 1.87+). Existing test's `udp://x:80/ann` becomes a blocking DNS lookup + possible external datagram — change to portless `udp://x/ann` (parse-stage failure, zero I/O) in both trees. Dual-stack: UDP `connect()` is local — "yields a connected socket" discriminates nothing; skip V6 whenever V4 exists. Pin retransmit tid reuse + worst-case wall clock (≤ 20 s/tracker ceiling suggested); announce_all runs synchronously inside the engine monitor loop. Windows: WSAECONNRESET on connected UDP recv must classify as no-reply; tests bind literal 127.0.0.1. Verified-good: HTTP path preserved byte-for-byte below the changed failure strings (no test asserts them); no new deps needed; loopback UDP low-flake with the two pins. |
| REL | **PROCEED-WITH-CONDITIONS** | **BEP-15 conformance bug: no 4-byte error code field exists** (fix before the spec commit). Rollback semantics absent (define the C2 revert unit: bt/{tracker,engine}.rs + win7 mirrors; magnet.rs is a no-op). Release-integrity clause absent (push-main-only, no tags, release.yml/packaging untouched). Bench wording dangerous ("tags") — label arguments + `scripts/s5-bench-record.txt`; the bench exercises no announce path — the loopback battery is the real witness. Close checklist owed (ROADMAP flip, MEMORY, ASSESSMENTS, BACKUPS row, tracker-issue comment, backup LAST). Five doc truth flips named. magnet.rs verification result recorded. Gate number 0014 correct (next after 0013). |

## Consensus

Zero `BLOCK` → **consensus reached; the gate closes with this record.**

## Conditions → plan mapping

All conditions are folded into the revised spec
`docs/SPECS/S5-UDP-TRACKERS-BEP15.md` (committed in the same
activation commit as this record):

1. Error-reply shape corrected (FALS #1 / REL #1): action+txid+message,
   no code field.
2. Source pinning + tid rules (ATKR #1-2): connected socket, retransmit
   reuses tid/dgram, `util::random_bytes` explicit, non-unicast refusal.
3. Input hygiene (ATKR #3-7): minimum lengths, control-char sanitization
   of message AND URL, 2048 B recv buffer, i32 semantics pinned.
4. Round cap 32 trackers/round with skip notes (ATKR #8 / INT #5) —
   worst case ≤ the unbounded-HTTP worst case for the same magnet.
5. IPv6: V4-filtered candidates, error before any v6 datagram (ATKR #9 /
   FALS #3 / REQ #3 / INT #4).
6. Witness + test fixes: `--all-targets` direct-1.75 win7 witness
   (REQ #1 / INT #1), portless dummy in both trees (INT #3 / REQ #6),
   wrong-tid witness mode (FALS #2), timeout-parameterized announce fn,
   classification+substring assertions (REQ #7).
7. Integration truth: `dht:` failure string + engine message rewording
   (FALS #5-6); magnet.rs recorded no-op (ATKR #11 / REL #7).
8. Release posture: C2 revert unit, push-main-only, bench-record
   artifact, close checklist, five doc flips (REL #2-6).
9. Deferred/non-goals section added (REQ #2): scrape, BEP-07,
   connection-id caching, round-level stop responsiveness.
