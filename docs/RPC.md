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

The daemon is unauthenticated by design and binds loopback only by
default. Three request-shape rules keep web pages out while local
clients work untouched (`crates/vdm-rpc/src/lib.rs`, `host_allowed` +
`handle_conn`):

1. `Host` must name a loopback authority (`127.0.0.0/8`, `localhost`,
   `[::1]`) when the bind is loopback — a rebound attacker hostname can
   never satisfy this. Wildcard (`0.0.0.0`) binds are LAN mode (below):
   loopback peers keep these same rules, non-loopback peers must pass
   the token gate first; other binds must match the bound host.
2. Any request carrying `Origin` or `Referer` is refused — browsers
   always send one on cross-site fetch/sendBeacon/form posts; native
   clients never do.
3. `POST /rpc` must be `Content-Type: application/json` — defeats
   no-cors "simple requests" (`text/plain` form posts) that execute
   without a CORS preflight.

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
`["Name", "Value"]` pairs** (CRLF/NUL-sanitized at emission) — and
`connPreset` (`auto|high|medium|low`, default `auto`; S11-i B2p).

`connPreset` is a link-class knob for the BASE connection count — a
**ceiling, never a raise**: `high` caps the base at 8, `medium` at 4,
`low` at 2, `auto` keeps the existing size-scaled behavior. An unknown
value is rejected at both the RPC engine boundary and the NMH host with
`unknown connection preset '<value>'`. **Honest note:** high connection
counts can antagonize servers (the reference product itself warns
about this) — that is why the presets default to `auto`.

`browserProxy` (S11-i B16): the browser-reported effective proxy
(`{"type":"http"|"socks5","host","port"}`) — the NMH forwards the
extension's `{scheme,host,port}` shape after validation. The engine
suggests, never overrides: an explicit task `proxy` always wins (a
differing browser proxy warns — the warning string rides the add reply
and the snapshot `notice`); otherwise the reported proxy is adopted only
when a one-connect reachability probe reaches it, and an unreachable one
is dropped with the loopback-flavored warning.

`referer` / `cookies` / `userAgent` are *native-messaging-host* inputs:
the NMH folds them into the `headers` array (Cookie/Referer/User-Agent)
before calling this method; integrators should do the same rather than
rely on any per-field passthrough.

### Task snapshot shape (camelCase)

`id`, `kind` (`http|hls|torrent|magnet`), `name`, `url`, `dir`, `outPath`,
`state`, `size`, `done`, `speed`, `etaSecs`, `connections`, `peers`,
`seeds`, `priority`, `startAt` (null or epoch), `error` (null or string),
`notice` (null or string — additive engine hint, e.g. proxy warnings or
recovery notes; may be absent on older snapshots), `checksumOk`
(null/bool), `userPaused`, `createdAt`. States: `queued`, `probing`,
`downloading`, `paused`, `verifying`, `completed`, `failed`, `seeding`.

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
`daemon.ping` over `/rpc`.) In LAN mode the health probe also requires
the bearer token from non-loopback peers.

## LAN mode (S8-W1, `--features lan`)

Binding a NON-loopback address (`--addr 0.0.0.0:36990` or an explicit
LAN IP) switches the daemon to LAN mode so a phone (the Android
remote mode) or other native clients on the network can drive it.
LAN mode is FAIL-CLOSED:

- Without the `lan` compile feature the daemon REFUSES to start on a
  non-loopback bind (the historical unauthenticated wildcard bind is
  closed — it was a footgun).
- With the feature, every PEER is classified at accept:
  - **loopback peers** keep the byte-identical plain-HTTP path above
    (CLI, NMH, GUI, the desktop frontends — all unchanged, no token);
  - **non-loopback peers** must speak **TLS** (self-signed certificate,
    rustls, TLS 1.2 kept enabled for Android API 26–28) and present
    `Authorization: Bearer <token>` on EVERY endpoint (`/rpc`,
    `/events`, `/health`); plain-HTTP bytes to the TLS path are
    dropped at the handshake (no HTTP error — fail-closed); a
    missing/wrong token is `401` + close.
- Resource bounds for non-loopback peers only (loopback is exempt
  and unbounded, as before): at most 32 concurrent connections, a
  10 s read timeout until the first authorized request (then 300 s),
  and a 128-line / 64 KiB header cap (`431`).

### Secrets (`velocitydm-lan-secrets.json`)

The bearer token (32 random bytes, hex), the certificate + key PEMs
and the certificate's SHA-256 fingerprint live in their OWN file
beside the config (portable: exe dir; else
`$XDG_CONFIG_HOME|$HOME/.config/velocitydm/`) — never inside
`config.json` (the daemon rewrites that file on every settings
change). Created `0600` on Unix. The identity is STABLE across
restarts (TOFU pins survive); `VELOCITYDM_LAN_SECRETS` overrides the
path (ops/tests).

- The daemon prints the token + fingerprint on a LAN-mode start.
- `velocitydm lan-token` reprints both; `--rotate` replaces the token
  only (clients re-pair; TLS pins are kept). Deleting the secrets
  file regenerates the whole identity.

### Pairing (Android)

1. Desktop: `velocitydm daemon --addr 0.0.0.0:36990` (or a LAN IP) —
   note the token and fingerprint from the output.
2. Phone (VelocityDM app → Remote): host, port, token; optionally the
   fingerprint (pre-pins the daemon and closes the first-connect
   MITM window; otherwise the app pins on first use — compare it
   against `velocitydm lan-token` out-of-band).

**Threat model honesty:** the token grants FULL engine control
(settings incl. proxy configuration and arbitrary `outDir` writes) —
treat it like a password. TLS is self-signed + TOFU: a network
attacker positioned before the first pairing can intercept; manual
fingerprint entry defeats that. Plain SharedPreferences on the phone
(keystore encryption is a recorded follow-up). Rotation is `lan-token
--rotate`.

Spec: `docs/SPECS/S8-LAN-RPC-WAVE1.md` (gate 0018).

## Client notes

- Keep-alive is supported (bounded to 256 requests per connection);
  the CLI closes per call.
- `daemon.shutdown` is also exposed as the CLI subcommand
  `velocitydm shutdown`.
- The native messaging host wraps this RPC for the browser extension —
  see docs/EXTENSIONS.md.
