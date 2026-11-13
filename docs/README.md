# VelocityDM

A clean-room, open-source (GPL-3.0-or-later) download manager in the spirit of
classic multi-segment download managers. Rust engine, Material Design 3
interfaces, browser extension integration for Chrome/Edge/Opera/Firefox (plus a
Safari conversion guide), and an Android 8+ client.

Version 1.0.0. English UI. No emoji in any user-facing surface. No DRM
circumvention logic of any kind.

## Components

| Component | Path | Notes |
| --- | --- | --- |
| `vdm-core` | `crates/vdm-core` | The download engine (see docs/ARCHITECTURE.md) |
| `vdm-rpc` | `crates/vdm-rpc` | HTTP JSON-RPC 2.0 + SSE on one loopback address (see docs/RPC.md) |
| `velocitydm` | `crates/velocitydm` | Daemon + CLI (and the `gui` feature for desktop builds) |
| `velocitydm-nmh` | `crates/velocitydm-nmh` | Native messaging host for browser extensions |
| Browser extension | `extensions/` | One MV3 codebase packaged for Chrome/Edge/Opera/Firefox (+ Safari guide) |
| Android client | `android/` | Kotlin + Compose, Material You dynamic color on 12+ |
| Packaging | `packaging/` | Linux, Flatpak, Windows, macOS, portable |
| `win7/` | `win7/` | Windows 7 headless workspace (daemon + CLI + NMH) |

## Feature overview

- Segmented, multi-connection downloads with a shared work-stealing block
  queue (BitTorrent-style), sparse pre-allocation, and scatter writes at
  absolute offsets (flat memory; no full-file buffering).
- Pause / resume at block granularity via `<file>.part.vdmctl` (atomic
  tmp+rename), including daemon restart recovery.
- Mirror failover with bounded retries and exponential backoff.
- AIMD connection tuning (TFRC / Chiu-Jain) with per-host connection caps.
- Token-bucket rate limiting, global and per task.
- HTTP and HTTPS (rustls). HLS playlists with AES-128 segment decryption.
- SHA-256 / SHA-1 / MD5 / CRC32 verification of finished downloads.
- Redirects (up to 5), Content-Disposition and URL filename detection.
- Event bus with snapshots coalesced to a UI refresh rate clamped to 10..30 Hz.
- Browser capture pipeline: context menu, media sniffing, notifications.
- Android client with the same engine semantics (parts, ctl files, throttler).

## Quick start

```sh
cargo build --release --workspace
./target/release/velocitydm daemon              # JSON-RPC + SSE on 127.0.0.1:36990
./target/release/velocitydm add https://example.com/file.zip   # auto-starts a daemon
```

Install the extension (see docs/EXTENSIONS.md), register the NMH, and
downloads from the browser land in the daemon.

## Tests

```sh
cargo test --workspace                      # 127 tests: unit + engine integration + full-stack E2E
node extensions/tests/run-tests.mjs        # 76 assertions incl. the MD3 scheme
bash scripts/e2e.sh && bash scripts/e2e-cross.sh   # 24 + 27 e2e checks
```

## Documentation

- docs/ARCHITECTURE.md - component and data-flow overview
- docs/ALGORITHMS.md - the algorithms and the papers they come from
- docs/CLEAN_ROOM.md - provenance and licensing statement
- docs/BUILDING.md - per-platform build and packaging instructions
- docs/EXTENSIONS.md - browser integration and NMH registration
- docs/RPC.md - the daemon RPC/SSE protocol
- docs/TEST-REPORT.md - final integration test results and benchmarks
- win7/README.md - Windows 7 build procedure
