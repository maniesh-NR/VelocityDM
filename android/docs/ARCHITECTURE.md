# Android architecture notes

> **Status:** this file describes the richer Room/DataStore design that
> was built as the v1.1.0 candidate (`com.velocitydm.app`) but is NOT the
> tree that ships 1.0.0. The shipped app is the simpler, CI-proven
> `com.velocitydm.android` tree (`DownloadsScreen` + `DownloadsViewModel`
> + `engine/SegmentEngine`, work-stealing segmented engine with
> `.part`/`.vdmctl` crash-safe resume, foreground `DownloadService`,
> sniffer `BrowserActivity`, wallpaper-based Material You theming). Read
> the below as the design target for a future migration, not the current
> code.

```
  Compose UI (Material 3, dynamic color)
  DownloadsScreen / AddScreen / SettingsScreen
              |  StateFlow (Room rows) + throttled snapshots (10..30 Hz)
  DownloadsViewModel
              |  intents (enqueue / pause / resume / remove)
  DownloadService  (foreground, dataSync, max 3 engines)
              |
  DownloadEngine  (per download)
    - probe: HEAD, ranged GET fallback (HttpRangeClient)
    - PartQueue: shared work-stealing queue, per-part ownership
    - SegmentWorker x N: Range GETs -> RandomAccessFile.position writes
    - VdmCtl: <name>.part.vdmctl atomic resume state (tmp + rename)
    - RateLimiter: token bucket (bytes/sec)
    - ProgressThrottler: snapshot flow at refreshHz (clamped 10..30)
              |
  OkHttp 4 (HTTP/1.1, range requests, redirects)
```

Design decisions:

- The engine never touches Compose state. UI data is Room rows plus the
  throttled snapshot flow, so recomposition cost is bounded by the refresh
  rate regardless of engine throughput.
- Scatter writes (`FileChannel.position` / `RandomAccessFile.seek`) keep
  memory flat: the only buffer is the 64 KiB read window per worker.
- Resume state is written atomically every 2 s and on pause/failure; a
  mismatched ETag or total invalidates it, forcing a clean restart.
- Endgame behavior mirrors the desktop engine: when unclaimed parts run out,
  workers exit; parts are small enough that the tail never stalls.
