#!/usr/bin/env bash
# Generate release notes for one VelocityDM release.
#
# Usage (env vars):
#   TAG       tag name, e.g. v1.0.0-linux
#   VER       version, e.g. 1.0.0
#   PLATFORM  all | linux | windows | extensions | macos | android
# Writes ./notes.md in the current directory (the repository root).
set -u
: "${TAG:=v1.0.0}"; : "${VER:=1.0.0}"; : "${PLATFORM:=all}"

LIMITATIONS=$(cat <<'EOF'
## Known limitations

- **DHT (BEP-5)** is not implemented yet: trackerless magnets do not resolve peers without an HTTP `&tr=` tracker entry (see the DHT roadmap issue).
- **UDP trackers (BEP-15)** are detected and skipped; HTTP trackers work (see the UDP tracker roadmap issue).
- **DASH** `.mpd` streams are detected and surfaced by the extensions but not downloaded; HLS is fully supported.
- **HLS**: SAMPLE-AES and fMP4 `EXT-X-MAP` streams are not supported yet.
- The scheduling design follows the parallel-download research literature (Rodriguez, Koo, guided self-scheduling, Padhye's TCP model) — see [docs/ALGORITHMS.md](docs/ALGORITHMS.md).
EOF
)

VERIFY=$(cat <<'EOF'
## Verify

`SHA256SUMS.txt` ships with each release:

    sha256sum --check SHA256SUMS.txt
EOF
)

case "$PLATFORM" in
linux)
cat > notes.md <<EOF
# VelocityDM $TAG — Linux

Clean-room, IDM-style download manager: segmented HTTP/HTTPS, HLS (AES-128),
BitTorrent + magnet, scheduler, browser integration, Rust engine, MD3 egui GUI.

## Assets

- \`velocitydm-$VER-linux-x64.tar.gz\` — **GUI + daemon + CLI + NMH**, glibc (any modern distro)
- \`velocitydm-$VER-linux-x64-musl.tar.gz\` — **GUI + daemon + CLI + NMH**, fully static (Alpine, musl distros, scratch containers)
- \`SHA256SUMS.txt\` — SHA-256 checksums

## Install

    tar xzf velocitydm-$VER-linux-x64.tar.gz -C ~/.local/bin/
    chmod +x ~/.local/bin/velocitydm ~/.local/bin/velocitydm-nmh
    ~/.local/bin/velocitydm            # starts GUI; or: velocitydm daemon  /  velocitydm add <url>

For the desktop entry and native-messaging registration see [packaging/linux](packaging/linux)
(\`install.sh\`, \`velocitydm.desktop\`) and [docs/EXTENSIONS.md](docs/EXTENSIONS.md).

## What works

HTTP/HTTPS segmentation with persistent connections and mirror failover, HLS master
playlists with AES-128, BitTorrent + magnet (HTTP trackers), batch URL patterns,
scheduler (priorities, max-active, daily stop window, start-at), proxies (HTTP
CONNECT + SOCKS5), global/per-download rate limits, checksum verification
(MD5/SHA-1/SHA-256), crash-safe resume (\`.part\`/\`.vdmctl\`), Refresh-link,
JSON-RPC + SSE on 127.0.0.1:36990, Chrome/Edge/Opera/Firefox extensions.

$LIMITATIONS

$VERIFY
EOF
;;
windows)
cat > notes.md <<EOF
# VelocityDM $TAG — Windows

Clean-room, IDM-style download manager: segmented HTTP/HTTPS, HLS (AES-128),
BitTorrent + magnet, scheduler, browser integration, Rust engine, MD3 egui GUI.

## Assets

- \`velocitydm-$VER-windows-x64.zip\` — **GUI + daemon + CLI + NMH**, 64-bit Windows 10/11
- \`velocitydm-$VER-windows-x86.zip\` — same, 32-bit
- \`velocitydm-$VER-windows7-x64.zip\` / \`-x86.zip\` — **headless Windows 7 payload**
  (daemon + CLI + NMH; the browser extension popup is the UI — the egui GUI needs rustc 1.76+)
- \`SHA256SUMS.txt\` — SHA-256 checksums

## Install

Unzip, run \`velocitydm.exe\` (GUI) or \`velocitydm.exe daemon\` (headless), then register the
native messaging host for your browser per [docs/EXTENSIONS.md](docs/EXTENSIONS.md).
MSI/Inno Setup installer: [packaging/windows/velocitydm.iss](packaging/windows/velocitydm.iss).

## What works

Same engine as the other platforms: segmented HTTP/HTTPS, HLS AES-128, BitTorrent +
magnet, batch, scheduler, proxies, rate limits, checksums, crash-safe resume,
Refresh-link, JSON-RPC + SSE, Chrome/Edge/Opera/Firefox extensions.

$LIMITATIONS

$VERIFY
EOF
;;
macos)
cat > notes.md <<EOF
# VelocityDM $TAG — macOS

Clean-room, IDM-style download manager: segmented HTTP/HTTPS, HLS (AES-128),
BitTorrent + magnet, scheduler, browser integration, Rust engine, MD3 egui GUI.

## Assets

- \`velocitydm-$VER-macos-aarch64.zip\` — **GUI + daemon + CLI + NMH**, Apple Silicon
- \`velocitydm-$VER-macos-x64.zip\` — same, Intel
- \`SHA256SUMS.txt\` — SHA-256 checksums

## Install

    unzip velocitydm-$VER-macos-aarch64.zip -d ~/.local/bin
    chmod +x ~/.local/bin/velocitydm ~/.local/bin/velocitydm-nmh
    xattr -d com.apple.quarantine ~/.local/bin/velocitydm   # unsigned build
    ~/.local/bin/velocitydm

For a bundled \`VelocityDM.app\` see [packaging/macos/make-app.sh](packaging/macos/make-app.sh);
Safari uses the conversion guide in [extensions/safari](extensions/safari).

## What works

Same engine as the other platforms: segmented HTTP/HTTPS, HLS AES-128, BitTorrent +
magnet, batch, scheduler, proxies, rate limits, checksums, crash-safe resume,
Refresh-link, JSON-RPC + SSE, Chrome/Edge/Opera/Firefox extensions.

$LIMITATIONS

$VERIFY
EOF
;;
android)
cat > notes.md <<EOF
# VelocityDM $TAG — Android

Clean-room, IDM-style download manager (Android 8.0+): segmented HTTP/HTTPS,
HLS (AES-128), BitTorrent + magnet, scheduler, Material Design UI, Rust engine
via JNI.

## Assets

- \`velocitydm-$VER-android.apk\` — debug-signed build (installable on any Android 8+ device)
- \`SHA256SUMS.txt\` — SHA-256 checksums

## Install

Sideload the APK ("install unknown apps" permission), or:
    adb install velocitydm-$VER-android.apk

## What works

Segmented HTTP/HTTPS with resume, per-download connections, rate limits,
scheduler, download queue with notifications, Material Design 3 UI.

$LIMITATIONS

$VERIFY
EOF
;;
extensions)
cat > notes.md <<EOF
# VelocityDM $TAG — Browser extensions

MV3 extensions that detect media, take browser downloads over via the native
messaging host (\`com.velocitydm.nmh\`), and add magnet/HLS capture.

## Assets

- \`chromium-$VER.zip\` — Chrome / Chromium
- \`edge-$VER.zip\` — Microsoft Edge
- \`firefox-$VER.zip\` — Firefox (event-page build)
- \`opera-$VER.zip\` — Opera
- \`SHA256SUMS.txt\` — SHA-256 checksums

## Install

1. Install and run VelocityDM (daemon listens on 127.0.0.1:36990) and register the NMH per
   [docs/EXTENSIONS.md](docs/EXTENSIONS.md) / [packaging/nmh](packaging/nmh).
2. Unzip the package for your browser.
3. Load it: Chrome/Edge/Opera → \`chrome://extensions\` → Developer mode → *Load unpacked*;
   Firefox → \`about:debugging\` → *Load Temporary Add-on*.

## What it does

- Context menu: **Download with VelocityDM** (links, media, all links)
- Media sniffing: m3u8/mpd/video/audio capture with size gate and blocklist
- Filename extraction from URL path and \`Content-Disposition\` (RFC 5987 \`filename*\`)
- Popup with captures, manual add, daemon status, download-all; badge counter
- Options: daemon host/port, sniffing toggle, category toggles, blocklist, connection test
- Safari: conversion guide at [extensions/safari](extensions/safari)

$VERIFY
EOF
;;
all)
cat > notes.md <<EOF
# VelocityDM $TAG

Fast, clean-room, IDM-style download manager: segmented HTTP/HTTPS, HLS
(AES-128), BitTorrent + magnet, scheduler, browser integration — one Rust
engine, one daemon (JSON-RPC + SSE on 127.0.0.1:36990), MD3 egui GUI, tiny
footprint (~6 MB RAM idle).

## Downloads

| Package | Contents |
| --- | --- |
| \`velocitydm-$VER-linux-x64.tar.gz\` | Linux x64, glibc: GUI + daemon + CLI + NMH |
| \`velocitydm-$VER-linux-x64-musl.tar.gz\` | Linux x64, static musl: GUI + daemon + CLI + NMH |
| \`velocitydm-$VER-windows-x64.zip\` / \`-x86.zip\` | Windows 10/11: GUI + daemon + CLI + NMH |
| \`velocitydm-$VER-windows7-x64.zip\` / \`-x86.zip\` | Windows 7 headless payload (daemon + CLI + NMH) |
| \`velocitydm-$VER-macos-aarch64.zip\` / \`-x64.zip\` | macOS: GUI + daemon + CLI + NMH |
| \`velocitydm-$VER-android.apk\` | Android 8.0+ (debug-signed) |
| \`chromium-$VER.zip\` / \`edge-$VER.zip\` / \`firefox-$VER.zip\` / \`opera-$VER.zip\` | MV3 browser extensions |
| \`SHA256SUMS.txt\` | SHA-256 checksums |

## What works

- **HTTP/HTTPS**: segmented transfers with persistent connections across segments,
  throughput-weighted mirror selection with failover, retry with backoff,
  ETag/Last-Modified resume, checksum verification (MD5/SHA-1/SHA-256)
- **Video**: HLS master playlists with variant selection and AES-128-CBC; DASH
  detection; video sniffing via the browser extensions
- **BitTorrent**: \`.torrent\` + \`magnet:\` (BEP-3/9/10), HTTP trackers, rarest-first,
  per-file selection, seeding + built-in tracker (\`velocitydm seed\`)
- **Queue**: priorities, max-simultaneous, daily stop window, start-at, on-drain actions
- **Batch**: URL patterns (\`pic[001:120].jpg\`), page link grabber
- **Networking**: HTTP CONNECT + SOCKS5 proxies (with auth), global + per-download limits
- **Recovery**: crash-safe \`.part\`/\`.vdmctl\` state, Refresh-link for expired URLs
- **UI**: egui GUI (Material Design 3 dark), CLI, JSON-RPC 2.0 + SSE
- **Browsers**: Chrome/Edge/Opera/Firefox extensions, Safari conversion guide

## Quick start

    velocitydm            # GUI
    velocitydm daemon     # headless daemon
    velocitydm add https://example.com/file.iso --conn 8
    velocitydm magnet "magnet:?xt=..." --dir ~/Downloads

$LIMITATIONS

$VERIFY
EOF
;;
*)
  echo "unknown PLATFORM '$PLATFORM' (want all|linux|windows|extensions|macos|android)" >&2
  exit 1
  ;;
esac
echo "wrote notes.md (TAG=$TAG PLATFORM=$PLATFORM)" >&2
