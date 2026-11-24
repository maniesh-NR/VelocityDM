// VelocityDM SwiftUI frontend — SSE frame-split + lenient-parse tests
// (XCTest). Pins the S12-SSE-WIRING §2 contract restated by S12-mac2
// rule 8 via the pure `SseFrameBuffer` / `SseFrames` helpers: frames
// straddle network reads; partial frames wait; the 1 MiB cap DROPS the
// buffer on a terminator-less overflow (never grows, never throws);
// `data:`-only lenient parsing; non-JSON / non-`type` frames are
// ignored, never fatal.

import XCTest

@testable import VelocityDMSwiftUIApp

final class SseFrameTests: XCTestCase {
    private func d(_ s: String) -> Data { Data(s.utf8) }

    func testSingleFrameSingleRead() {
        var buf = SseFrameBuffer()
        let frames = buf.append(d("data: {\"type\":\"hello\"}\n\n"))
        XCTAssertEqual(frames, ["data: {\"type\":\"hello\"}"])
        XCTAssertTrue(buf.pending.isEmpty)
    }

    func testFrameStraddlingReads() {
        var buf = SseFrameBuffer()
        XCTAssertTrue(buf.append(d("data: {\"typ")).isEmpty)
        XCTAssertTrue(buf.append(d("e\":\"tasks\"")).isEmpty)
        let frames = buf.append(d("}\n\n"))
        XCTAssertEqual(frames, ["data: {\"type\":\"tasks\"}"])
    }

    func testMultipleFramesOneRead() {
        var buf = SseFrameBuffer()
        let frames = buf.append(d("data: {\"type\":\"a\"}\n\ndata: {\"type\":\"b\"}\n\n"))
        XCTAssertEqual(frames.count, 2)
        XCTAssertEqual(SseFrames.parse(frames[0])?.type, "a")
        XCTAssertEqual(SseFrames.parse(frames[1])?.type, "b")
    }

    func testPartialTailRetained() {
        var buf = SseFrameBuffer()
        let frames = buf.append(d("data: {\"type\":\"a\"}\n\ndata: {\"type\":\"b"))
        XCTAssertEqual(frames.count, 1)
        XCTAssertEqual(buf.pending, d("data: {\"type\":\"b"))
        let frames2 = buf.append(d("\"}\n\n"))
        XCTAssertEqual(frames2.count, 1)
        XCTAssertEqual(SseFrames.parse(frames2[0])?.type, "b")
        XCTAssertTrue(buf.pending.isEmpty)
    }

    func testOverflowDropsBuffer() {
        var buf = SseFrameBuffer()
        // A terminator-less buffer over the cap is dropped entirely.
        let junk = String(repeating: "x", count: SseFrameBuffer.cap + 1024)
        let frames = buf.append(d(junk))
        XCTAssertTrue(frames.isEmpty)
        XCTAssertTrue(buf.pending.isEmpty, "buffer must be dropped, not grown")
        XCTAssertEqual(buf.overflowDrops, 1)
        // The client survives: a normal frame after the drop parses.
        let frames2 = buf.append(d("data: {\"type\":\"ok\"}\n\n"))
        XCTAssertEqual(frames2.count, 1)
        XCTAssertEqual(buf.overflowDrops, 1)
    }

    func testDropFiresAtCapNotBefore() {
        var buf = SseFrameBuffer()
        _ = buf.append(d(String(repeating: "y", count: SseFrameBuffer.cap)))
        XCTAssertEqual(buf.overflowDrops, 0, "exactly at the cap: no drop")
        _ = buf.append(d("z"))
        XCTAssertEqual(buf.overflowDrops, 1, "one byte over: dropped")
    }

    // MARK: lenient parse (SseFrames)

    func testDataJoinStripsOneOptionalSpace() {
        XCTAssertEqual(SseFrames.dataPayload("data: {\"a\":1}"), "{\"a\":1}")
        XCTAssertEqual(SseFrames.dataPayload("data:{\"a\":1}"), "{\"a\":1}")
        XCTAssertEqual(SseFrames.dataPayload("data:  {\"a\":1}"), " {\"a\":1}")
        // Multi-line join of two data: fields.
        XCTAssertEqual(SseFrames.dataPayload("data: {\"a\":\ndata: 1}"), "{\"a\":1}")
    }

    func testNonJsonIgnored() {
        XCTAssertNil(SseFrames.parse("data: this is not json"))
        XCTAssertNil(SseFrames.parse("data: "))
        XCTAssertNil(SseFrames.parse(": comment only"))
    }

    func testMissingTypeIgnored() {
        XCTAssertNil(SseFrames.parse("data: {\"tasks\":[]}"))
        XCTAssertNil(SseFrames.parse("data: {\"type\":42}"))
    }

    func testOtherFieldsIgnored() {
        let frame = SseFrames.parse("event: tasks\nid: 7\ndata: {\"type\":\"tasks\",\"tasks\":[]}\n: ping")
        XCTAssertNotNil(frame)
        XCTAssertEqual(frame?.type, "tasks")
        XCTAssertEqual(frame?.taskDictionaries.count, 0)
    }

    func testTasksTickDecodesSnapshots() {
        let frame = SseFrames.parse(
            "data: {\"type\":\"tasks\",\"tasks\":[{\"id\":3,\"name\":\"a.zip\",\"state\":\"downloading\",\"done\":10,\"size\":20,\"speed\":5,\"error\":null,\"notice\":null}]}"
        )
        XCTAssertEqual(frame?.type, "tasks")
        XCTAssertEqual(frame?.taskDictionaries.count, 1)
    }
}
