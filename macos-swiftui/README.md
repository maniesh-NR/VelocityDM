# VelocityDM SwiftUI frontend (S12-mac2, preview)

The SwiftUI variant of the macOS frontend — the "macOS newer" stack of
the platform matrix. Ships in NO release asset until its own promotion
gate; `macos-app/` (Swift + AppKit, floor macOS 10.15) remains the
shipped macOS frontend and Monterey users keep it.

## What this is

- Standalone SwiftPM package, `swift-tools-version:5.9`, floor
  **macOS 12** (SwiftUI `@main App` needs 11+; `.task`/`.onSubmit`/
  `.refreshable` need 12+; 13-only APIs are forbidden unguarded — see
  `docs/SPECS/S12-MAC2-SWIFTUI.md` rule 9).
- Zero third-party dependencies — SwiftPM writes no `Package.resolved`.
- Remote client of the shared Rust core (daemon binary) over the
  documented loopback protocol (JSON-RPC POST /rpc + SSE GET /events on
  127.0.0.1:36990, `docs/RPC.md`). Start the daemon first:

  ```sh
  velocitydm daemon --addr 127.0.0.1:36990
  cd macos-swiftui && swift run VelocityDMSwiftUIApp
  ```

- Material You chrome from the Swift port of the shared `vdm-md3` token
  engine: **18 of the 27 Scheme roles** (honest subset, same as the
  AppKit tree — NOT full token parity). Seed cycle + light/dark toggle
  persist under the SAME UserDefaults keys as the AppKit app
  (`vdm.seed`, `vdm.dark`).
- Parity surface: the promotion-gates Swift column as the floor PLUS the
  two registered Swift gaps closed as real UI — filename override
  (`downloads.add` `options.filename`) and local `.torrent` add
  (`downloads.addTorrent` top-level `torrentPath`).

## Build + test

```sh
swift build    # the app
swift test     # golden-literal + frame-split + parse + sanitize suites
```

`swift build` alone does NOT compile test targets — `swift test` is
where the golden tests actually run. CI witnesses both steps in the
non-required `swiftui-frontend` job (macos-latest); a red
`swiftui-frontend` does not block tag cuts (it ships nothing).

**Honest local-verification sentence:** the dev sandbox is Linux with
no Swift toolchain — no local build, test, or format of this tree is
possible; CI is the sole witness (spec rule 11). No record anywhere
claims otherwise.

## Copy provenance + drift risk (spec rule 12)

Three files are COPIES from `macos-app/` (adapted — never claimed as
identical reuse):

| File | Copy | Adaptation |
| --- | --- | --- |
| `Sources/VelocityDMSwiftUIApp/Md3.swift` | math core verbatim | NSColor/CGColor bridges replaced by a SwiftUI `Color` bridge; header claim corrected to the honest 18/27 subset |
| `Sources/VelocityDMSwiftUIApp/RpcClient.swift` | protocol surface | `@MainActor` isolation; dedicated cookie-less session + redirect refusal; `filename` in `options`; `addTorrent` params at TOP LEVEL (the AppKit `"options": [:]` nesting is a silently-ignored key and is NOT copied); `TaskSnapshot.notice` additive field |
| `Sources/VelocityDMSwiftUIApp/SseClient.swift` | stream contract | cookie-less session + redirect refusal added; frame-split/parse extracted into the pure testable helpers `SseFrameBuffer` / `SseFrames` |

**Canonical-copy rule:** the AppKit tree's `Md3.swift` math is
canonical for Swift ports. **Color-math/seed drift rule:** any change to
`vdm-md3` color math or seeds patches Rust → Kotlin → BOTH Swift trees
in the SAME commit and extends `scripts/gen-md3-baseline.py`'s
provenance sentence. The SwiftPM golden-literal test target
(`Tests/VelocityDMSwiftUIAppTests/Md3GoldenTests.swift`, 288 hardcoded
values copied from the Rust-generated baseline) is the Swift-side
tripwire the chain otherwise lacks; regenerate it with
`scripts/gen-md3-golden-swift.py`.

The macos-app library+executable split (single source of truth via a
shared library product) is the recorded promotion-gate end-state — not
this stage.

## Rendering safety

Every daemon-relayed string (task name, url, dir, outPath, error,
notice, SSE paths, RPC/transport errors, banners embedding them)
renders through `VDMText` (DisplayText.swift) — control-character
sanitize + `Text(verbatim:)`. The SwiftUI hazard this closes:
`Text`'s default initializers bind to `LocalizedStringKey`, which
parses Markdown — on macOS 12+ Markdown links render as tappable
Links, so a hostile task name `[Invoices.pdf](https://evil.example)`
would become a phishing element. Banned in this tree: string
interpolation inside `Text("…\(x)…")` with daemon data, `Text`
concatenation (`+`) with daemon parts, `AttributedString(markdown:)`
on daemon data, tappable links from daemon strings.
