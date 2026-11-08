# Architecture

```
                        browser extension(s)
                              |  chrome.runtime.sendNativeMessage
                              v
  velocitydm gui ----> velocitydm-nmh ----> 127.0.0.1:36990 (vdm-rpc)
  (egui, in-proc     (native messaging         |        ^
   engine + rpc)      host, 4-byte LE JSON)     | JSON-RPC 2.0 POST /rpc
                                                    SSE GET /events
                              +-----------------+---------+
                              |                           |
                        velocitydm CLI                    |
                              |                           |
                              v                           v
                     =====================  Engine (vdm-core)  ===============
                     |  Engine facade (engine/mod.rs)                       |
                     |   - task registry, snapshots, EventBus               |
                     |   - PriorityGate: max_active + (priority, FIFO)      |
                     |   - scheduler thread: stop-window, queue-drain       |
                     |   - global TokenBucket (speed cap for all tasks)     |
                     +------------------------------------------------------+
                        | http/hls task            | bt task
                        v                          v
                 engine/task.rs                bt/engine.rs
                 probe -> workers              magnet metadata (BEP-9)
                 RangePool leases              tracker announce
                 mirror failover               peer threads (BEP-3)
                 .part/.vdmctl state           PieceStore (rarest-first)
                        |                          |
                        v                          v
                   http.rs Conn                peer.rs wire protocol
                   tls.rs (rustls)            seeder.rs (serve/share)
                   proxy.rs (CONNECT/SOCKS5)  tracker.rs (+in-proc tracker)
```

## Process model

- `velocitydm daemon` — engine + RPC server, headless (6 MB RSS).
- `velocitydm gui` — same engine embedded, plus the egui front-end.
- `velocitydm <command>` — short-lived CLI talking to the daemon over RPC;
  auto-starts one if none answers.
- `velocitydm-nmh` — stdio bridge between browser native messaging and the
  RPC server; also able to launch the daemon/GUI.

## Data flow and threading

1. `Engine::add_download` allocates an id, inserts a `TaskSnapshot`, stores
   the task spec (for pause/resume/refresh) and spawns a **task thread**.
2. The task thread waits on the `PriorityGate` (max active, priority order),
   probes (HEAD, fallback ranged GET, per-mirror), then either:
   - runs the segmented worker pool (`connections` threads leasing from the
     shared `RangePool`, writing positioned blocks into the pre-allocated
     `.part`), or
   - runs the HLS pipeline (4 fetch workers, ordered writer, AES-128), or
   - hands off to the BT engine (tracker announce, peer threads, piece
     store, multi-file writer).
3. Workers publish progress by updating the shared snapshot (≥100 ms
   throttle) and emit coarse `Event`s on the `EventBus` (sync channel).
4. Pollers — SSE loop (10 Hz, change detection), GUI (5 Hz), CLI — read the
   snapshot store. Nothing blocks the engine on UI speed.

## Resume formats

- HTTP: `<file>.part` (pre-allocated) + `<file>.vdmctl` (JSON: url, mirrors,
  size, etag/last-modified, checksum, remaining intervals).
- BT: `<name>.vdmctl` (JSON: info hash, have-set, priorities); files are
  hash-verified to rebuild the have-set when the state file is missing.

## RPC surface (127.0.0.1:36990)

`daemon.ping|info|shutdown`, `downloads.list|get|add|addBatch|addTorrent|
pause|pauseAll|resume|remove|refresh|priority`, `torrents.files`,
`batch.expand`, `queue.get|set`, `limits.get|set`, `settings.get|set`,
plus `GET /events` (SSE) and `GET /health`.

## Extension components

- `shared/` — UMD modules (detector, patterns, links, filters, messaging,
  state) shared by all four browsers and by the node test suite.
- background — sniffing (`webRequest.onResponseHeaders`), download takeover
  (`downloads.onCreated` → NMH → cancel), context menus, message router.
- content — link/media grabber for the popup and "download all links".
- popup — sniffed videos (quality labels), live download list (1 s poll),
  quick add (URL/magnet/pattern), grab-links.
- options — interception categories, size floor, host exclusions, host
  connection test.

## Android components

`MainActivity` (Compose) → `DownloadsViewModel` → `SegmentEngine`
(coroutines + OkHttp, same range-pool/`.vdmctl` model) →
`DownloadService` (foreground, notifications) + `BootReceiver`;
`BrowserActivity` provides the 1DM-style sniffer browser.
