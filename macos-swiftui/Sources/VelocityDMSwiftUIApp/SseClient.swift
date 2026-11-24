// VelocityDM SwiftUI frontend — SSE client for the daemon's GET /events.
//
// ADAPTED COPY of macos-app's SseClient (S12-mac2 spec rule 8 — never
// claimed as identical reuse). Deltas from the AppKit tree's copy:
//   1. Dedicated cookie-less URLSession configuration + redirect refusal
//      (spec rule 4 hardening pins, restated for this tree by rule 8)
//      instead of the plain default configuration.
//   2. The frame-split and parse logic is extracted into the pure,
//      unit-testable helpers `SseFrameBuffer` and `SseFrames` (the
//      spec-owed test target pins straddling reads, the 1 MiB drop cap
//      and the lenient data:-only rules — the AppKit copy keeps that
//      logic inline where no test can reach it).
//
// Consumes the documented stream (docs/RPC.md §GET /events): a
// text/event-stream of `data: <single-line JSON>` frames plus the 10 Hz
// coalesced `tasks` snapshot tick. The daemon sends no heartbeats, so a
// terminal transport error/EOF is the daemon-death signal the UI turns
// into its stale-rows state. The request is a plain native-client GET —
// no Origin/Referer header is ever set (loopback trust boundary,
// docs/RPC.md §Loopback trust boundary).

import Foundation

/// Pure frame assembler (spec §2 of docs/SPECS/S12-SSE-WIRING.md,
/// restated by S12-mac2 rule 8): complete frames are split on the
/// `\n\n` terminator (inclusive), frames may straddle network reads,
/// partial frames wait. The buffer is capped at 1 MiB: a
/// terminator-less buffer over the cap is DROPPED — never grown, never
/// thrown (a malformed or hostile stream cannot grow memory
/// unboundedly; the daemon never sends such frames, the cap is
/// defensive).
struct SseFrameBuffer {
    private(set) var pending = Data()
    /// Number of times the 1 MiB drop fired (observability only).
    private(set) var overflowDrops = 0

    static let cap = 1 << 20

    /// Append a network read; returns the text of every complete frame.
    mutating func append(_ data: Data) -> [String] {
        pending.append(data)
        var frames: [String] = []
        let terminator = Data("\n\n".utf8)
        while let r = pending.range(of: terminator, options: [],
                                    in: pending.startIndex..<pending.endIndex) {
            let frame = pending.subdata(in: pending.startIndex..<r.lowerBound)
            pending.removeSubrange(pending.startIndex..<r.upperBound)
            if let text = String(data: frame, encoding: .utf8) {
                frames.append(text)
            }
        }
        if pending.count > Self.cap {
            // Terminator-less overflow: drop the whole buffer (the
            // memory-exhaustion defense — it never grows past the cap).
            pending.removeAll()
            overflowDrops += 1
        }
        return frames
    }
}

/// Pure frame parse (spec §2.2 lenient rule): lines prefixed `data:`
/// are joined (stripping one optional leading space) and the result
/// must parse as a JSON object with a string `type`; anything else
/// (non-JSON, missing type, `event:`/`id:`/comment-only frames) is
/// ignored — nil, never fatal.
enum SseFrames {
    struct Frame {
        let type: String
        let payload: [String: Any]
        /// Raw snapshot dictionaries; only populated for `tasks` ticks.
        let taskDictionaries: [[String: Any]]
    }

    static func dataPayload(_ frameText: String) -> String {
        var payload = ""
        for line in frameText.split(separator: "\n", omittingEmptySubsequences: true) {
            if line.hasPrefix("data:") {
                let v = line.dropFirst(5)
                payload += v.hasPrefix(" ") ? String(v.dropFirst()) : String(v)
            }
            // Any other field (`event:`, `id:`, comments) is ignored —
            // the daemon emits `data:` frames only (lenient rule).
        }
        return payload
    }

    static func parse(_ frameText: String) -> Frame? {
        let payload = dataPayload(frameText)
        guard !payload.isEmpty,
              let obj = (try? JSONSerialization.jsonObject(with: Data(payload.utf8))) as? [String: Any],
              let type = obj["type"] as? String else {
            return nil
        }
        var taskDictionaries: [[String: Any]] = []
        if type == "tasks", let arr = obj["tasks"] as? [[String: Any]] {
            taskDictionaries = arr
        }
        return Frame(type: type, payload: obj, taskDictionaries: taskDictionaries)
    }
}

final class SseClient: NSObject, URLSessionDataDelegate {
    struct Event {
        let type: String
        let payload: [String: Any]
        /// Present only on the `tasks` tick: the changed TaskSnapshots.
        let tasks: [TaskSnapshot]?
    }

    /// Called on the session's serial delegate queue for every parsed frame.
    var onEvent: ((Event) -> Void)?
    /// Called on the session's serial delegate queue when the stream
    /// connects (true) or reaches a terminal state (false).
    var onStateChange: ((Bool) -> Void)?

    private let baseURL: URL
    private var session: URLSession?
    private var streamTask: URLSessionDataTask?
    private var frameBuffer = SseFrameBuffer()

    // App-lifetime object: URLSession retains its delegate, so the cycle
    // is broken by stop() (wired into applicationWillTerminate) rather
    // than by deinit.
    init(addr: String = "127.0.0.1:36990") {
        self.baseURL = URL(string: "http://\(addr)/events")!
        super.init()
    }

    /// (Re)open the stream. Safe to call again after a terminal state.
    func start() {
        stop() // one stream at a time
        let cfg = URLSessionConfiguration.default
        // Dedicated cookie-less session (S12-mac2 rule 4/8): never
        // inherits URLSession.shared's persisted cookie jar.
        cfg.httpShouldSetCookies = false
        cfg.httpCookieAcceptPolicy = .never
        cfg.requestCachePolicy = .reloadIgnoringLocalCacheData
        // The server legitimately sends nothing while idle (no heartbeats),
        // so the 60 s default request timeout would kill healthy streams;
        // 24 h bounds a wedged-but-unclosed socket instead of hanging
        // forever. Genuine daemon death is an error/EOF.
        cfg.timeoutIntervalForRequest = 86_400
        cfg.timeoutIntervalForResource = 86_400
        // delegateQueue: nil — URLSession creates a serial queue, so the
        // frame buffer is only ever touched from one thread.
        let sess = URLSession(configuration: cfg, delegate: self, delegateQueue: nil)
        session = sess
        var req = URLRequest(url: baseURL)
        req.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        streamTask = sess.dataTask(with: req)
        streamTask?.resume()
    }

    func stop() {
        streamTask?.cancel()
        streamTask = nil
        session?.invalidateAndCancel()
        session = nil
        frameBuffer = SseFrameBuffer()
    }

    // MARK: - URLSessionDataDelegate

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask,
                    didReceive response: URLResponse,
                    completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
        guard let http = response as? HTTPURLResponse,
              http.statusCode == 200,
              (http.value(forHTTPHeaderField: "Content-Type") ?? "")
                  .lowercased().contains("text/event-stream") else {
            completionHandler(.cancel)
            onStateChange?(false)
            return
        }
        completionHandler(.allow)
        onStateChange?(true)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask,
                    didReceive data: Data) {
        for text in frameBuffer.append(data) {
            if let frame = SseFrames.parse(text) {
                let tasks = frame.taskDictionaries.compactMap { d -> TaskSnapshot? in
                    guard let data = try? JSONSerialization.data(withJSONObject: d) else {
                        return nil
                    }
                    return try? JSONDecoder().decode(TaskSnapshot.self, from: data)
                }
                onEvent?(Event(type: frame.type, payload: frame.payload, tasks: tasks))
            }
            // nil frame: ignored, never fatal (lenient rule).
        }
    }

    // Redirect refusal (S12-mac2 rule 4/8): a redirect from the loopback
    // daemon can only come from a hostile local port-owner.
    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest,
                    completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
        onStateChange?(false)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask,
                    didCompleteWithError error: Error?) {
        // Terminal state, clean or not (daemon death is an error/EOF on
        // loopback). The fallback probe owns recovery from here (spec §5).
        frameBuffer = SseFrameBuffer()
        streamTask = nil
        onStateChange?(false)
    }
}
