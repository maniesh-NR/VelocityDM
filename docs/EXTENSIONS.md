# Browser integration

The VelocityDM browser extension (`extensions/` — one Chromium MV3 codebase
packaged for Chrome, Edge, Opera and Firefox) detects downloads and hands
them to the desktop daemon through the native messaging host
`com.velocitydm.nmh`.

```sh
bash scripts/build-ext.sh   # builds extensions/dist/{chromium,edge,opera,firefox}-<version>.zip
```

## Pipeline

1. **Context menu** — "Download with VelocityDM" on links, video, audio and
   selected URLs. Manual adds bypass the sniffing gates (explicit intent)
   and only require http(s).
2. **Sniffing** — `webRequest` observation (non-blocking, MV3-safe) on
   `media`, `xmlhttprequest` and `object` requests. Response headers
   (Content-Type / Content-Length / Content-Disposition) feed the capture
   decision: scheme and method gates, size gate (default 1 MB, tunable),
   category toggles (video, audio, archive, document, software;
   image-large/opt-in, other/off), and a hostname blocklist. HTML responses
   are never captured unless the URL extension is downloadable.
3. **Dedupe** — a bounded LRU prevents repeat captures; the capture
   list (capped at 50) lives in `chrome.storage.session` with an in-memory
   fallback.
4. **Native messaging** — `chrome.runtime.sendNativeMessage` one-shot
   calls to `com.velocitydm.nmh` (no long-lived port; the extension
   re-polls `status` for progress).
5. **Popup + options** — daemon status, manual add, capture list with
   per-item and bulk download, Material You seed pickers (see below).

Shared logic lives in `extensions/shared/` (`detector`, `patterns`,
`links`, `filters`, `messaging`, `state`, `md3`); the Chromium service
worker loads them with `importScripts`, the Firefox event page via
manifest scripts (`scripts/build-ext.sh` rewrites this at package time).

## Host registration

The NMH manifest (`packaging/nmh/com.velocitydm.nmh.json`) points to the
host binary and lists the allowed extension ids:

```json
{
  "name": "com.velocitydm.nmh",
  "description": "VelocityDM native messaging host",
  "path": "/usr/local/bin/velocitydm-nmh",
  "type": "stdio",
  "allowed_extensions": ["velocitydm@velocitydm.example"],
  "allowed_origins": ["chrome-extension://<extension-id>/"]
}
```

Install locations:

| Browser | Linux | Windows |
| --- | --- | --- |
| Chrome/Edge/Opera | `~/.config/google-chrome/NativeMessagingHosts/` | `HKCU\Software\Google\Chrome\NativeMessagingHosts\` (same for Edge/Opera keys) |
| Chromium | `~/.config/chromium/NativeMessagingHosts/` | `HKCU\Software\Chromium\...` |
| Firefox | `~/.mozilla/native-messaging-hosts/` | `HKCU\Software\Mozilla\NativeMessagingHosts\` |

`packaging/linux/install.sh` writes the Linux manifests; the Inno Setup
script writes the Windows registry keys.

## Native messaging protocol

Frames both ways are `u32 LE length + UTF-8 JSON` on stdio. The host finds
the daemon via `VELOCITYDM_ADDR` (default `127.0.0.1:36990`) and will start
one if it is not running. Requests carry a `cmd`; replies carry `ok`:

```json
{"cmd":"ping"}
{"ok":true,"pong":true,"version":"1.0.0"}

{"cmd":"add","url":"https://example.com/file.zip","filename":"file.zip",
 "referer":"...","userAgent":"...","cookies":"...","headers":{...}}
{"ok":true,"id":7}

{"cmd":"addBatch","pattern":"https://x/pic[001:120].jpg"}   // or {"urls":[...]}
{"ok":true,"ids":[7,8,9]}

{"cmd":"addTorrent","magnet":"magnet:?..."}               // or {"torrentPath":"..."}
{"ok":true,"id":11}

{"cmd":"status"}
{"ok":true,"tasks":[ ...snapshots... ]}

{"cmd":"open"}        // launch the desktop GUI
{"ok":true}
```

`add`/`addBatch`/`addTorrent` map straight onto the daemon RPC
(`downloads.add` / `downloads.addBatch` / `downloads.addTorrent`) — see
docs/RPC.md for the full daemon surface.

## Material You (MD3) dynamic color

`extensions/shared/md3.js` is a clean-room MD3 dynamic-color engine: a seed
color is mapped through CIELAB tonal palettes into a full light/dark scheme
(role tokens). Popup and options offer eight preset seed swatches plus a
custom color entry; the mode (`auto` / `light` / `dark`) and seed persist in
`chrome.storage`. Auto mode follows `prefers-color-scheme`.

## Safari (preview — CI build witness only)

Safari is a **preview** stage (S9, gate 0019, `docs/SPECS/S9-SAFARI-XCODE.md`):
`scripts/build-ext.sh` emits an unpacked `extensions/dist/safari/` variant
(event-page background — dodges the Safari 15.4–16.3 service-worker
importScripts breakage; `downloads` and `proxy` permissions stripped because
Safari implements neither API), and the non-required CI job
"Safari app extension (Xcode conversion)" converts it with Apple's
`safari-web-extension-packager` (renamed from converter in Xcode 26) and
builds the generated app+extension targets unsigned. That is the entire
honest capability today:

| Capability | Safari status | Consequence |
| --- | --- | --- |
| Sniffing (URL/DOM) | works 15.4+; response-header MIME/length detail 18.4+ | below 18.4, URL-extension matching + DOM detection |
| Sniffed list | 16.4+ (`storage.session`) | below, the list stays empty (documented floor) |
| Context menus / cookies / tabs / popup UI | supported | parity |
| Download takeover (`downloads` API) | **absent** | categories off; adds go through menu/popup, not browser intercept |
| Browser-proxy report (B16) | **absent/unverified** | adds carry no `browserProxy` (designed degradation) |
| Engine integration (native messaging) | **unavailable in W1** | `sendNativeMessage` routes to the containing app's `SafariWebExtensionHandler` (host name ignored); the Swift bridge is S9-W2, its own gate, and must replicate `velocitydm-nmh`'s input validation |
| Loading the extension | requires signing | free personal-team/"Sign to Run Locally" + Develop ▸ Allow Unsigned Extensions on the owner's Mac (W2, owner Apple-ID lane); Developer ID + notarization for distribution |

## Tests

```sh
node extensions/tests/run-tests.mjs   # 253 assertions: shared modules + md3 scheme + S9 structural tripwires
```
