// VelocityDM SwiftUI frontend — client parse tests (XCTest).
//
// Byte-parse pins mirror vdm_core::util::parse_bytes' tolerant input
// (1024-based single-letter suffixes, 1000-based kb/mb/gb, 1024-based
// KiB forms — the gate-0012 truth that parse_bytes("2M") is 2 MiB =
// 2_097_152) and RpcProxy.parse's structured round-trip (http/socks5,
// range-validated port, "none"/empty clearing).

import XCTest

@testable import VelocityDMSwiftUIApp

final class ClientParseTests: XCTestCase {
    // MARK: byte-parse

    func testParseBytes1024BasedSingleLetter() {
        // The gate-0012 truth: single-letter suffixes are 1024-based.
        XCTAssertEqual(String.parseBytes("2M"), 2_097_152)
        XCTAssertEqual(String.parseBytes("2m"), 2_097_152)
        XCTAssertEqual(String.parseBytes("1k"), 1024)
        XCTAssertEqual(String.parseBytes("1.5g"), 1_610_612_736)
    }

    func testParseBytes1000BasedLongForms() {
        XCTAssertEqual(String.parseBytes("2MB"), 2_000_000)
        XCTAssertEqual(String.parseBytes("2mb"), 2_000_000)
        XCTAssertEqual(String.parseBytes("1kb"), 1000)
    }

    func testParseBytesKiBForms() {
        XCTAssertEqual(String.parseBytes("2MiB"), 2_097_152)
        XCTAssertEqual(String.parseBytes("1KiB"), 1024)
        XCTAssertEqual(String.parseBytes("1GiB"), 1_073_741_824)
    }

    func testParseBytesPlainAndEmpty() {
        XCTAssertEqual(String.parseBytes("500"), 500)
        XCTAssertEqual(String.parseBytes("0"), 0)
        // Empty = none (spec §5: empty = no limit) — parseBytes returns
        // 0 and 0 never reaches the wire (rateLimitBps omitted).
        XCTAssertEqual(String.parseBytes(""), 0)
        XCTAssertEqual(String.parseBytes("   "), 0)
    }

    func testParseBytesGarbage() {
        XCTAssertNil(String.parseBytes("off"))
        XCTAssertNil(String.parseBytes("abc"))
        XCTAssertNil(String.parseBytes("-5"))
        XCTAssertNil(String.parseBytes("2Xb"))
    }

    // MARK: proxy parse

    func testProxyParseHttp() {
        let p = RpcProxy.parse("http://127.0.0.1:8080")
        XCTAssertEqual(p?.type, "http")
        XCTAssertEqual(p?.host, "127.0.0.1")
        XCTAssertEqual(p?.port, 8080)
    }

    func testProxyParseSocks5() {
        let p = RpcProxy.parse("socks5://proxy.lan:1080")
        XCTAssertEqual(p?.type, "socks5")
        XCTAssertEqual(p?.host, "proxy.lan")
        XCTAssertEqual(p?.port, 1080)
    }

    func testProxyParseCaseAndWhitespace() {
        let p = RpcProxy.parse("  HTTP://Host:3128 ")
        XCTAssertEqual(p?.type, "http")
        XCTAssertEqual(p?.host, "Host")
        XCTAssertEqual(p?.port, 3128)
    }

    func testProxyParseRejects() {
        XCTAssertNil(RpcProxy.parse("none")) // explicit clear sentinel
        XCTAssertNil(RpcProxy.parse(""))     // empty clears
        XCTAssertNil(RpcProxy.parse("http://no-port"))
        XCTAssertNil(RpcProxy.parse("http://host:99999")) // out of range
        XCTAssertNil(RpcProxy.parse("ftp://host:21"))     // unsupported scheme
    }

    func testProxyDisplay() {
        let p = RpcProxy(type: "http", host: "h", port: 1)
        XCTAssertEqual(p.display, "http://h:1")
    }
}
