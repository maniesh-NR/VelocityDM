// VelocityDM SwiftUI frontend — attacker-controlled text rendering
// (S12-mac2 spec rule 2, the SwiftUI analog of the gtk3 gate's
// set_text()-only rule).
//
// SwiftUI hazard pinned by gate 0013 (ATKR): `Text`'s default string
// initializers bind to `LocalizedStringKey`, which parses Markdown —
// and on macOS 12+ Markdown links render as TAPPABLE Links. A hostile
// task name `[Invoices.pdf](https://evil.example)` becomes a phishing
// element inside a download manager. Every daemon-relayed string
// (name, url, dir, outPath, error, notice, task-completed path, RPC
// and transport errors, banners embedding them) therefore renders
// through `VDMText`, which (a) sanitizes control characters and (b)
// wraps `Text(verbatim:)` — the literal-only initializer.
//
// Banned everywhere in this tree: string interpolation inside
// `Text("…\(x)…")` with daemon data, `Text` concatenation (`+`) with
// daemon parts, `AttributedString(markdown:)` on daemon data, and any
// tappable link built from a daemon string.

import SwiftUI

/// Control-character sanitizer (the gtk3 `display_text` analog):
/// literal rendering discipline — C0 controls and DEL become spaces,
/// content is never logged.
public func vdmSanitize(_ raw: String) -> String {
    var out = String()
    out.reserveCapacity(raw.count)
    for scalar in raw.unicodeScalars {
        if scalar == "\n" || scalar == "\t" {
            out.unicodeScalars.append(" ")
        } else if scalar.value < 0x20 || scalar.value == 0x7F {
            out.unicodeScalars.append(" ")
        } else {
            out.unicodeScalars.append(scalar)
        }
    }
    return out
}

/// The ONLY sanctioned way to render daemon-relayed strings in this
/// tree. Always literal: no Markdown parsing, no localization keys,
/// no link activation.
public struct VDMText: View {
    private let storage: Text

    public init(_ raw: String) {
        self.storage = Text(verbatim: vdmSanitize(raw))
    }

    public var body: some View { storage }
}
