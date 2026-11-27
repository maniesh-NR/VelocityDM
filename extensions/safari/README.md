# Safari extension (S9 — preview, CI build witness only)

Status: the honest capability today is an **unsigned build witnessed on CI**
plus the capability matrix in `docs/EXTENSIONS.md` §Safari. Nothing here is
installable/distributable yet — that is S9-W2, the owner Apple-ID lane
(`docs/SPECS/S9-SAFARI-XCODE.md` rule 6). Gate record:
`harness/ASSESSMENTS/0019-s9-safari-xcode.md`.

## What exists

- `scripts/build-ext.sh` emits an unpacked `extensions/dist/safari/`
  variant: event-page background, `downloads`/`proxy` permissions stripped
  (Safari implements neither), version inherited from the chromium manifest.
- CI job `Safari app extension (Xcode conversion)` (non-required preview,
  `macos-latest`) converts that variant and builds the generated app +
  extension targets unsigned. It is the ONLY Apple witness — the dev
  sandbox is Linux; no local Xcode claims exist.

## Converting on your own Mac (W2)

The converter ships with Xcode on macOS only (Xcode 26 renames it
`safari-web-extension-packager`; probe both names):

```sh
bash scripts/build-ext.sh          # produces extensions/dist/safari/ (unpacked)
xcrun safari-web-extension-packager extensions/dist/safari \
    --project-location build/safari \
    --app-name VelocityDM \
    --swift --macos-only --copy-resources --no-open --no-prompt --force
```

(No `--bundle-identifier` here: on Xcode 26.6 the packager derives the app
id from `--app-name` while the flag drives the extension's base id — mixing
them fails the embedded-binary prefix validation, as the CI
evidence-then-fix cycle proved. Pass an explicit id only after verifying
your Xcode version's flag semantics; that control matters at W2 signing
time, not for a local build.)

Open the generated project, select a **free personal team** (or "Sign to
Run Locally") for BOTH targets, build, then in Safari: Settings ▸ Advanced ▸
*Show features for web developers* ▸ Develop ▸ *Allow Unsigned Extensions*,
and enable the extension under Settings ▸ Extensions. Distribution requires
Developer ID signing + notarization.

## Native messaging — the honest boundary

`runtime.sendNativeMessage`/`connectNative` in Safari routes **only to the
containing app's `SafariWebExtensionHandler`** (the host-name parameter is
ignored; there is no NMH registry lookup, no stdio host spawn, no
`allowed_origins` matching — Apple's "Messaging between the app and
JavaScript in a Safari Web Extension"). Until S9-W2 adds that Swift bridge
to the app target, every engine integration on Safari is honestly
UNAVAILABLE: adds from the menu/popup end in the extension's unreachable
state. The bridge must replicate `velocitydm-nmh`'s input validation (cmd
allowlist, `connPreset` validation, `browserProxy` shape check, `cookiesBy`
cap, no frame logging) or it becomes the weakest ingress to the loopback
daemon; content scripts cannot message the handler — the bridge is
background-mediated only.

(The old version of this README documented a Mozilla-path NMH JSON with a
`safari-web-extension://` origin. That configuration is consumed by no
engine and has been removed — see the gate record for the falsification.)

## Permissions Safari will actually ask about

Safari mediates `<all_urls>` via the per-site toolbar access popover
(content scripts do not run until granted), and since Safari 16.4 an
extension is turned off upon update if the new version requests more host
permissions. The variant requests: `webRequest`, `nativeMessaging`,
`storage`, `contextMenus`, `cookies`, `tabs`.
