# Requirements coverage

The original project brief, checked line-by-line against this tree. All
numbers refer to verified artifacts (tests in-tree, binaries built).

## Original request

| Requirement | Status | Where / evidence |
| --- | --- | --- |
| Clean-room IDM-style download manager, brand VelocityDM | Done | [CLEAN_ROOM.md](CLEAN_ROOM.md); no proprietary code, GPL-3.0-or-later |
| Windows 7+ (x86/x64) | Done | Windows x64/x86 (full GUI+TLS) + Windows 7 headless payloads cross-built via cargo-zigbuild and shipped; Win7 uses the pinned 1.75 toolchain workspace (`win7/`) |
| macOS Catalina+ | Partial | Bundle script + Info.plist done (`packaging/macos/bundle-macos.sh`); final link step needs a Mac (documented) |
| Linux Mint Cinnamon/MATE/Xfce/Debian | Done | glibc build verified end-to-end; `.desktop` + install.sh; musl static build for older baselines |
| Flatpak | Done | `packaging/flatpak/io.github.velocitydm.VelocityDM.yml` |
| Portable version | Done | `--portable` flag (config beside the binary) + launcher script |
| Android 8+ armv7/arm64 with 1DM-style features | Done (source) | `android/` — Compose app, segmented engine, sniffer browser, takeover, foreground service; APK must be built with the Android SDK (README) |
| Browser extensions Chrome/Safari/Edge/Firefox/Opera (MV3) | Done | 4 zips built (`extensions/dist`), Safari conversion guide; MV3 service workers; 64 node assertions |
| Video stream sniffing/capture | Done | Extension `webRequest` sniffing with quality labels; HLS engine with AES-128; Android sniffer browser |
| Rust engine | Done | `vdm-core` (engine), `vdm-rpc`, `velocitydm`, `velocitydm-nmh` |
| Material Design 3 GUI | Done | egui Material You theme (dynamic color seeds, light + dark; desktop), Compose MD3 + wallpaper dynamic color on Android 12+ (Android) |
| GUI/engine decoupled, non-blocking channels | Done | Engine threads ↔ `EventBus` (sync channel) ↔ RPC SSE / GUI poll at 5–10 Hz; GUI is a client of the same engine API |
| Refresh-rate limiting (10–30 Hz) | Done | SSE ticks 10 Hz, GUI repaint request 200 ms (5 Hz), progress updates ≥ 100 ms |
| Zero-copy streaming writes | Done | 64–256 KiB read buffers stream straight to `pwrite`-style positioned writes on the pre-allocated `.part`; no full-file buffering (verified: 6 MB RSS during 3 MB transfer) |
| Connection pooling | Done | Keep-alive `Conn` **held across leases** (recycled after every cleanly drained body), stale-connection free retry, HLS per-worker reuse; reconnect only on error/mirror-switch — A/B: 70% fewer TCP connections (ALGORITHMS.md) |
| English UI, no emoji, no DRM circumvention | Done | All UI strings English; no emoji in product surfaces; no DRM code |

## This round's ask: incorporate XDM / Motrix / Gopeed improvements

See [IMPROVEMENTS.md](IMPROVEMENTS.md) — 24 adopted ideas, each mapped to
origin and implementation. Headline additions this round: full BitTorrent +
magnet stack, torrent creation and seeding, refresh-link, scheduler
(start-at, daily stop-window, queue-drain actions), global speed limiter,
batch patterns, HTTP/SOCKS5 proxies, extension link-grabber + download
takeover with cookies.

## Test evidence

| Suite | Count | Result |
| --- | --- | --- |
| `cargo test --workspace` (unit + integration) | 124 | all pass |
| `node extensions/tests/run-tests.mjs` | 64 assertions | pass |
| `scripts/e2e.sh` cross-component smoke | 24 checks | all pass |
| `scripts/e2e-cross.sh` feature-combination suite | 27 checks | all pass |
| `scripts/bench-perf.sh` A/B performance | 4 runs | pass (byte-identical) |

E2E coverage: daemon↔CLI↔seeder↔torrent↔magnet↔NMH↔SSE, checksum, resume,
refresh, batch, limits, scheduler, settings, memory (RSS ~6–7 MB after
mixed HTTP+HLS+BT+magnet load).

Cross-feature combinations verified (`e2e-cross.sh`): segmented×per-task
limit×mirrors×checksum×pause/resume (with .part crash-safety), HLS
AES-128×quality×global limiter (byte-exact decrypt vs openssl reference),
concurrent HTTP×batch×torrent×magnet under the max-active gate + global
limit, NMH×magnet (addTorrent) + NMH×HTTP with filename override.

Performance round (paper-informed; see ALGORITHMS.md "Research notes"):
persistent connections across leases, throughput-weighted mirror selection,
guided lease sizing, size-capped connection counts, staggered starts,
paced 64 KiB writes (bounded pause latency under hard limits), positioned
pwrite I/O, RFC 3986 dot-segment URL normalization, HLS per-worker
connection reuse. A/B on 128 MiB / 8 connections with simulated WAN RTT:
1.74× faster at 40 ms RTT, 1.23× at 250 ms, 70% fewer TCP connections.

## Known limitations (honest list)

1. **macOS bundle not linked here** — cross-linking eframe needs Apple
   frameworks; script + Info.plist ready. Windows x64/x86 and Windows 7
   payloads ARE cross-built via cargo-zigbuild (mingw) and shipped.
2. **UDP trackers / DHT not implemented** — HTTP trackers only; magnets need
   `&tr=` entries. Documented in ALGORITHMS.md.
3. **Live HLS (no `#EXT-X-ENDLIST`) rejected** — VOD playlists only.
4. **SAMPLE-AES** and **fMP4 EXT-X-MAP** not supported in the HLS engine.
5. **Android APK not built here** (SDK download size); source + instructions
   complete.
6. Scheduler stop-window uses UTC wall-clock (documented in
   `engine/scheduler.rs`); no DHT means private magnets without trackers
   cannot resolve.
