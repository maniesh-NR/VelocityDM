# Safari conversion guide

VelocityDM Companion can be converted into a Safari Web Extension with Apple's
first-party converter. All steps require macOS with Xcode.

## 1. Convert the package

```sh
unzip velocitydm-chrome-1.0.0.zip -d velocitydm-safari
cd velocitydm-safari
xcrun safari-web-extension-converter . --project-location ~/VelocityDmSafari \
    --app-name "VelocityDM Companion" --bundle-identifier local.velocitydm.companion \
    --ios-only false --macos-only true
```

The converter generates an Xcode project containing the extension and an empty
container app. Open it, set a development team, and build.

## 2. Native messaging differences

Safari supports native messaging, but the host must be registered for the
*containing app*, not globally:

- The extension id changes to the Safari-generated identifier
  (`local.velocitydm.companion.Extension` by default). Update the host manifest's
  `allowed_extensions` to that id.
- The host manifest is embedded in the container app bundle under
  `Contents/Resources/` and referenced from the app's `Info.plist` settings the
  converter generates.
- The message bridge differs: instead of `chrome.runtime.connectNative`, Safari
  routes messages through `browser.runtime.sendNativeMessage` in the extension
  and `SFExtensionMessage` handling in `SafariWebExtensionHandler`. For a
  stateful long-lived port, buffer requests in the handler and forward them to
  the same frame protocol (`u32` LE length prefix + JSON) over a persistent
  connection to `127.0.0.1:36990`.

## 3. API differences checklist

- `chrome.*` is available as `browser.*` (WebExtension APIs); the converter adds
  the polyfill automatically.
- `webRequest` observation works, but Safari requires the user to grant per-site
  permission on first use; `onHeadersReceived` sees only granted sites.
- `chrome.storage.session` is supported in Safari 16.4+; the background code
  already falls back to memory when it is missing.
- `alarms` minimum period is enforced more strictly; the 4-minute keepalive is
  within limits.
- Context menus work; `selection` context behaves the same.

## 4. Testing

1. Build and run the container app once (it requests permission to run the
   extension).
2. Enable the extension in Safari > Settings > Extensions, grant "All websites".
3. Verify the popup shows daemon status (daemon must be running on
   `127.0.0.1:36990` with the NMH registered for the app), then capture a video
   URL and add it from the popup.

## 5. Notarization

For distribution outside the App Store, sign the container app with a Developer
ID certificate and notarize it (`xcrun notarytool submit`). The native host
binary must be signed with the same team id; harden it with
`--options runtime`.
