# VelocityDM for Android

A Material You (Material Design 3, wallpaper dynamic color on Android
12+) **View-based Material 3** front-end (Android Views + MDC — S12-e;
the Jetpack Compose stack is gone) over a Kotlin segmented-download
engine (OkHttp + coroutines) restructured into a **Kotlin
Multiplatform** `:shared` module. The architecture mirrors the desktop
Rust engine: probe, work-stealing range pool, mirror failover,
token-bucket rate limit, `.part`/`.vdmctl` crash-safe resume.

## Features

- HTTP/HTTPS accelerated downloads (up to 8 segments, work-stealing)
- Pause/resume with crash-safe, strictly validated `.vdmctl` state
  (tampered or stale state restarts instead of resuming)
- Per-download speed limit
- Download takeover: register as handler for browser downloads,
  `.torrent` files and `magnet:` links (share/open-with)
- Built-in sniffer browser (`BrowserActivity`, a real Activity since
  S12-e) that lists videos/music found in page traffic
  (m3u8/DASH/mp4/webm) for one-tap download
- Foreground service with progress notifications, boot receiver

## Layout

- `shared/` — Kotlin Multiplatform module:
  `commonMain` = the Md3 token-engine port (Kotlin twin of
  `crates/vdm-md3`, golden-tested against the Rust-generated
  `md3-tokens-baseline.json`), pure engine math (range pool, `.vdmctl`
  schema, token bucket), formatting, URL/filename sanitization, task
  models; `jvmCommon` = the SegmentEngine orchestration shared by the
  android and jvm targets; `jvm()` target runs the tests host-side.
- `app/` — the View-based Material 3 UI (programmatic View tree:
  MaterialToolbar, RecyclerView of Material cards, FAB, add-URL dialog
  with URL validation / filename / speed limit), ViewModel, foreground
  service, boot receiver, sniffer browser.

## Building

No Gradle wrapper is committed — the CI/release builds install
Gradle 8.9 directly (`gradle/actions/setup-gradle`). Locally, with the
SDK installed (`platform-tools`, `platforms;android-34`,
`build-tools;34.0.0`) and JDK 17:

```sh
export ANDROID_HOME=$HOME/android-sdk   # or android/local.properties
gradle -p android :shared:jvmTest       # unit tests (no device needed)
gradle -p android :app:assembleDebug    # debug APK (one universal APK)
gradle -p android :app:assembleRelease  # minified release (R8)
```

One universal APK per release (`velocitydm-<ver>-android.apk`); there
are no per-ABI splits.

> Note: the Android engine is Kotlin-only; BitTorrent/HLS stay on the
> desktop Rust engine (the RPC protocol is shared, so a desktop daemon
> can also be reached over LAN in a future release — S8 also owns
> unifying the Android engine onto the Rust core).
