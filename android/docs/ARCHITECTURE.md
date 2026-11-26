# Android architecture notes (S12-e: Views + Kotlin Multiplatform)

> **Status:** this describes the current `com.velocitydm.android` tree
> after the S12-e restructure (View-based Material 3 UI + KMP `:shared`
> module; gate 0005). The old note about a richer Room/DataStore
> `com.velocitydm.app` candidate is retired — that design remains a
> possible future migration, not a tracked stage.

```
  Android View tree (Material 3 via MDC — MaterialToolbar, RecyclerView
  of MaterialCardViews, FAB, MaterialAlertDialog)      [app module]
      |  StateFlow<List<TaskState>>
  DownloadsViewModel
      |  intents (enqueue / pause / resume / remove)
  DownloadService  (foreground, dataSync, progress notifications)
      |
  SegmentEngine (android/shared, jvmCommon source set)
    - probe: HEAD, ranged GET fallback
    - range pool: work-stealing interval leases (commonMain math)
    - workers x N: Range GETs -> RandomAccessFile writes
    - Vdmctl schema: <name>.vdmctl strict validated resume state
    - TokenBucket: per-download rate limit (commonMain wait math)
      |
  OkHttp 4 (HTTP/1.1, range requests, redirects)
```

## Module structure (Kotlin Multiplatform)

`android/shared/` is a KMP module with two targets, `android()` and
`jvm()`, plus a custom `jvmCommon` intermediate source set that both
JVM-family targets compile:

- `commonMain` — pure, platform-independent logic: the **Md3 token
  engine port** (Kotlin twin of `crates/vdm-md3`), interval-leasing
  math, the `.vdmctl` schema (kotlinx.serialization, strict),
  token-bucket wait math, byte/speed/ETA formatting, URL validation +
  filename sanitization, and the task models (String paths only).
- `jvmCommon` — the SegmentEngine orchestration (OkHttp,
  `RandomAccessFile`, atomics, coroutines). Any future non-JVM target
  adds one `actual` set behind this boundary.
- `jvmTest` — runs the common tests on the host JVM, including the
  **Rust<->Kotlin drift tripwire**: `Md3GoldenTest` asserts every field
  of every named seed against `md3-tokens-baseline.json`, generated
  from the canonical Rust engine by `scripts/gen-md3-baseline.py`.

## Scheme -> MDC mapping (Material You on Android)

Colors resolve per element via `ui/Chrome.kt`:

| Element | Android 12+ (dynamic) | Android < 12 (engine) |
| --- | --- | --- |
| Title / accents / progress / downloading label | `colorPrimary` (wallpaper) | `scheme.primary` |
| FAB container / icon | `colorPrimaryContainer` / `colorOnPrimaryContainer` | same roles from the Kotlin port |
| Card background | `colorSurfaceContainer` | `scheme.surfaceContainer` |
| Filename / primary text | `colorOnSurface` | `scheme.onSurface` |
| Stats / empty state / URLs | `colorOnSurfaceVariant` | `scheme.onSurfaceVariant` |
| Completed label | `colorTertiary` | `scheme.tertiary` |
| Failed label / error text | `colorError` | `scheme.error` |

- On Android 12+ the whole app follows the **wallpaper Material You**
  scheme (MDC dynamic color) — the OS-specific element the S12 stage
  keeps.
- On Android < 12 the chrome comes from the **shared Kotlin Md3 port**
  seeded `0x4156A4` (`vdm_md3::DEFAULT_SEED` — the same engine, same
  math, same seed as the desktop frontends). Pre-12 colors visibly
  change versus the old hand-copied Google baseline (engine tone
  tables, brand-blue hue) — that is the point: one seed, same roles on
  every OS.
- **Intentional remainder on < 12:** MDC-owned containers this app does
  not paint (dialog accents, ripples) keep MDC baseline values. The
  promotion gate may bake a generated overlay if that mixture matters
  on real hardware.

## Security posture (gate 0005)

- `.vdmctl` resume state is strictly validated (`0 <= start <= end <
  size`, coverage <= size, unknown keys rejected): tampered state
  restarts the download instead of seeking wild offsets; a missing
  server ETag only resumes on an exact size match.
- Every filename (dialog override, URL-derived, browser-sniffed) passes
  `sanitizeFilename` (separators stripped, control chars dropped,
  reserved device names rejected, length capped).
- URL entry points validate real http/https URLs (not
  `startsWith("http")`).
- Cleartext: the manifest `usesCleartextTraffic` attribute is the one
  source of truth (the dead, unreferenced network security config XML
  was removed).


## Remote mode (S8-W1, gate 0018)

`data/DownloadsViewModel` holds BOTH engines behind the shared
`engine/DownloadEngine` interface: `SegmentEngine` (local, Kotlin) and
`remote/RemoteEngine` (:shared jvmCommon — a remote-control client of
the desktop daemon). The mode is a persisted setting (`RemotePrefs`,
default OFF — local behavior is unchanged until pairing).

- **RemoteEngine** implements the gate-0010 SSE consumer contract:
  initial `downloads.list`; `tasks` ticks merge in place keyed by id;
  membership/metadata changes and non-tick events coalesce into a
  150 ms re-list; ticks are dropped inside the resync window so the
  full re-list precedes any merge after (re)connect; a dead stream
  freezes rows (stale banner) and probes every 2 s — probe failures
  never mutate the list.
- **RemoteRpc** (OkHttp): cookie-less, redirects refused, bearer token
  on every request, dedicated no-read-timeout SSE client; failures are
  typed (unreachable / 401 / TLS).
- **TofuTrustManager**: first-use pins the daemon certificate's SHA-256
  (the MITM window — mitigated by entering the fingerprint printed by
  `velocitydm lan-token`); a changed certificate is a hard failure.
- **Mapping** (`remote/RemoteMapper`): the explicit snapshot->TaskState
  table — state/error/notice/userPaused pass through (verifying and
  seeding render as distinct chips); etaSecs/connections/peers/seeds/
  priority/startAt/checksumOk/createdAt are dropped (the card renders
  none of them).
- **Mode switch**: leaving local calls `SegmentEngine.stopAll()`
  (graceful pause, resumable); leaving remote closes the stream only
  (the daemon is untouched). Lists are never merged (independent id
  namespaces); notifications use distinct tags (`local`/`remote`).
- **Tests**: `:shared:jvmTest` covers the parser, mapper, config
  validation and TOFU decision (fixed OpenSSL fixtures); the live
  stack (OkHttp client <-> rustls daemon) is witnessed by the env-gated
  `LanDaemonLiveTest` (runbook recipe) and by the Rust-side feature
  suite in `crates/vdm-rpc/tests/lan.rs`.
