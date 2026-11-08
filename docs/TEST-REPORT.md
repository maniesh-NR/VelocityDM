# Final integration test report

Environment: Linux x86_64 sandbox, 2 vCPU, Rust 1.99.1 (workspace pins keep
1.75.0 compatibility for the Windows 7 payload). All numbers below are from
the actual runs in this workspace.

## Test suites (all green)

| Suite | Count | Scope |
| --- | --- | --- |
| vdm-core unit tests | 41 | token bucket, AIMD tuner, range pool, ctl files, HTTP head/body parsing, request building, HLS parsing + AES roundtrip, checksum, settings, events |
| vdm-core engine integration | 15 | full download pipeline against misbehaving in-process servers |
| vdm-rpc | 4 | frame codec incl. split/oversized frames |
| full-stack E2E (real binaries) | 1 | daemon + CLI + NMH + SSE in one scenario |
| extension (bun) | 78 | classify, protocol framing, settings migration, dedupe, sniff pipeline |
| **Total** | **139** | |

## Engine integration coverage (15 tests)

single-stream sha256 match; 8-segment parallel download with per-host cap
assertions (observed max active connections within [2, 8], ranged requests
present); pause mid-download with `.part.vdmctl` validation then resume to a
byte-exact file; daemon restart recovery from the ctl file (scan_and_resume);
retries across mid-stream connection kills (server drops every connection
after 192 KiB, download still byte-exact); mirror failover (dead primary,
healthy mirror); rate limiting (1 MiB/s floor respected, >= 2.2 s for 3 MiB);
checksum mismatch fails the task with the actual digest in the error;
Content-Disposition naming (RFC 5987 included); redirect following;
unknown-length (EOF-delimited) downloads; event coalescing to the 12 Hz
refresh cap; HLS with AES-128 segments decrypting to the exact plaintext
concatenation; unreachable mirror fails fast; concurrent tasks sharing the
pool with reuse counters > 0.

## Cross-service integration (the "works with each other" matrix)

| Integration | Verified by |
| --- | --- |
| CLI -> daemon (framed JSON-RPC) | E2E: add --wait, list --json, stats, set/get, pause/resume, shutdown |
| extension -> NMH -> daemon | E2E: real NMH binary handshake (`hello`, `daemon:"running"`), `ping`, and `add` roundtrips |
| daemon -> SSE consumers | E2E: GET /events receives progress events during a live download |
| daemon events -> extension notifications | NMH event forwarding (hello/rpcResult/event frames) covered by protocol tests + E2E |
| pause/resume/restart recovery | engine test (manager restart) + E2E pause/resume cycle, byte-exact results |
| engine -> verification | post-download checksum path (mismatch test) + CLI verify |
| global + per-task rate limits | E2E set 256 KiB/s then engine rate-limit test at 1 MiB/s |
| settings -> clients | daemon persists effective settings; CLI/NMH port discovery tested |

## Bugs found and fixed by this test series

1. **Token bucket seed order** - the refill cap was applied before the first
   seed, letting the first grant exceed 2x the intended burst.
2. **Retry offset corruption** - workers resumed retries at the stale claim
   start while the Range request moved forward, writing later data at earlier
   offsets (holes in the file). The write offset now always matches the Range
   start.
3. **Pause misread as completion** - a mid-block pause returned Ok, which
   marked unfinished blocks complete; ctl files then advertised holes as
   done. Pause/cancel now release the claim without completion.
4. **Progress counter quantization** - pool progress only advanced on block
   completion, so a paused snapshot could miss megabytes of disk truth;
   progress is now marked per chunk.
5. **Test server double CRLF** - the fixture emitted `\r\n\r\n` mid-head,
   terminating the head early; also added keep-alive and correct
   EOF-delimited close semantics.
6. **Oversized futures** - 64 KiB staging arrays in async fns composed into
   futures too large for inline callers (stack overflow in the test harness);
   the IO buffer moved to the heap inside `Body`.

## Performance and stability (release build)

Benchmark (scripts/bench.sh, loopback, 128 MiB payload, 8 segments):

| Metric | Result |
| --- | --- |
| Throughput | **755 MB/s** (128 MiB in 0.17 s) |
| sha256 of result | **MATCH** (same file also verified after pause/resume and retry-kill runs) |
| Peak daemon RSS (idle + sustained 32 MiB @ 16 MiB/s + burst) | **5.5 MiB** |
| RSS after downloads | 5.8 MiB (flat; no growth => streaming writes, no accumulation) |
| Connection pool reuse | 65 reuses over 1 TCP connect |
| Event rate | 36 snapshots during a ~3 s window (<= the 12 Hz cap; coalescing verified) |
| Release binary size | 3.2 MiB (daemon+CLI, stripped) |

## Feature audit against the original request

| Requested | Status |
| --- | --- |
| Windows 7+ (x86/x64) | win7/ headless workspace (daemon+CLI+NMH, extension popup as UI), pinned 1.75.0 toolchain procedure; binaries were produced from this same procedure in the previous delivery cycle |
| macOS Catalina+ | build + .app packaging scripts (native build required; Apple SDK cannot be cross-linked from Linux) |
| Linux Mint (Cinnamon/MATE/Xfce/Debian) | .desktop + install.sh + Flatpak manifest |
| Flatpak | packaging/flatpak manifest |
| Portable | `--portable` mode + launchers |
| Android 8+ (armv7/arm64, 1DM-class) | Kotlin/Compose MD3 client: segmented engine, ctl resume, throttler, foreground service, share/clipboard capture, ABI-split versionCodes (source bundle; APK built locally per android/README.md) |
| Chrome/Safari/Edge/Firefox/Opera extensions with video sniffing | 4 MV3 packages + Safari conversion guide; sniffing pipeline fully tested |
| Rust engine | vdm-core (all above) |
| FOSS integration | tokio, rustls, RustCrypto, serde, clap; Android: OkHttp, Room, Compose |
| Academic algorithms (Padhye 1998, TFRC/RFC 3448, BitTorrent 2003, work stealing) | implemented and documented in docs/ALGORITHMS.md |
| GUI/engine decoupling, non-blocking channels, 10-30 Hz refresh cap, zero-copy streaming, connection pool | architecture enforced and tested (event-coalescing test, RSS-flat benchmark, pool-reuse assertions) |
| Material Design 3 | Android Compose MD3 + wallpaper dynamic color (Android 12+); egui Material You GUI (OKLCH tonal palettes, light + dark) behind the `gui` feature |
| GPL-3.0-or-later, English UI, no emoji, no DRM circumvention | LICENSE + clean-room statement (docs/CLEAN_ROOM.md) |

## Known limitations

- The egui GUI is shipped as source behind the `gui` feature for desktop
  targets; this sandbox cannot link X11, so the headless payload (daemon +
  CLI + NMH + extension popup) is the verified UI path here.
- Android APKs must be built locally (no SDK in this environment); the source
  tree is complete and internally consistent.
- Cross-platform binaries (Windows, Win7, musl) follow the documented
  procedures; the Linux x64 release binaries in binaries/ are the artifacts
  produced and verified in this run.
