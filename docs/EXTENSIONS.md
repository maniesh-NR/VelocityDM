# Browser integration

The Companion extension (extension/) detects downloads and hands them to the
desktop daemon through the native messaging host `com.velocitydm.nmh`.

## Pipeline

1. **Context menu** - "Download with VelocityDM" on links, video, audio, and
   selected URLs. Manual adds bypass the sniffing gates (explicit intent) and
   only require http(s).
2. **Sniffing** - `webRequest` observation (non-blocking, MV3-safe) on
   `media`, `xmlhttprequest`, and `object` requests. Response headers
   (Content-Type / Content-Length / Content-Disposition) feed the capture
   decision: scheme and method gates, size gate (default 1 MB, tunable),
   category toggles (video, audio, archive, document, software;
   image-large/opt-in, other/off), and a hostname blocklist. HTML responses
   are never captured unless the URL extension is downloadable.
3. **Dedupe** - a bounded ring (500 entries) prevents repeat captures; the
   capture list (max 100) lives in `chrome.storage.session` with an in-memory
   fallback.
4. **Native messaging** - one long-lived port to `com.velocitydm.nmh`;
   requests are matched by RPC id with a 10 s timeout; the host forwards
   daemon events (completed/failed) into browser notifications.
5. **Popup** - daemon status, manual add, capture list with per-item and
   bulk download.

## Host registration

The NMH manifest (`com.velocitydm.nmh.json`) points to the host binary and
lists the allowed extension ids:

```json
{
  "name": "com.velocitydm.nmh",
  "description": "VelocityDM native messaging host",
  "path": "/usr/local/bin/velocitydm-nmh",
  "type": "stdio",
  "allowed_extensions": ["velocitydm@velocitydm.local"],
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

## Protocol summary

Frames both ways are `u32 LE length + UTF-8 JSON`:

- extension -> host: `{"type":"hello"}` and `{"type":"rpc","id":N,"method":...,"params":{...}}`
- host -> extension: `{"type":"hello","name":"velocitydm-nmh","version":...,"daemon":"running"}`
  , `{"type":"rpcResult","id":N,"ok":true,"result":...}`, and
  `{"type":"event","event":"completed","params":{...}}`

See docs/RPC.md for the daemon-side protocol (identical frame shape on TCP).
