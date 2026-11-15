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
