# VelocityDM Companion (browser extension)

MV3 extension that hands downloads to the VelocityDM desktop daemon.

## Features

- Context menu entry **Download with VelocityDM** for links, video, audio, and selected URLs.
- Media sniffing: observes `media`, `xmlhttprequest`, and `object` requests and captures
  video/audio/HLS/archive/document/software responses that clear the size gate.
- Filename extraction from the URL path and `Content-Disposition` (including RFC 5987 `filename*`).
- Popup with the captured list, manual URL add, daemon status, and download-all.
- Options page: daemon host/port, sniffing toggle, size gate, category toggles, blocklist,
  and a connection test.
- Native messaging bridge to the host `com.velocitydm.nmh` (frame protocol: u32 LE length
  prefix + UTF-8 JSON; message types `hello`, `rpc`, `rpcResult`, `event`).
- Completion/failure notifications forwarded from daemon events; badge counter for captures.

## Browser packages

Built by `bun build.mjs` into `dist/`:

| Package | Notes |
| --- | --- |
| `velocitydm-chrome-1.0.0.zip` | Chromium base (also works in Chromium forks) |
| `velocitydm-edge-1.0.0.zip` | Same code, Edge-branded description |
| `velocitydm-opera-1.0.0.zip` | Same code as Chrome |
| `velocitydm-firefox-1.0.0.zip` | Firefox overlay manifest (event-page style background) |

Install unpacked: unzip the package, then load it via
`chrome://extensions` (Developer mode, "Load unpacked") or `about:debugging#/runtime/this-firefox`
("Load Temporary Add-on").

## Native host registration

The extension talks to `com.velocitydm.nmh`. Register the host binary shipped with the
desktop app (see `docs/EXTENSIONS.md` in the main repository or `packaging/`):

- Linux: `~/.config/google-chrome/NativeMessagingHosts/com.velocitydm.nmh.json`
- Windows: registry key `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.velocitydm.nmh`
- Firefox: `~/.mozilla/native-messaging-hosts/com.velocitydm.nmh.json` (allowed extension id
  `velocitydm@velocitydm.local`)

## Tests

```sh
bun tests/run.mjs
```

The suite covers URL/filename classification, mime mapping, the capture decision, native
messaging framing (including split frames), settings migration, and the dedupe ring.
