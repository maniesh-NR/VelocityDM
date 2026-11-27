# Assessment 0019 — S9 activation gate: Safari native app extension (Xcode conversion)

Date: 2026-11-27 (session 23). Five roles dispatched in parallel with identical
evidence packs (ATKR/FALS/REQ/INT/REL), independent research, WebSearch-cited
external facts. Evidence base: `extensions/chromium/background.js`,
`extensions/shared/*.js`, `extensions/safari/README.md`, `scripts/build-ext.sh`,
`packaging/nmh/*`, `packaging/macos/make-app.sh`, `.github/workflows/ci.yml`,
`docs/{REQUIREMENTS-COVERAGE,EXTENSIONS}.md`, live run of
`node extensions/tests/run-tests.mjs` (240/240 green at gate time).

## Verdicts

| Role | Verdict | Headline finding |
| --- | --- | --- |
| ATKR | PROCEED-WITH-CONDITIONS | Safari native messaging is categorically different from the README's NMH recipe; a naive containing-app proxy would become the weakest ingress to the loopback daemon |
| FALS | BLOCK (unblock path enumerated) | "No code changes are required" falsified on 4 axes; NMH-JSON-for-Safari recipe is dead config; universal background crash |
| REQ | PROCEED-WITH-CONDITIONS | Coverage row overclaims (Safari = guide only; "64 assertions" doubly stale); demands W1/W2 split and $0 owner path |
| INT | PROCEED-WITH-CONDITIONS | Cheap real CI witness exists: converter (renamed `safari-web-extension-packager` in Xcode 26) + unsigned xcodebuild on macos-latest |
| REL | PROCEED-WITH-CONDITIONS | No release asset ever (unsigned Safari artifact is not loadable); docs reconciliation is a gate condition, not cleanup |

Consensus resolution: the FALS BLOCK is resolved by folding its enumerated
unblock path into the spec BEFORE any implementing commit. Shape adopted:
**(b)+(d) hybrid** — committed converter-driver invocation executed by a new
non-required CI preview job (witness, ephemeral output) PLUS the honest
capability matrix / Safari-specific manifest variant as deliverables; a
committed hand-written `.xcodeproj` is rejected (drift, zero local verify
ability); docs-only is rejected (leaves the stage's stated purpose with zero
witness). The containing-app `SafariWebExtensionHandler` bridge is EXCLUDED
from W1 and re-gated later (ATKR condition: it must replicate NMH-parity
input validation; content scripts cannot message the handler).

## Load-bearing findings (all verified in-tree + cited)

1. **Universal background crash (all five shipped browsers).**
   `background.js:93` registers `chrome.webRequest.onResponseHeaders` — an
   event that exists in NO engine (developer.chrome.com webRequest reference
   lists 9 events, none of this name; MDN BCD agrees). Top-level
   `.addListener` on `undefined` throws at service-worker/event-page
   evaluation → no menus, no sniffing, no message router, no NMH link on
   Chromium/Edge/Opera. Firefox additionally dies at `background.js:208`
   (`chrome.downloads.onDeterminingFilename` is Chromium-only); Safari dies
   at both plus `background.js:186` (`chrome.downloads` absent entirely —
   MDN BCD: `version_added: false`). The 240-assertion battery never
   evaluates `background.js`, so CI structurally could not catch this.
   Byte-level (od) verification performed this session; the neighboring
   `headers[h.name.toLowerCase()]` line is CORRECT code (initially
   mis-suspected as a syntax error through an output-rendering artifact;
   disproven before any fix was attempted).
2. **Safari API deltas (Apple docs + MDN BCD cited by the panel):**
   `chrome.downloads` absent; `chrome.proxy` unverified-partial (Safari 17
   note covers WKWebView, not Safari; code already null-guards);
   `storage.session` 16.4+; MV3 service workers 15.4+ with importScripts in
   SW broken 15.4–16.3 (16.4 release note); `webRequest` responseHeaders
   delivery via extraInfoSpec 18.4+ (older = URL-based sniffing only, DOM
   sniffing unaffected); `nativeMessaging` = containing-app
   `SafariWebExtensionHandler` ONLY (applicationID ignored — Apple's
   "Messaging between the app and JavaScript in a Safari Web Extension");
   `chrome.*` namespace supported (Apple compat article); `contextMenus`
   macOS OK.
3. **The README's NMH recipe is dead config**: the Mozilla-path NMH JSON
   with a `safari-web-extension://` origin is consumed by no engine
   (`safari-web-extension://` is WebKit's extension-resource origin scheme;
   the Mozilla dir is a Firefox mechanism). `make-app.sh` registers it for
   Firefox only (correct there).
4. **Firefox NMH id mismatch (B24-class access-denied bug, found at gate)**:
   `scripts/build-ext.sh` gecko id `velocitydm@velocitydm.example` (matches
   `packaging/nmh/com.velocitydm.nmh.firefox.json`) vs
   `packaging/macos/make-app.sh` generated allowlist
   `velocitydm@velocitydm.local` → Firefox-on-macOS native messaging
   access-denied. Folded as a fix.
5. **CI toolchain facts (INT, cited)**: the converter was renamed
   `safari-web-extension-packager` (Apple's "Packaging a web extension for
   Safari"; old URL 301-redirects); flags `--project-location --app-name
   --bundle-identifier --swift --macos-only --copy-resources --no-open
   --no-prompt --force`; warnings are informational. `macos-14` retired
   2026-11-02; `macos-latest` = macOS 26 arm64, Xcode 26.6; `macos-15`
   fallback = Xcode 16.4 (old converter name primary). Unsigned build via
   `CODE_SIGNING_ALLOWED=NO` (+ belt-and-braces identity/required = no);
   converter output is NOT deterministic across Xcode generations →
   generated project is ephemeral (gitignored), shape-based assertions only.

## Folded conditions → spec (`docs/SPECS/S9-SAFARI-XCODE.md`)

All five roles' conditions are folded there, including: the universal crash
fix as a standalone prerequisite commit; the Safari variant (event-page
form, `downloads`/`proxy`/`minimum_chrome_version` dropped, version derived
not bumped, unpacked dir only — never a zip); battery tripwires (syntax
checks on every extension JS, `onResponseHeaders` denylist, unguarded
top-level `chrome.downloads.` denylist, Safari-variant derivation rules,
NMH id-match invariant incl. the make-app.sh fix, `velocitydm-nmh` bin
guard); the CI preview job spec (non-required, name
`Safari app extension (Xcode conversion)`, probe-both-tool-names, pinned
flags, unsigned build, shape assertions, warnings informational); the W2
split (signed loadable build + NMH UUID pinning + owner runtime verification
+ the handler bridge = owner Apple-ID lane, its own future gate, ROADMAP
never DONE while W2 is open); the docs reconciliation table; the release
rules (no release.yml contact, frozen assets/versions untouched, preview red
never blocks tag cuts, flake policy = pinned-invocation-unchanged + rerun).

## Dispositions

- Bench A/B: **not owed** — zero engine delta (extensions/ + CI + docs only).
- CLEAN_ROOM row: **not owed** — first-party extension packaging; no S11/T2
  linkage anywhere in scope.
- Local battery: `run-tests.mjs` grows an append-only S9 section (pure node,
  required job command unchanged); CI is the only Apple witness (zero local
  Xcode claims permitted).
- **Session-24 recovery note:** a sandbox recycle between sessions destroyed
  the unpushed C1/C2a/C2b commits of session 23 (the repo snapshot restored
  to 20a6af4; the pushed round-5 commit 0f49b0b survived on origin). This
  record and the spec were re-created byte-equivalent from the session
  transcript; the discipline change (push after EVERY commit, recorded in
  MEMORY) is the direct outcome.
