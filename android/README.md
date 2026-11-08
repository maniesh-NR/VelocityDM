# VelocityDM for Android

A Material Design 3 Compose front-end over a Kotlin segmented-download
engine (OkHttp + coroutines). The architecture mirrors the desktop Rust
engine: probe, work-stealing range pool, mirror failover, token-bucket rate
limit, `.part`/`.vdmctl` crash-safe resume.

## Features

- HTTP/HTTPS accelerated downloads (up to 8 segments, work-stealing)
- Pause/resume with crash-safe `.vdmctl` state
- Per-download speed limit
- Download takeover: register as handler for browser downloads, `.torrent`
  files and `magnet:` links (share/open-with)
- Built-in sniffer browser (1DM-style) that lists videos/music found in
  page traffic (m3u8/DASH/mp4/webm) for one-tap download
- Foreground service with progress notifications, boot receiver

## Building

Requires Android Studio (Ladybug+) or the command line:

```sh
# install the SDK cmdline tools, then:
sdkmanager "platform-tools" "platforms;android-34" "build-tools;34.0.0"
export ANDROID_HOME=$HOME/android-sdk

cd android
gradle wrapper --gradle-version 8.10.2   # once
./gradlew assembleRelease                # or assembleDebug
```

ABI splits produce per-architecture APKs (armv7 versionCode 20001,
arm64 versionCode 20002) plus signing as usual:

```sh
./gradlew bundleRelease
```

> Note: the Android engine is Kotlin-only; BitTorrent/HLS stay on the
> desktop Rust engine (the RPC protocol is shared, so a desktop daemon
> can also be reached over LAN in a future release).
