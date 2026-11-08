# Daemon RPC protocol

Two transports, one method set.

## Framed JSON-RPC over TCP (default 127.0.0.1:36990)

Each frame is `u32 LE length + UTF-8 JSON`. Requests carry an `id`; daemon
events arrive as frames without an `id`:

```json
{"id": 1, "method": "add", "params": {"url": "https://example.com/file.zip", "segments": 8}}
{"event": "progress", "params": {"id": 1, "state": "running", "downloaded": 123, "total": 456}}
{"id": 1, "result": {"id": 1}}
```

## HTTP/SSE (default 127.0.0.1:36991)

- `GET /events` - `text/event-stream`; one `data: <event JSON>` per event,
  heartbeat comment every 15 s.
- `POST /rpc` - body `{"id":..,"method":..,"params":{..}}`; response is the
  JSON-RPC reply.
- `GET /health` - `ok`.

## Methods

| Method | Params | Result |
| --- | --- | --- |
| `ping` / `hello` | {} | `{name, version, protocol}` |
| `add` | TaskSpec (url, mirrors, filename, dir, headers, segments, rate_limit_bps, checksum{algorithm,expected}, referrer, resume) | `{id}` |
| `pause` | `{id}` | `{id, state}` (waits for the settled state) |
| `resume` | `{id}` | `{id, state}` |
| `remove` | `{id, purge?}` | `{id, removed}` |
| `list` | {} | `[Snapshot]` |
| `status` | `{id}` | Snapshot |
| `verify` | `{id}` | `{id, ok}` |
| `stats` | {} | task counts, pool connects/reuses, limits, uptime |
| `get_settings` | {} | download_dir, global_rate_limit_bps, refresh_hz |
| `set_settings` | `{key, value}` | `{key, value}` (global_rate_limit_bps, refresh_hz) |
| `shutdown` | {} | `{ok}` then graceful exit |

Errors are JSON-RPC errors: `-32601` unknown method, `-32602` invalid params,
`-32000` daemon errors (task not found, not paused, etc).

## Events

`added`, `progress` (throttled to the refresh rate), `state_changed`,
`completed`, `failed`, `removed` - each `{"event": name, "params": {...}}`.
`progress` params are full Snapshots, so consumers can render without extra
requests.
