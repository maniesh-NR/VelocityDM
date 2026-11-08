# Safari conversion guide

Apple's Safari supports WebExtensions converted with Xcode 14+ (also usable
from Linux to *prepare* the folder; the final signing step needs macOS).

## Steps

1. Open Xcode on a Mac, choose **File > New > Project > Safari Web
   Extension**.
2. Copy the contents of `dist/chromium/` (the built unpacked extension)
   into the generated `Resources/` folder of the new project.
3. Convert on macOS with:

   ```sh
   xcrun safari-web-extension-converter dist/chromium \
       --project-location . \
       --app-name VelocityDM \
       --bundle-identifier com.velocitydm.safari
   ```

4. Safari native messaging hosts use the app-group container or:
   `~/Library/Application Support/Mozilla/NativeMessagingHosts/`.
   Register the `velocitydm-nmh` binary from the macOS bundle:

   ```json
   {
     "name": "com.velocitydm.nmh",
     "description": "VelocityDM native host",
     "path": "/Applications/VelocityDM.app/Contents/MacOS/velocitydm-nmh",
     "type": "stdio",
     "allowed_origins": ["safari-web-extension://VELOCITYDM-EXTENSION-UUID/"]
   }
   ```

   Replace `VELOCITYDM-EXTENSION-UUID` with the UUID shown in Safari's
   Extensions developer settings.

5. Build and sign the app (`Product > Archive`), then enable it in
   Safari's Extensions preferences.

## Notes

- No code changes are required: the shared modules are plain scripts and
  the background worker uses only standard WebExtension APIs
  (`runtime.sendNativeMessage`, `webRequest`, `downloads`, `storage`).
- The `tabs` permission is used only to message the active tab for link
  grabbing; Safari will show the standard permission prompt.
