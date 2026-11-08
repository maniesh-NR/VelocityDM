# Algorithms

## Work-stealing range pool (HTTP)

The remaining file is a set of disjoint byte intervals. A worker asks for a
lease taken from the **high end of the largest** remaining interval, so an
early-finishing connection immediately splits the biggest piece left (XDM's
acceleration model). On pause/error the unconsumed tail is released back and
merged. Resume restores the pool from the saved intervals after
ETag/Last-Modified validation. Coverage property tested:
`many_workers_drain_exactly`.

### Guided lease sizing

The lease size is not fixed: `lease = clamp(remaining / workers, 512 KiB,
16 MiB)` — guided self-scheduling (Polychronopoulos & Kuck) as recommended
for parallel downloading by Koo et al. Large leases early amortize each
request's RTT over more bytes; the schedule shrinks geometrically toward the
end so every worker shares the tail instead of one grinding a large final
chunk (the straggler problem). The connection count itself is capped by file
size (`>= 1 MiB per connection`) so small files never pay setup-dominated
parallelism (Koo et al.: efficiency ~ S / (S + N*C)).

### Persistent connections

Workers keep their keep-alive `Conn` **across leases** and recycle it via
`Response::into_conn()` after every cleanly drained body; a TCP/TLS
handshake is paid only on the first lease, on mirror switches, or after
errors. A request that fails as the *first* request on a recycled connection
is the classic server idle-timeout: it is retried once, free, on the same
mirror. The HLS segment fetchers do the same per worker (with origin
tracking so a connection is never reused against a different host).

### Throughput-weighted mirror selection

Rodriguez et al.'s parallel mirror access: workers start spread round-robin
across mirrors, and each completed lease feeds a per-mirror throughput EMA
(bytes/s, 1/8 decay). At lease boundaries (and on reconnects) workers switch
to the best-scoring mirror; the score is `speed_ema / 4^consecutive_fails`,
and failed mirrors enter a 2-8 s cooldown. One transient failure barely
dents a much-faster mirror; consecutive failures dominate (anti-flap).

## AIMD block sizing

Each worker keeps a burst window `w` (256 KiB start). Clean progress grows
`w` additively (+32 KiB, capped at 1 MiB); a read error halves it (min
64 KiB) — the classic additive-increase/multiplicative-decrease shape from
TCP congestion control (Padhye's model for the loss-rate ceiling; we run
AIMD at the application layer over each lease). AIMD state persists across
leases on the same worker, matching the steady-state per-flow behavior the
Padhye model describes. Between bursts the worker re-checks pause/stop
flags, keeping cancellation latency low without busy-waiting.

### Paced writes

Bodies are paced and written in <= 64 KiB chunks, re-checking pause/stop
between chunks: under a hard rate limit a full-window token take can block
for seconds inside the bucket, and pause must stay responsive (~250 ms).
Writes use positioned `pwrite`-style I/O (`write_all_at` /
`seek_write`) — one syscall, no cursor, shorter file-lock hold.

## Token-bucket rate limiting

Two levels: a per-task bucket (options) and one global bucket shared by all
tasks (XDM "global speed limiter"). Before writing `n` bytes a worker takes
`n` tokens from both buckets; deficits sleep at ≤250 ms granularity with a
256 KiB burst headroom so short transfers are not starved. Live rate change
(`limits.set`) affects new takes immediately.

## Priority gate (queue)

Admission control: at most `max_active` tasks hold slots; waiters are
admitted by `(priority desc, arrival asc)` using a condvar broadcast after
each release. The scheduler thread applies the daily stop-window (pause
active, auto-resume after the window — never user-paused tasks) and fires
queue-drained actions (run command / shutdown).

## HLS assembly

1. Master playlist parsed → variants; pick best/worst/nearest-bandwidth.
2. `#EXT-X-KEY` tracked per segment; keys fetched once and cached.
3. 4 workers fetch segments concurrently; a reorder map + ordered writer
   appends to the `.part`, renaming to `<name>.ts` at the end.
4. AES-128-CBC per segment with the playlist IV or the media-sequence
   number as the 16-byte big-endian default IV; strict PKCS7 validation.

## BitTorrent

- **Bencode** with raw-span extraction so `SHA-1(info)` matches other
  clients byte-for-byte even for non-canonical key order.
- **Piece store**: availability counts per connected peer; picker prefers a
  partial piece (resume-friendly), then rarest-first among needed pieces the
  peer has (sequential mode available). Per-file priorities mark pieces
  unneeded when every overlapping file is skipped; boundary pieces shared
  between selected/skipped files are downloaded (their skipped-side bytes
  are written, which is exactly what the protocol requires).
- **Peer loop**: handshake → interested → pipelined requests (≤8 blocks of
  16 KiB outstanding) → blocks assembled into pieces → SHA-1 verify →
  positioned writes across the multi-file layout.
- **Magnet**: BEP-10 extension handshake → BEP-9 `ut_metadata` chunked
  download of the info dict → verify `SHA-1(info) == info_hash` → proceed.
- **Failover**: peer threads die independently; partial pieces stay in the
  assembly buffer so other peers continue them. Corrupted pieces are dropped
  and re-requested (counted).
- **Seeding** (`velocitydm seed`): in-process HTTP tracker + full bitfield
  seeder that also serves metadata to magnet clients.

## HTTP client details worth knowing

- Response head is read with a bounded scan; any bytes read past the
  terminator become the body "prelude" and are served through the same
  accounting as socket bytes (fixes the classic swallowed-body bug).
- `Content-Length` and chunked framing are both handled; connection reuse
  only after a clean, fully-drained body.
- Relative URL resolution (playlists, redirects) implements RFC 3986
  section 5.2.4 dot-segment removal, so `../key.bin` references in real
  CDN playlists resolve to the right path.
- Proxies: HTTP CONNECT tunnels for TLS origins, absolute-form requests +
  Basic `Proxy-Authorization` for plain HTTP, SOCKS5 with optional
  user/password.

## Research notes (papers behind the scheduling choices)

The download engine's scheduling follows the parallel-download literature:

1. **Padhye, Firoiu, Towsley, Kurose. *Modeling TCP Throughput: A Simple
   Model and its Empirical Validation.* SIGCOMM 1998.** Per-connection
   throughput is bounded by RTT and loss; parallel connections multiply the
   effective window. Basis of our per-worker AIMD burst sizing (state kept
   per persistent worker = per steady-state flow).
2. **Rodriguez et al. *Parallel-Access for Mirror Sites in the Internet.***
   Splitting a file across mirrors in parallel and **reassigning ranges
   based on measured per-mirror throughput** removes the slow-mirror
   straggler. Implemented as round-robin spread + throughput-EMA scoring +
   cooldown (see above).
3. **Koo, Rosenberg et al. *Analysis of Parallel Downloading for Large File
   Distribution.*** Chunk count/size trades per-request setup overhead
   against load imbalance; guided (remaining-aware) allocation beats static
   splits. Implemented as guided lease sizing + size-capped connection
   counts.
4. **Polychronopoulos & Kuck. *Guided Self-Scheduling.*** `chunk =
   remaining / P`, clamped — the exact form of our lease sizing, including
   the geometric endgame shrink.
5. **Manzoor et al. *The Curious Case of Parallel Connections in HTTP/2*
   (2016).** A single multiplexed HTTP/2 connection suffers TCP-level
   head-of-line blocking; parallel TCP connections measurably outperform it.
   This validates the engine's HTTP/1.1 + N parallel connections design
   over an HTTP/2 multiplexing rewrite.
6. Parallel-TCP measurement studies: a synchronized burst of connection
   setups induces loss on constrained links (SYN bursts overwhelm
   middleboxes/queues). Workers beyond 8 start staggered (15 ms apart,
   capped 300 ms).

### Measured effect (A/B, `scripts/bench-perf.sh`)

128 MiB over 8 connections, localhost with simulated WAN latency (every
request pays RTT; fresh connections pay an extra handshake RTT):

| RTT | Baseline (fixed 4 MiB leases, per-lease reconnect) | Optimized | Gain |
| --- | --- | --- | --- |
| 40 ms | 1.41 s (91.1 MB/s), 34 TCP conns | 0.81 s (158.9 MB/s), 10 conns | 1.74x, -70% conns |
| 250 ms | 3.21 s (39.9 MB/s), 34 TCP conns | 2.61 s (49.1 MB/s), 10 conns | 1.23x, -70% conns |

Byte-identical results in all runs. On zero-RTT loopback the two designs
converge (handshakes are free), which is exactly why the persistent-
connection design matters on real networks and costs nothing locally.

## Not implemented (documented limits)

- UDP trackers (BEP-15) and DHT (BEP-5): magnets require `&tr=` HTTP
  trackers; UDP/DHT is future work.
- Live HLS playlists without `#EXT-X-ENDLIST` are rejected.
- Endgame mode and super-seeding for BitTorrent.
