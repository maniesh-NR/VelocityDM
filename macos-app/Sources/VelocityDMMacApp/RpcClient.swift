// VelocityDM macOS frontend — JSON-RPC client for the shared Rust core.
//
// Speaks the documented daemon protocol (docs/RPC.md): JSON-RPC 2.0
// over POST /rpc on 127.0.0.1:36990. URLSession sends no Origin/Referer
// and the Host header names the loopback authority, so the loopback
// trust boundary accepts this native client. The periodic `tasks` SSE
// tick is optional for this client; it polls `downloads.list` at the
// same cadence the other frontends refresh (200 ms).

import Foundation

struct TaskSnapshot: Codable {
    let id: UInt64
    let kind: String?
    let name: String?
    let url: String?
    let dir: String?
    let outPath: String?
    let state: String?
    let size: UInt64?
    let done: UInt64?
    let speed: UInt64?
    let etaSecs: UInt64?
    let connections: UInt32?
    let peers: UInt32?
    let seeds: UInt32?
    let priority: UInt32?
    let startAt: Double?
    let error: String?
    let checksumOk: Bool?
    let userPaused: Bool?
    let createdAt: Double?
}

struct RpcProxy: Codable {
    let type: String
    let host: String
    let port: Int
    var user: String?
    var pass: String?

    var display: String {
        "\(type)://\(host):\(port)"
    }

    /// Parse "http://host:port" or "socks5://host:port" into the
    /// tagged enum shape the daemon expects
    /// (`{"type":"http","host":...,"port":...}`).
    static func parse(_ s: String) -> RpcProxy? {
        let t = s.trimmingCharacters(in: .whitespaces)
        for scheme in ["http://", "socks5://"] {
            if let r = t.lowercased().range(of: scheme) {
                let rest = String(t[r.upperBound...])
                let parts = rest.split(separator: ":", maxSplits: 1)
                guard parts.count == 2, let port = Int(parts[1]), (1...65535).contains(port) else { continue }
                return RpcProxy(
                    type: String(scheme.dropLast(3)),
                    host: String(parts[0]),
                    port: port
                )
            }
        }
        return nil
    }
}

struct EngineSettings: Codable {
    var downloadDir: String?
    var defaultConnections: Int?
    var proxy: RpcProxy?
    var userAgent: String?
}

struct LimitInfo: Codable {
    var globalBps: UInt64?
}

struct QueueRules: Codable {
    var maxActive: Int?
    var stopWindow: [String]?
    var onQueueDrained: String?
}

enum RpcError: Error, CustomStringConvertible {
    case transport(String)
    case http(Int)
    case rpc(code: Int, message: String)
    case malformed(String)

    var description: String {
        switch self {
        case .transport(let m): return "transport: \(m)"
        case .http(let s): return "http \(s)"
        case .rpc(let c, let m): return "rpc \(c): \(m)"
        case .malformed(let m): return "malformed: \(m)"
        }
    }
}

final class RpcClient {
    let baseURL: URL

    init(addr: String = "127.0.0.1:36990") {
        self.baseURL = URL(string: "http://\(addr)")!
    }

    private var nextId: UInt64 = 0

    /// One JSON-RPC 2.0 call; `params` is a dictionary or NSNull.
    private func call(_ method: String, params: Any?, completion: @escaping (Result<Any?, RpcError>) -> Void) {
        nextId += 1
        let id = nextId
        var payload: [String: Any] = [
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        ]
        if let p = params {
            payload["params"] = p
        }
        guard let body = try? JSONSerialization.data(withJSONObject: payload) else {
            completion(.failure(.malformed("cannot encode request for \(method)")))
            return
        }
        var req = URLRequest(url: baseURL.appendingPathComponent("rpc"))
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = body
        req.timeoutInterval = 10.0
        let task = URLSession.shared.dataTask(with: req) { data, response, error in
            if let error = error {
                DispatchQueue.main.async {
                    completion(.failure(.transport(error.localizedDescription)))
                }
                return
            }
            if let http = response as? HTTPURLResponse, http.statusCode != 200 {
                DispatchQueue.main.async {
                    completion(.failure(.http(http.statusCode)))
                }
                return
            }
            guard let data = data,
                  let obj = try? JSONSerialization.jsonObject(with: data),
                  let envelope = obj as? [String: Any] else {
                DispatchQueue.main.async {
                    completion(.failure(.malformed("bad response envelope")))
                }
                return
            }
            DispatchQueue.main.async {
                if let err = envelope["error"] as? [String: Any],
                   let code = err["code"] as? Int,
                   let message = err["message"] as? String {
                    completion(.failure(.rpc(code: code, message: message)))
                } else {
                    completion(.success(envelope["result"]))
                }
            }
        }
        task.resume()
    }

    private func decode<T: Decodable>(_ type: T.Type, from result: Any?) -> Result<T, RpcError> {
        guard let result = result else {
            return .failure(.malformed("missing result"))
        }
        do {
            let data = try JSONSerialization.data(withJSONObject: result)
            let decoded = try JSONDecoder().decode(type, from: data)
            return .success(decoded)
        } catch {
            return .failure(.malformed("decode \(type): \(error)"))
        }
    }

    // MARK: typed surface

    func ping(_ completion: @escaping (Result<String, RpcError>) -> Void) {
        call("daemon.ping", params: nil) { [weak self] res in
            guard let self = self else { return }
            switch res {
            case .failure(let e): completion(.failure(e))
            case .success(let r): completion(self.decode(String.self, from: r))
            }
        }
    }

    func list(_ completion: @escaping (Result<[TaskSnapshot], RpcError>) -> Void) {
        call("downloads.list", params: nil) { [weak self] res in
            guard let self = self else { return }
            switch res {
            case .failure(let e): completion(.failure(e))
            case .success(let r): completion(self.decode([TaskSnapshot].self, from: r))
            }
        }
    }

    func add(url: String, quality: String?, limitBps: UInt64?, completion: @escaping (Result<UInt64, RpcError>) -> Void) {
        var options: [String: Any] = [:]
        if let q = quality, !q.isEmpty { options["quality"] = q }
        if let l = limitBps, l > 0 { options["rateLimitBps"] = l }
        call("downloads.add", params: ["url": url, "options": options]) { [weak self] res in
            guard let self = self else { return }
            switch res {
            case .failure(let e): completion(.failure(e))
            case .success(let r):
                // {id}
                if let dict = r as? [String: Any], let id = dict["id"] as? UInt64 {
                    completion(.success(id))
                } else {
                    completion(self.decode(UInt64.self, from: r))
                }
            }
        }
    }

    func addTorrent(magnet: String, completion: @escaping (Result<UInt64, RpcError>) -> Void) {
        call("downloads.addTorrent", params: ["magnet": magnet, "options": [:]]) { res in
            switch res {
            case .failure(let e): completion(.failure(e))
            case .success(let r):
                if let dict = r as? [String: Any], let id = dict["id"] as? UInt64 {
                    completion(.success(id))
                } else {
                    completion(.failure(.malformed("no id in addTorrent result")))
                }
            }
        }
    }

    func pause(id: UInt64, completion: ((Result<Bool, RpcError>) -> Void)? = nil) {
        call("downloads.pause", params: ["id": id]) { res in
            completion?(self.boolResult(res))
        }
    }

    func resume(id: UInt64, completion: ((Result<Bool, RpcError>) -> Void)? = nil) {
        call("downloads.resume", params: ["id": id]) { res in
            completion?(self.boolResult(res))
        }
    }

    func remove(id: UInt64, completion: ((Result<Bool, RpcError>) -> Void)? = nil) {
        call("downloads.remove", params: ["id": id]) { res in
            completion?(self.boolResult(res))
        }
    }

    func getSettings(_ completion: @escaping (Result<EngineSettings, RpcError>) -> Void) {
        call("settings.get", params: nil) { [weak self] res in
            guard let self = self else { return }
            switch res {
            case .failure(let e): completion(.failure(e))
            case .success(let r): completion(self.decode(EngineSettings.self, from: r))
            }
        }
    }

    func getLimits(_ completion: @escaping (Result<LimitInfo, RpcError>) -> Void) {
        call("limits.get", params: nil) { [weak self] res in
            guard let self = self else { return }
            switch res {
            case .failure(let e): completion(.failure(e))
            case .success(let r): completion(self.decode(LimitInfo.self, from: r))
            }
        }
    }

    func setLimits(globalBps: UInt64, completion: ((Result<Bool, RpcError>) -> Void)? = nil) {
        call("limits.set", params: ["globalBps": globalBps]) { res in
            completion?(self.boolResult(res))
        }
    }

    func setSettings(_ settings: [String: Any], completion: ((Result<Bool, RpcError>) -> Void)? = nil) {
        call("settings.set", params: settings) { res in
            completion?(self.boolResult(res))
        }
    }

    func getQueue(_ completion: @escaping (Result<QueueRules, RpcError>) -> Void) {
        call("queue.get", params: nil) { [weak self] res in
            guard let self = self else { return }
            switch res {
            case .failure(let e): completion(.failure(e))
            case .success(let r): completion(self.decode(QueueRules.self, from: r))
            }
        }
    }

    func setQueue(_ queue: [String: Any], completion: ((Result<Bool, RpcError>) -> Void)? = nil) {
        call("queue.set", params: queue) { res in
            completion?(self.boolResult(res))
        }
    }

    private func boolResult(_ res: Result<Any?, RpcError>) -> Result<Bool, RpcError> {
        switch res {
        case .failure(let e): return .failure(e)
        case .success(let r):
            if let b = r as? Bool { return .success(b) }
            return .success(true)
        }
    }
}

extension String {
    /// Parse "1.5 MiB"-style byte strings the way the engine formats
    /// them is not needed here — this helper parses plain numbers and
    /// simple suffixes for the limit field.
    static func parseBytes(_ s: String) -> UInt64? {
        let trimmed = s.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return 0 }
        let map: [(String, UInt64)] = [
            ("kib", 1024), ("mib", 1024 * 1024), ("gib", 1024 * 1024 * 1024),
            ("kb", 1000), ("mb", 1000 * 1000), ("gb", 1000 * 1000 * 1000),
            ("k", 1024), ("m", 1024 * 1024), ("g", 1024 * 1024 * 1024),
        ]
        let lower = trimmed.lowercased()
        for (suffix, mult) in map where lower.hasSuffix(suffix) {
            let head = String(lower.dropLast(suffix.count)).trimmingCharacters(in: .whitespaces)
            if let v = Double(head), v.isFinite, v >= 0 {
                return UInt64(v * Double(mult))
            }
        }
        if let v = Double(lower), v.isFinite, v >= 0 {
            return UInt64(v)
        }
        return nil
    }
}
