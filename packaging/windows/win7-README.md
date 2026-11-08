# Windows 7 payload (x64 / x86)

These binaries are built with Rust 1.75 (the last toolchain whose standard
library runs on Windows 7) and therefore contain **no GUI**: the payload is

- `velocitydm.exe` — daemon + full CLI (`daemon`, `add`, `list`, `torrent`,
  `magnet`, `seed`, ...). TLS is disabled in this build; plain HTTP and the
  loopback RPC work normally.
- `velocitydm-nmh.exe` — the browser native messaging host.

The browser extension popup acts as the UI, exactly like the original
VelocityDM Win7 design: install the extension, point its native host at
`velocitydm-nmh.exe`, and the daemon starts on demand.

## Setup

1. Copy both binaries to e.g. `C:\Program Files\VelocityDM7\`.
2. Register the native messaging host (per browser, per user). Save as
   `com.velocitydm.nmh.json` next to the exe and reference it from the
   registry:

   ```reg
   [HKEY_CURRENT_USER\Software\Google\Chrome\NativeMessagingHosts\com.velocitydm.nmh]
   @="C:\\Program Files\\VelocityDM7\\com.velocitydm.nmh.json"
   ```

   The manifest:

   ```json
   {
     "name": "com.velocitydm.nmh",
     "description": "VelocityDM native messaging host",
     "path": "C:\\Program Files\\VelocityDM7\\velocitydm-nmh.exe",
     "type": "stdio",
     "allowed_origins": ["chrome-extension://YOUR_EXTENSION_ID/"]
   }
   ```

   Firefox instead uses
   `HKCU\Software\Mozilla\NativeMessagingHosts\com.velocitydm.nmh` with
   `"allowed_extensions": ["velocitydm@velocitydm.example"]`.

3. Load the extension zip (Chrome/Edge/Opera or Firefox), note its id, put
   the id into the manifest above.
4. First download: `velocitydm.exe daemon` (or let the NMH spawn it), then
   `velocitydm.exe add http://... --dir D:\Downloads` or simply click a
   download in the browser.

## Why no GUI here

egui/winit require a newer Win32/WGL path that the Rust 1.75-era std and
the Win7-capable toolchain cannot guarantee; the GUI ships in the
Windows 10+ builds (`windows-x64/`, `windows-x86/`).
