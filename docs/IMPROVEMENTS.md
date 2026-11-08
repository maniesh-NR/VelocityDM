# Improvements adopted from XDM, Motrix and Gopeed

This rebuild of VelocityDM studied three open download managers and adopted
their best ideas. Every item below is implemented in this tree; the table
maps each idea to its origin and where it lives in our code.

## From Xtreme Download Manager (XDM) — https://xtremedownloadmanager.com

| Idea (as XDM presents it) | VelocityDM implementation |
| --- | --- |
| Dynamic segmentation: "splits the biggest remaining piece so no connection sits idle" | `vdm-core/src/range_pool.rs` — work-stealing leases from the largest remaining interval; workers re-acquire on completion (`engine/task.rs::worker`) |
| "Refresh link revives expired links" (resume without starting over) | `Engine::refresh` (`engine/mod.rs`) + `downloads.refresh` RPC + `velocitydm refresh ID --url NEW`; keeps the original filename and `.part` progress |
| Scheduler & speed limiter: start/stop at set times, cap speed, shut down when done | `engine/scheduler.rs` (daily stop-window, queue-drained actions) + `TokenBucket` global + per-task limits + `shutdown-when-done`/`on-queue-drained` |
| Batch download: paste hundreds of links or a pattern | `batch.rs` pattern expansion (`[001:120]`, `[1,4,7]`) + `downloads.addBatch` RPC + extension page-link grabber ("Grab links") |
| HLS/DASH stream capture with quality picking | `engine/hls.rs` (master playlist, best/worst/bandwidth variant choice, AES-128) + extension video sniffing with quality labels |
| "Works with your browser ... downloads are captured automatically with the cookies the site needs" | Extension `downloads.onCreated` takeover pauses→hands URL+cookies+referer→cancels browser download; falls back gracefully when the host is down |
| Native app, no Electron, ~30 MB idle memory | Rust + egui; measured daemon RSS ≈ 6 MB (headless), GUI ≈ 15 MB |
| Adjustable connections per download, automatic retries | `--conn N` (1–32), worker retry with exponential backoff + mirror rotation |

## From Motrix — https://motrix.app

| Idea | VelocityDM implementation |
| --- | --- |
| BitTorrent with per-file selection | `bt/pieces.rs` file priorities (skipped files only receive boundary-piece bytes) + `torrents.files` RPC for pre-add inspection + `--files 0,2` |
| Magnet link support (fetch metadata, then pick files) | `bt/engine.rs::fetch_metadata` — BEP-10 extension handshake + BEP-9 `ut_metadata` exchange, then normal download |
| Built-in tracker list management | `.torrent` announce-list preserved; magnets accept multiple `&tr=` entries; `create_torrent` emits `announce-list`; UDP trackers are reported and skipped (documented) |
| Seeding / sharing from the app | `velocitydm seed FILE` starts tracker + seeder and prints a ready-to-share magnet; `torrent-create` from files or directories |
| Clean task UI with categories and progress | MD3-dark egui list (state chips, progress bars, peers/seeds) and the Compose app on Android |
| Pause/resume that survives crashes | `.part` + `.vdmctl` interval state for HTTP; `.vdmctl` have-set for BitTorrent; both verified by integration tests |

## From Gopeed — https://gopeed.com

| Idea | VelocityDM implementation |
| --- | --- |
| Multi-protocol surface (HTTP/HTTPS/BT/magnet) behind one CLI | One daemon + one JSON-RPC surface; `velocitydm add|torrent|magnet|batch|seed` |
| Browser extension for every desktop browser | Same code base packaged for Chrome/Edge/Opera (MV3 service worker) and Firefox (event page), Safari conversion guide |
| Lightweight, fast startup | No Electron, no JVM for the engine: static musl build ~3 MB (CLI trio), 14 MB GUI binary; daemon cold-start < 100 ms |
| Open, scriptable core | JSON-RPC 2.0 + SSE (`/rpc`, `/events`, `/health`) on loopback only; NMH protocol documented for other clients |
| Create torrents / share files | `torrent-create` (file or directory, progressive piece size) |

## From the research literature (final performance round)

| Paper | Adopted as |
| --- | --- |
| Rodriguez et al., *Parallel-Access for Mirror Sites in the Internet* | Throughput-weighted mirror selection: round-robin spread, per-mirror speed EMA, 4^fails penalty, cooldown, lease-boundary reassignment |
| Koo et al., *Analysis of Parallel Downloading for Large File Distribution* | Guided lease sizing (`remaining/workers`, clamped 512 KiB–16 MiB) and size-capped connection counts (≥ 1 MiB per connection) |
| Polychronopoulos & Kuck, *Guided Self-Scheduling* | The exact `remaining/P` lease form with geometric endgame shrink |
| Padhye et al., *Modeling TCP Throughput* (SIGCOMM'98) | AIMD burst sizing kept per persistent worker (steady-state flow); already the basis, now with persistent connections matching the model |
| Manzoor et al., *The Curious Case of Parallel Connections in HTTP/2* | Validates HTTP/1.1 + N parallel connections over an HTTP/2 multiplexing rewrite (TCP head-of-line blocking) — design kept |
| Parallel-TCP measurement studies (SYN bursts) | Staggered worker starts beyond 8 connections (15 ms apart, capped) |
| RFC 3986 §5.2.4 | Dot-segment normalization in relative URL resolution (fixes real CDN playlists with `../key.bin` key URIs) |

Measured effect (`scripts/bench-perf.sh`, 128 MiB / 8 conns, simulated
WAN RTT): 1.74× faster at 40 ms, 1.23× at 250 ms, 70% fewer TCP
connections, byte-identical output. Plus: paced ≤ 64 KiB writes keep
pause latency ~250 ms even under hard rate limits.

## Deliberately not adopted

- **ed2k support** (Gopeed): the eDonkey network is effectively dead; would
  add a server protocol for no practical use. Documented as out of scope.
- **Electron UI** (Motrix): XDM's native-first argument wins; egui keeps the
  binary at 14 MB with no web view.
- **Subscription/licensing** (XDM 9): VelocityDM stays GPL-3.0-or-later with
  no paid tiers.
- **Mid-download adaptive connection spawning** (adPD-style): connection
  count is user-chosen and size-capped at add time; live re-sizing adds
  failure modes (worker bookkeeping under pause/failover) for gains the
  guided-lease + mirror-weighting schedule already captures. Documented as
  a conscious scope cut.
- **HTTP/2 multiplexing over one connection**: loses to parallel
  connections on TCP head-of-line blocking (see Manzoor et al. above) and
  would forfeit per-connection AIMD isolation.

## Verified

Every adopted feature is exercised by tests: 124 Rust tests (unit +
integration) plus a 24-check cross-component E2E script
(`scripts/e2e.sh`), a 27-check feature-combination suite
(`scripts/e2e-cross.sh`) and 64 extension assertions — see
[REQUIREMENTS-COVERAGE.md](REQUIREMENTS-COVERAGE.md).
