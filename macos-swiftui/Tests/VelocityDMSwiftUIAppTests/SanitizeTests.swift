// VelocityDM SwiftUI frontend — sanitize payload tests (XCTest).
//
// Pins the rule-2 sanitize helper (the gtk3 `display_text` analog):
// C0 controls and DEL become spaces, content is never logged. The
// payload classes from the spec: markdown link, HTML-ish span, control
// chars, padding. Markdown/HTML are NOT stripped — they render
// literally because the only sanctioned view wraps Text(verbatim:),
// which never parses them; the sanitizer's job is control characters.

import XCTest

@testable import VelocityDMSwiftUIApp

final class SanitizeTests: XCTestCase {
    func testControlCharsBecomeSpaces() {
        XCTAssertEqual(vdmSanitize("a\nb"), "a b")
        XCTAssertEqual(vdmSanitize("a\tb"), "a b")
        XCTAssertEqual(vdmSanitize("a\rb"), "a b")
        XCTAssertEqual(vdmSanitize("a\u{00}b"), "a b")
        XCTAssertEqual(vdmSanitize("a\u{01}b"), "a b")
        XCTAssertEqual(vdmSanitize("a\u{1F}b"), "a b")
        XCTAssertEqual(vdmSanitize("a\u{7F}b"), "a b") // DEL
    }

    func testMarkdownLinkSurvivesLiterally() {
        // Not stripped — rendered literally via Text(verbatim:), so the
        // LocalizedStringKey markdown/link axis is unreachable.
        let raw = "[Invoices.pdf](https://evil.example)"
        XCTAssertEqual(vdmSanitize(raw), raw)
    }

    func testHtmlIshSpanSurvivesLiterally() {
        let raw = "<script>alert(1)</script>"
        XCTAssertEqual(vdmSanitize(raw), raw)
    }

    func testPaddingWhitespacePreserved() {
        // Padding spaces are not control characters — preserved as-is.
        XCTAssertEqual(vdmSanitize("  name.zip  "), "  name.zip  ")
    }

    func testPlainAndUnicodePassThrough() {
        XCTAssertEqual(vdmSanitize("report-2026.pdf"), "report-2026.pdf")
        // Above 0x7F scalars are untouched.
        XCTAssertEqual(vdmSanitize("naïve–名前.tar.gz"), "naïve–名前.tar.gz")
        XCTAssertEqual(vdmSanitize(""), "")
    }

    func testNewlinesCollapseWithinLine() {
        // A hostile multi-line payload flattens to one line of spaces.
        XCTAssertEqual(vdmSanitize("line1\n\nline3"), "line1  line3")
    }
}
