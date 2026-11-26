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

> Note: the local engine is Kotlin-only; BitTorrent/HLS/DASH downloads
> require the desktop Rust engine. Since S8-W1 the app can PAIR with a
> desktop daemon and remote-control it over the LAN (below); S8's
> second half (unifying the local engine onto the Rust core) is still
> pending its own gate.

## Remote mode (S8-W1)

The toolbar overflow menu gains **Remote**: pair the phone with a
desktop daemon running in LAN mode.

1. On the desktop: `velocitydm daemon --addr 0.0.0.0:36990` — the
   daemon prints the bearer token and the certificate fingerprint
   (`velocitydm lan-token` reprints them; `--rotate` replaces the
   token).
2. On the phone: Remote dialog -> enable, enter the desktop's host,
   port, and token (optionally the fingerprint, which pre-pins the
   daemon and closes the first-connect window). **Test** probes
   `daemon.ping` and reports honestly.
3. In remote mode the phone is a remote control: adds send
   url + filename only (the daemon's downloadDir owns placement), the
   task list is the daemon's (states `verifying`/`seeding` included),
   and the stale banner appears while the daemon is unreachable (rows
   freeze; a 2 s probe reconnects automatically).

Security posture (spec `docs/SPECS/S8-LAN-RPC-WAVE1.md`): non-loopback
daemon peers require TLS (self-signed certificate) + the bearer token;
the phone pins the certificate fingerprint on first use (TOFU) — a
later change is a hard failure, never silently re-pinned. Switching
local <-> remote pauses local tasks gracefully (resumable via
`.part`/`.vdmctl`); the two task lists are never merged. The token is
stored in app-private SharedPreferences (a keystore-encrypted store is
a recorded follow-up). Desktop snapshot fields the card does not
render (BT p/s, ETA, connections, peers/seeds) are dropped (see the
mapping table in the spec); real-device pairing/Doze behavior is an
owner-verification item, not CI-tested.
