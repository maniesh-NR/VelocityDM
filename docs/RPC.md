# Daemon RPC protocol

One HTTP server, one address (default `127.0.0.1:36990`), two endpoints and a
health probe. Plain JSON-RPC 2.0 over `POST /rpc`, server-sent events over
`GET /events`. The CLI, the native messaging host and the GUI all speak this
same surface.

```sh
./target/release/velocitydm daemon --addr 127.0.0.1:36990
./target/release/velocitydm --addr 127.0.0.1:36990 add https://example.com/file.zip
```

## Loopback trust boundary

The daemon is unauthenticated by design and binds loopback only. Three
request-shape rules keep web pages out while local clients work untouched
(`crates/vdm-rpc/src/lib.rs`, `host_allowed` + `handle_conn`):

1. `Host` must name a loopback authority (`127.0.0.0/8`, `localhost`,
   `[::1]`) when the bind is loopback — a rebound attacker hostname can
   never satisfy this. Wildcard (`0.0.0.0`) binds accept any IP literal;
   other binds must match the bound host.
2. Any request carrying `Origin` or `Referer` is refused — browsers always
   send one on cross-site fetch/sendBeacon/form posts; native clients never
   do.
3. `POST /rpc` must be `Content-Type: application/json` — defeats
   no-cors "simple requests" (`text/plain` form posts) that execute without
   a CORS preflight.

Outgoing request headers (per-download `options.headers`, referer/cookies
forwarded from the browser, the configured user-agent) are CRLF/NUL-sanitized
before emission, so no request-splitting toward third-party hosts.

## `POST /rpc` — JSON-RPC 2.0

Request:

```json
{"jsonrpc":"2.0","id":1,"method":"downloads.add",
 "params":{"url":"https://example.com/file.zip",
           "options":{"filename":"file.zip","connections":8}}}
```

Response: `{"jsonrpc":"2.0","id":1,"result":{...}}` or
`{"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"..."}}`.

### Method set

| Method | Params (beyond `id`) | Result |
| --- | --- | --- |
| `daemon.ping` | — | `"pong"` |
| `daemon.info` | — | `{name, version, rpc, features[]}` |
| `daemon.shutdown` | — | `true`; the daemon stops the engine, winds down the RPC/SSE server and exits cleanly |
| `downloads.list` | — | array of task snapshots |
| `downloads.get` | `id` | one task snapshot |
| `downloads.add` | `url`, `options` | `{id}` |
| `downloads.addBatch` | `pattern`, `options` | `{ids}` (`urls` is an NMH-side input, expanded there) |
| `batch.expand` | `pattern` | `{urls}` (dry expansion) |
| `downloads.addTorrent` | `magnet` or `torrentPath`, `options` | `{id}` |
| `torrents.files` | `magnet` or `torrentPath` | file list (pre-add inspection) |
| `downloads.pause` | `id` | `true` |
| `downloads.pauseAll` | — | `true` |
| `downloads.resume` | `id` | `true` |
| `downloads.remove` | `id`, `deleteFiles?` | `true` |
| `downloads.refresh` | `id`, `url` | `true` (XDM-style refresh-link) |
| `downloads.priority` | `id`, `priority` (0-255) | `true` |
| `queue.get` / `queue.set` | `maxActive`, `stopWindow`, `onQueueDrained` | scheduler config |
| `limits.get` / `limits.set` | `globalBps` | rate limits |
| `settings.get` / `settings.set` | `downloadDir`, `defaultConnections`, `proxy`, `userAgent` | persisted config |

### `downloads.add` options

`filename`, `outDir`, `connections`, `rateLimitBps`, `mirrors[]`,
`checksumAlgo` (`md5|sha1|sha256`) + `checksumHex`, `quality`
(HLS `best|worst|<bandwidth>`), `priority` (0-255), `startAt` (UNIX ts),
`fresh` (ignore saved progress), `headers` — an **array of
`["Name", "Value"]` pairs** (CRLF/NUL-sanitized at emission).

`referer` / `cookies` / `userAgent` are *native-messaging-host* inputs:
the NMH folds them into the `headers` array (Cookie/Referer/User-Agent)
before calling this method; the RPC itself does not accept them.

### Task snapshot shape (camelCase)

`id`, `kind` (`http|hls|torrent|magnet`), `name`, `url`, `dir`, `outPath`,
`state`, `size`, `done`, `speed`, `etaSecs`, `connections`, `peers`,
`seeds`, `priority`, `startAt` (null or epoch), `error` (null or string),
`checksumOk` (null/bool), `userPaused`, `createdAt`. States: `queued`,
`probing`, `downloading`, `paused`, `verifying`, `completed`, `failed`,
`seeding`.

## `GET /events` — server-sent events

`text/event-stream`; one `data: <event JSON>` line pair per event, plus a
10 Hz coalesced `tasks` progress tick.

Events: `task-added`, `task-state`, `task-progress`, `task-completed`
(with `id` and `path`), `task-failed`, `task-removed`, `queue-drained`,
`config-changed`, and the periodic `tasks` snapshot delta.

```
data: {"type":"task-added","id":1}

data: {"type":"task-completed","id":1,"path":"/home/me/Downloads/file.zip"}
```

`task-progress` carries `{type, id}` — consumers coalesce by polling
`downloads.list` or reading the periodic `tasks` tick.

## `GET /health`

`200 {"ok":true}` — liveness probe. (The CLI's daemon auto-start probes
`daemon.ping` over `/rpc`.)

## Client notes

- Keep-alive is supported (bounded to 256 requests per connection);
  the CLI closes per call.
- `daemon.shutdown` is also exposed as the CLI subcommand
  `velocitydm shutdown`.
- The native messaging host wraps this RPC for the browser extension —
  see docs/EXTENSIONS.md.
