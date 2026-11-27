# S9 — Safari native app extension (Xcode conversion), wave 1

Gate: `ASSESSMENTS/0019-s9-safari-xcode.md` (2026-11-27, five roles, one
BLOCK resolved by folding its unblock path). Owner context: D5x/D4x/S10x
deferred (round 5, `harness/DECISIONS.md`) — the autonomous queue owns W1;
W2 is the owner Apple-ID lane. Shape: (b)+(d) hybrid — converter-driver CI
witness with ephemeral output + honest capability matrix + Safari manifest
variant. A committed hand-written `.xcodeproj` is OUT (nondeterministic
converter format across Xcode generations, zero local verify ability).

## Rules

1. **Prerequisite crash fix ships first, alone.** `background.js:93`
   `chrome.webRequest.onResponseHeaders` (an event that exists in no engine)
   becomes `onHeadersReceived`. The top-level `chrome.downloads.onCreated`
   and `chrome.downloads.onDeterminingFilename` registrations become
   feature-guarded (same pattern as the existing `applyShelf` guard): when
   the API is absent the intercept category degrades by omission and a
   comment states it. This fixes Chromium/Edge/Opera (event name), Firefox
   (`onDeterminingFilename` absent) and de-crashes Safari. No behavior
   change on Chromium beyond the event rename — `onHeadersReceived` with
   `['responseHeaders']` supplies the same `details.responseHeaders` the
   handler reads.
2. **Safari manifest variant** (`scripts/build-ext.sh`, mirroring the
   Firefox variant shape): `background.scripts` event-page list (same
   twelve-module order as Firefox, `importScripts` call replaced by the
   same comment), permissions = chromium's minus `downloads` and `proxy`
   (Safari lacks the first; the second is unverified — B16 stays guarded
   and its absence is already a designed degradation), `minimum_chrome_version`
   dropped, `browser_specific_settings` NOT added (no gecko id for Safari),
   version DERIVED from the chromium manifest (frozen `1.0.0` — never
   bumped), `web_accessible_resources` + `content_scripts` + `host_permissions`
   verbatim. Output: unpacked `extensions/dist/safari/` directory ONLY —
   **never a zip** (the release glob `extensions/dist/*.zip` and the
   explicit publish mapping must both stay unable to pick it up).
3. **Battery tripwires** (append-only section in
   `extensions/tests/run-tests.mjs`; pure node; the required "Extension
   tests" job keeps its command and its green):
   a. `node --check` syntax on every shipped extension JS (background,
      content, popup, options, all shared modules). Honest note recorded in
      the battery comment: the S9 gate bug PARSED clean — syntax checks are
      necessary, not sufficient.
   b. Denylist: the string `onResponseHeaders` appears in NO shipped JS.
   c. Unguarded-listener denylist: no line of any shipped `background.js`
      may START (at statement level) with `chrome.downloads.` — every
      downloads reference must be feature-guarded or lazy-inside-a-guard.
   d. Safari-variant derivation rules asserted in-memory (the same mapping
      build-ext.sh performs): event-page form, no `downloads`/`proxy`/
      `minimum_chrome_version`, twelve-script list order, version inherited.
   e. NMH id-match invariant: `packaging/nmh/com.velocitydm.nmh.firefox.json`
      `allowed_extensions` == the gecko id build-ext.sh emits ==
      the allowlist `packaging/macos/make-app.sh` generates (catches the
      `.local`/`.example` class at test time forever).
   f. Guard: `crates/velocitydm-nmh` still declares the `velocitydm-nmh`
      bin target (the macOS binary story cannot silently vanish).
   g. Assertion-count sync duty: the new printed total replaces the docs
      numbers in the same stage (240 → N; three sites: REQUIREMENTS-COVERAGE
      rows + EXTENSIONS.md).
4. **CI witness** (additive, NON-required preview job; the 8 required check
   names stay byte-identical): name `Safari app extension (Xcode conversion)`,
   `macos-latest`. Steps: checkout → `bash scripts/build-ext.sh` → probe
   `xcrun -f safari-web-extension-packager` else
   `xcrun -f safari-web-extension-converter` (fail only if NEITHER exists)
   → headless convert of `extensions/dist/safari` into
   `extensions/safari/build/` (gitignored; ephemeral — never committed,
   never diffed) with `--app-name VelocityDM --bundle-identifier
   com.velocitydm.safari --swift --macos-only --copy-resources --no-open
   --no-prompt --force` → shape assertions on `xcodebuild -list` (an app
   scheme and an `Extension` target exist) → unsigned build
   `CODE_SIGNING_ALLOWED=NO CODE_SIGN_IDENTITY= CODE_SIGNING_REQUIRED=NO`,
   destination `generic/platform=macOS` → artifact upload of the built
   `.app` (optional, non-load-bearing). Converter manifest warnings are
   INFORMATIONAL (never fail the job on them — Apple's documented behavior).
5. **Honest claims**: zero local Apple verification (the dev sandbox is
   Linux); CI macos-latest is the ONLY witness and witnesses COMPILE +
   CONVERT only — never "works in Safari", never "installable" (loading
   requires signing: at minimum Develop ▸ Allow Unsigned Extensions on the
   building machine; distribution requires Developer ID + notarization).
   Engine integration on Safari is UNAVAILABLE in W1: `sendNativeMessage`
   routes to the containing app's `SafariWebExtensionHandler` (host name
   ignored — Apple), which does not exist until W2's Swift bridge. Until
   then Safari shows capture/UI only and every add path ends in the
   extension's honest unreachable state. Content scripts cannot message the
   handler; the bridge is background-mediated only, and it MUST replicate
   `velocitydm-nmh`'s input validation (cmd allowlist, connPreset,
   browserProxy shape, cookiesBy cap, no frame logging) or it becomes the
   weakest ingress to the loopback daemon.
6. **W2 (owner lane, own future gate, ROADMAP never DONE while open)**:
   free personal-team / "Sign to Run Locally" build on the owner's Mac →
   Safari ▸ Settings ▸ Advanced ▸ Show Develop menu ▸ Allow Unsigned
   Extensions ▸ enable; real extension UUID → NMH/registration pinning; the
   `SafariWebExtensionHandler` bridge; owner runtime verification
   (capture → add → daemon round trip; takeover UX is N/A on Safari).
7. **Docs truth** lands in this stage: REQUIREMENTS-COVERAGE row split
   (four browsers Done with the corrected assertion count; Safari = preview
   row), EXTENSIONS.md §Safari rewritten to the capability matrix + the
   corrected count, `extensions/safari/README.md` rewritten (converter
   macOS-only, packager rename, the NMH recipe deleted, "no code changes
   required" deleted, the honest W1/W2 boundary), README.md / docs/README.md /
   BUILDING.md / IMPROVEMENTS.md wording rows, ROADMAP S9 → ACTIVE with the
   W1/W2 split spelled out.
8. **Release rules**: no release.yml contact; no tag; frozen versions
   untouched (`versionCode`/`versionName`/manifest `1.0.0`); the 8 required
   check names untouched; preview-job red does NOT block tag cuts. Flake
   policy: pinned-invocation-unchanged + shape-assertions-unchanged + rerun
   green = the documented runner-flake class; converter-output churn under
   an unchanged pinned invocation is NEW evidence (spec amendment or Xcode
   pin), never silently rerun through.
9. **Capability matrix (docs source of truth, panel-cited)**:
   Safari 15.4+ floor (MV3 manifest; the event-page variant avoids the
   15.4–16.3 importScripts breakage entirely); `storage.session` 16.4+
   (below: sniff list silently empty — pushVideo rejections are swallowed
   by design; the matrix states the floor); responseHeaders delivery 18.4+
   (below: MIME/length sniffing degrades to URL-extension matching; DOM
   detection unaffected); `downloads`/`proxy`/`userScripts`/`notifications`
   absent (takeover categories off; B16 report carries no endpoint);
   `contextMenus`/`cookies`/`tabs`/`nativeMessaging`/WAR supported.
