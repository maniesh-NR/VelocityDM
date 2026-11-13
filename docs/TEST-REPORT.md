# Final integration test report

Environment: Linux x86_64 sandbox, Rust stable 1.100.0 for the main tree;
Rust 1.75.0 compatibility guarded by the dedicated win7 CI job (checked
locally on stable; the win7 tree deliberately avoids post-1.75 APIs and
keeps conservative constructs — e.g. `io::Error::new`, `fetch_update`).
All numbers below are from the actual runs in this workspace on the
reconciled tree (2026-11-13, assessment 0002).

## Test suites (all green)

| Suite | Count | Scope |
| --- | --- | --- |
| vdm-core unit tests | 97 | token bucket, AIMD tuner, range pool, ctl files, HTTP head/body parsing, request building + header sanitization, HLS parsing + AES roundtrip, checksum, batch, events |
| vdm-core engine integration | 25 | full download pipeline against misbehaving in-process servers (14 HTTP, 5 HLS, 6 BitTorrent) |
| vdm-rpc | 4 | roundtrip + method set, SSE stream, **web-page request shapes refused**, host-allowlist matrix |
| full-stack E2E (real binaries) | 1 | daemon + CLI + NMH + SSE + limits + pause/resume + graceful shutdown in one scenario |
| extension (node) | 76 assertions | shared modules: detector, patterns, links, filters, messaging, state, **MD3 dynamic-color scheme** |
| shell e2e smoke | 24 checks | daemon, CLI add/checksum/batch/refresh, torrent create/seed/download, magnet, limits/scheduler/settings, SSE, NMH, RSS |
| shell e2e cross-feature | 27 checks | mixed HTTP+HLS+BT+magnet+batch+NMH load, RSS after load |
| **cargo total** | **127 tests** | |

## Security regression coverage (new in 0002)

- `web_page_request_shapes_are_refused`: `text/plain` cross-site POST
  (no-cors simple request) -> 415; Origin-bearing POST -> 403;
  Referer-bearing POST -> 403; rebound Host on `/events` and `/health`
  -> 403; missing Host (HTTP/1.0 style) -> 403; legitimate localhost
  spellings -> 200.
- `host_allowed_matrix`: 17 assertions over loopback / wildcard /
  explicit-LAN binds (IP literals ok, non-localhost hostnames refused).
- Outgoing request headers (per-download `options.headers`, user-agent):
  CRLF/NUL-bearing names or values rejected at emission.
- `daemon.shutdown` now exits the daemon process (verified end-to-end in
  the full-stack E2E: RPC shutdown -> clean exit code 0).

## Performance & memory (e2e runs, this session)

- daemon RSS idle/after-mixed-load: 6.3-7.4 MiB (well under the 200 MiB
  gate).
- No `.part`/`.vdmctl` leftovers after completed downloads (verified in
  e2e.sh and the full-stack test).
- Benchmarks: `scripts/bench.sh` (rewritten for the single-address CLI)
  measures 128 MiB segmented throughput, sustained rate-limited load,
  peak RSS, SSE event rate. Run on a quiescent machine for numbers worth
  recording here.

## Bugs found and fixed by this test series (cumulative)

1. **Token bucket seed order** — refill cap applied before the first seed
   (first grant could exceed 2x the burst).
2. **Retry offset corruption** — retry workers resumed at stale claim
   starts while the Range moved forward (holes in the file).
3. **Pause misread as completion** — mid-block pause returned Ok and
   marked unfinished blocks complete.
4. **Progress counter quantization** — progress advanced only on block
   completion; now marked per chunk.
5. **(0002) daemon.shutdown never exited the process** — engine stopped,
   daemon lived on; fixed via server wind-down + loop exit, regression in
   the full-stack E2E.
6. **(0002) web-page-reachable RPC** — see security coverage above.

## Known limitations (documented, tracked as issues)

No DHT (BEP-5); UDP trackers (BEP-15) detected then skipped; DASH
detect-only; HLS SAMPLE-AES and fMP4 EXT-X-MAP unsupported.
