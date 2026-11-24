// VelocityDM macOS frontend — SSE client for the daemon's GET /events.
//
// Consumes the documented stream (docs/RPC.md §GET /events): a
// text/event-stream of `data: <single-line JSON>` frames plus the 10 Hz
// coalesced `tasks` snapshot tick. Behavior is specified in
// docs/SPECS/S12-SSE-WIRING.md: frames may straddle network reads (the
// buffer splits on the `\n\n` terminator); the server sends no
// heartbeats, so a terminal transport error/EOF is the daemon-death
// signal the UI turns into its stale-rows state. The request is a plain
// native-client GET — no Origin/Referer header is ever set (loopback
// trust boundary, docs/RPC.md §Loopback trust boundary).

import Foundation

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
    private var buffer = Data()

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
        // The server legitimately sends nothing while idle (no heartbeats),
        // so the 60 s default request timeout would kill healthy streams;
        // 24 h bounds a wedged-but-unclosed socket instead of hanging
        // forever (spec §1.2). Genuine daemon death is an error/EOF.
        cfg.timeoutIntervalForRequest = 86_400
        cfg.timeoutIntervalForResource = 86_400
        cfg.requestCachePolicy = .reloadIgnoringLocalCacheData
        // delegateQueue: nil — URLSession creates a serial queue, so the
        // frame buffer is only ever touched from one thread (spec §6.1).
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
        buffer.removeAll()
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
        buffer.append(data)
        let terminator = Data("\n\n".utf8)
        while let r = buffer.range(of: terminator, options: [],
                                   in: buffer.startIndex..<buffer.endIndex) {
            let frame = buffer.subdata(in: buffer.startIndex..<r.lowerBound)
            buffer.removeSubrange(buffer.startIndex..<r.upperBound)
            if let text = String(data: frame, encoding: .utf8) {
                parseFrame(text)
            }
        }
        // Defensive cap (spec §2.3): a terminator-less stream cannot grow
        // memory unboundedly. The daemon never sends such frames.
        if buffer.count > 1 << 20 {
            buffer.removeAll()
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask,
                    didCompleteWithError error: Error?) {
        // Terminal state, clean or not (daemon death is an error/EOF on
        // loopback). The fallback probe owns recovery from here (spec §5).
        buffer.removeAll()
        streamTask = nil
        onStateChange?(false)
    }

    // MARK: - Frame parsing

    private func parseFrame(_ text: String) {
        var payload = ""
        for line in text.split(separator: "\n", omittingEmptySubsequences: true) {
            if line.hasPrefix("data:") {
                let v = line.dropFirst(5)
                payload += v.hasPrefix(" ") ? String(v.dropFirst()) : String(v)
            }
            // Any other field (`event:`, `id:`, comments) is ignored —
            // the daemon emits `data:` frames only (spec §2.2 lenient rule).
        }
        guard !payload.isEmpty,
              let obj = (try? JSONSerialization.jsonObject(with: Data(payload.utf8))) as? [String: Any],
              let type = obj["type"] as? String else {
            return
        }
        var tasks: [TaskSnapshot]?
        if type == "tasks", let arr = obj["tasks"] as? [[String: Any]] {
            tasks = arr.compactMap { d in
                guard let data = try? JSONSerialization.data(withJSONObject: d) else {
                    return nil
                }
                return try? JSONDecoder().decode(TaskSnapshot.self, from: data)
            }
        }
        onEvent?(Event(type: type, payload: obj, tasks: tasks))
    }
}
