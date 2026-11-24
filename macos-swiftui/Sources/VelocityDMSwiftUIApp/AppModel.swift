// VelocityDM SwiftUI frontend — application model.
//
// @MainActor ObservableObject owning the task model, the SSE event
// state machine (the gate-0010 consumer contract restated by S12-mac2
// spec rule 8), the settings round-trips and the theme preferences.
//
// State machine truths pinned here (docs/SPECS/S12-SSE-WIRING.md §3–§5
// as restated by docs/SPECS/S12-MAC2-SWIFTUI.md rule 8):
//   • `tasks` ticks merge in place keyed by id (the four tracked
//     fields only); metadata-only drift additionally marks dirty for a
//     coalesced 150 ms re-list; all other events coalesce to re-lists;
//     ticks omit removals — removal convergence rides `task-removed`
//     → re-list; unknown ids append, never reorder; idle daemon ⇒ zero
//     RPC.
//   • On every (re)connect ONE full re-list runs before any merge is
//     trusted — ticks arriving inside the resync window are dropped.
//   • Terminal stream state: reachable(false); the banner swaps to the
//     fixed stale wording; rows FROZEN (no merges, no re-lists mutate
//     the model while down); a 2 s repeating `downloads.list` probe
//     whose failures keep the banner and never mutate the model;
//     recovery = first probe success → fresh list → stream reopen →
//     one full re-list precedes any merge.
//   • The machine is idempotent both ways INCLUDING launch-with-dead-
//     daemon: the initial list failure opens the fallback even though
//     no false→true transition ever fired.
//   • The SSE-spec §5.3 "(reason updated)" wording is omitted — probe
//     failures update nothing (S12-mac2 rule 8).

import SwiftUI

@MainActor
final class AppModel: ObservableObject {
    // MARK: published UI state

    @Published var tasks: [TaskSnapshot] = []
    @Published var banner: String = "idle"
    @Published var reachable = false
    @Published var addError: String?
    @Published var settingsVisible = false

    // Add-form fields (§5): URL (or magnet:), filename override, quality
    // preset, speed limit, local .torrent path. Success clears them.
    @Published var urlField = ""
    @Published var filenameField = ""
    @Published var qualityField = ""
    @Published var limitField = ""
    @Published var torrentPathField = ""

    // Settings panel fields (populated from the live config on open).
    @Published var dirField = ""
    @Published var connField = ""
    @Published var maxActiveField = ""
    @Published var glimitField = ""
    @Published var proxyField = ""
    @Published var stopStartField = ""
    @Published var stopEndField = ""
    @Published var shutdownWhenDone = false

    // Theme preferences — the SAME UserDefaults keys as the AppKit app.
    @Published var prefsSeed: UInt32 = Md3DefaultSeed {
        didSet { UserDefaults.standard.set(Int(prefsSeed), forKey: "vdm.seed") }
    }
    @Published var prefsDark: Bool = true {
        didSet { UserDefaults.standard.set(prefsDark, forKey: "vdm.dark") }
    }
    var scheme: Md3Scheme { Md3Scheme.fromSeedHex(prefsSeed, dark: prefsDark) }

    // MARK: private machinery

    private let rpc = RpcClient()
    private let sse = SseClient()
    private var awaitingResync = false
    private var dirty = false
    private var coalesceTask: Task<Void, Never>?
    private var probeTask: Task<Void, Never>?
    private var started = false

    init() {
        if let hex = UserDefaults.standard.object(forKey: "vdm.seed") as? Int {
            prefsSeed = UInt32(truncatingIfNeeded: hex)
        }
        if let dark = UserDefaults.standard.object(forKey: "vdm.dark") as? Bool {
            prefsDark = dark
        }
    }

    /// §3.3 initial state: one immediate `downloads.list`, then open the
    /// stream. A dead daemon at launch fails the initial list and opens
    /// the fallback directly (launch-with-dead-daemon idempotency).
    func start() {
        guard !started else { return }
        started = true
        refreshInitial()
        sse.onEvent = { [weak self] event in
            guard let self else { return }
            Task { @MainActor in self.handleEvent(event) }
        }
        sse.onStateChange = { [weak self] up in
            guard let self else { return }
            Task { @MainActor in self.setStream(up: up) }
        }
        sse.start()
    }

    /// Called from the app's termination path (AppDelegate via
    /// @NSApplicationDelegateAdaptor): no orphaned stream task at exit.
    func stopLiveUpdates() {
        probeTask?.cancel()
        probeTask = nil
        coalesceTask?.cancel()
        coalesceTask = nil
        dirty = false
        sse.stop()
    }

    // MARK: stream state machine

    /// Only entry for stream-state flips. Both directions are
    /// idempotent: enterFallback re-runs safely on a second terminal
    /// state, and every (re)connect invalidates the fallback probe and
    /// resyncs with a full re-list before trusting merges.
    private func setStream(up: Bool) {
        if up {
            reachable = true
            probeTask?.cancel()
            probeTask = nil
            // Full re-list precedes any merge after (re)connect (§3.3).
            awaitingResync = true
            refreshNow()
        } else {
            enterFallback()
        }
    }

    /// Terminal stream state (daemon death/EOF): the stale quad — fixed
    /// stale banner, frozen rows, 2 s probe. Idempotent via the
    /// probeTask == nil guard.
    private func enterFallback() {
        reachable = false
        awaitingResync = false
        coalesceTask?.cancel()
        coalesceTask = nil
        dirty = false
        banner = "daemon unreachable — rows stale"
        startProbe()
    }

    /// §5: the probe runs ONLY while the stream is down; failures keep
    /// the stale banner and never mutate the task model; the first
    /// success flips reachable, renders the fresh list and re-opens the
    /// stream (whose connect handler then runs one full re-list before
    /// any merge is trusted).
    private func startProbe() {
        guard probeTask == nil else { return }
        probeTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                guard let self = self, !Task.isCancelled else { return }
                self.runProbe()
            }
        }
    }

    private func runProbe() {
        rpc.list { [weak self] result in
            guard let self else { return }
            if case .success(let fresh) = result {
                self.probeTask?.cancel()
                self.probeTask = nil
                self.reachable = true
                self.applyList(fresh)
                self.sse.start()
            }
            // Failures keep the banner, mutate nothing.
        }
    }

    // MARK: re-list + event routing

    private func refreshInitial() {
        rpc.list { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let fresh):
                self.applyList(fresh)
            case .failure:
                // Launch-with-dead-daemon: the false→false "transition"
                // still opens the fallback (idempotent re-entry later
                // when the stream's terminal state arrives).
                self.enterFallback()
            }
        }
    }

    /// Full re-list while the stream is considered connected. A failure
    /// hands refresh ownership to the fallback path (idempotent; the
    /// probe converges the machine back if the stream is in fact alive).
    private func refreshNow() {
        rpc.list { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let fresh):
                self.applyList(fresh)
            case .failure:
                self.enterFallback()
            }
        }
    }

    /// Replaces the model in list order (never reordered by ticks) and
    /// closes the resync window.
    private func applyList(_ fresh: [TaskSnapshot]) {
        tasks = fresh
        awaitingResync = false
        updateBanner()
    }

    private func updateBanner() {
        let total = tasks.map { $0.speed ?? 0 }.reduce(0, +)
        banner = total > 0 ? "\(byteString(total))/s total" : "idle"
    }

    /// §3.4: events arriving after a terminal state are dropped — the
    /// fallback path owns refreshes from there.
    private func handleEvent(_ event: SseClient.Event) {
        guard reachable else { return }
        if event.type == "tasks", let changed = event.tasks {
            // Ticks inside the resync window are dropped: the pending
            // full re-list must land before any merge is trusted (§3.3).
            guard !awaitingResync else { return }
            mergeSnapshots(changed)
        } else {
            markDirty() // coalesced full re-list (§3.2)
        }
    }

    /// §3.2: a single pending re-list is scheduled 150 ms after the
    /// first dirty mark; further marks within the window are absorbed.
    private func markDirty() {
        guard coalesceTask == nil else { return }
        dirty = true
        coalesceTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 150_000_000)
            guard let self = self, !Task.isCancelled else { return }
            self.coalesceTask = nil
            if self.dirty, self.reachable {
                self.dirty = false
                self.refreshNow()
            }
        }
    }

    /// §3.1: tick merge in place keyed by id; unknown ids append, never
    /// reorder. The tick carries only snapshots whose (state, done,
    /// speed, error) changed — a merge that moved none of those four
    /// (metadata-only drift, e.g. resolved name/outPath/notice) marks
    /// dirty for the coalesced re-list so such changes still converge.
    private func mergeSnapshots(_ changed: [TaskSnapshot]) {
        var metadataDrift = false
        for c in changed {
            if let i = tasks.firstIndex(where: { $0.id == c.id }) {
                let old = tasks[i]
                tasks[i] = c
                if old.state == c.state && old.done == c.done
                    && old.speed == c.speed && old.error == c.error {
                    let metadataMoved = old.name != c.name
                        || old.outPath != c.outPath || old.dir != c.dir
                        || old.size != c.size || old.kind != c.kind
                        || old.notice != c.notice
                    if metadataMoved { metadataDrift = true }
                }
            } else {
                tasks.append(c) // unknown id: append, never reorder ticks
            }
        }
        updateBanner()
        if metadataDrift { markDirty() }
    }

    // MARK: add path (§5)

    /// URL entry: a plain URL routes to `downloads.add` (with the
    /// filename override inside `options`); a `magnet:` URL routes to
    /// `downloads.addTorrent` (top-level `magnet` param). Enter-to-add
    /// rides `.onSubmit` on the URL field. Errors surface verbatim in
    /// the dedicated error line; success clears the fields. No
    /// auto-retry, no silent fallback.
    func addDownload() {
        let trimmed = urlField.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        let limitBps = String.parseBytes(limitField) ?? 0
        let quality = qualityField.trimmingCharacters(in: .whitespaces)
        let filename = filenameField.trimmingCharacters(in: .whitespaces)
        let finish: @MainActor (Result<UInt64, RpcError>) -> Void = { [weak self] result in
            guard let self else { return }
            switch result {
            case .failure(let e):
                self.addError = "\(e)" // verbatim, no swallowing
            case .success:
                self.addError = nil
            }
        }
        if trimmed.hasPrefix("magnet:") {
            rpc.addTorrent(magnet: trimmed, limitBps: limitBps) { [weak self] result in
                finish(result)
                if case .success = result {
                    self?.clearAddFields()
                }
            }
            return
        }
        rpc.add(url: trimmed, quality: quality, limitBps: limitBps, filename: filename.isEmpty ? nil : filename) { [weak self] result in
            finish(result)
            if case .success = result {
                self?.clearAddFields()
            }
        }
    }

    /// Local `.torrent` path via the TOP-LEVEL `torrentPath` param —
    /// populated ONLY from an explicit user action (the typed path
    /// field); never parsed from daemon or network data.
    func addTorrentFile() {
        let trimmed = torrentPathField.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        let limitBps = String.parseBytes(limitField) ?? 0
        rpc.addTorrent(path: trimmed, limitBps: limitBps) { [weak self] result in
            guard let self else { return }
            switch result {
            case .failure(let e):
                self.addError = "\(e)" // verbatim, no swallowing
            case .success:
                self.addError = nil
                self.clearAddFields()
            }
        }
    }

    /// Success clears the fields (spec §5); failure keeps them for the
    /// verbatim error line.
    private func clearAddFields() {
        addError = nil
        urlField = ""
        filenameField = ""
        qualityField = ""
        limitField = ""
        torrentPathField = ""
    }

    // MARK: task actions

    func pause(id: UInt64) { rpc.pause(id: id) }
    func resume(id: UInt64) { rpc.resume(id: id) }
    func remove(id: UInt64) { rpc.remove(id: id) }

    // MARK: settings (§7)

    func toggleSettings() {
        settingsVisible.toggle()
        guard settingsVisible else { return }
        rpc.getSettings { [weak self] result in
            if case .success(let s) = result {
                self?.dirField = s.downloadDir ?? ""
                if let c = s.defaultConnections { self?.connField = String(c) }
                self?.proxyField = s.proxy?.display ?? ""
            }
        }
        rpc.getLimits { [weak self] result in
            if case .success(let l) = result, let gbps = l.globalBps, gbps > 0 {
                self?.glimitField = String(gbps)
            }
        }
        rpc.getQueue { [weak self] result in
            if case .success(let q) = result {
                if let m = q.maxActive { self?.maxActiveField = String(m) }
                if let w = q.stopWindow {
                    self?.stopStartField = w.first ?? ""
                    self?.stopEndField = w.count > 1 ? w[1] : ""
                }
                if let drained = q.onQueueDrained {
                    self?.shutdownWhenDone = drained == "shutdown"
                }
            }
        }
    }

    /// Apply starts from the live config so unexposed fields survive:
    /// the panel pulls the live values on open (toggleSettings) and the
    /// daemon's settings.set/queue.set MERGE provided keys into the
    /// stored config (crates/vdm-rpc/src/lib.rs:630 — `let mut cfg =
    /// engine.config()`), so only the fields this panel exposes are
    /// sent and e.g. `userAgent` is preserved. Clamps are applied on
    /// save: connections 1...32, max-active 1...16 (the daemon clamps
    /// 32 but not the floor; the client owns the honest range).
    func applySettings() {
        var settings: [String: Any] = [:]
        let dir = dirField.trimmingCharacters(in: .whitespaces)
        if !dir.isEmpty { settings["downloadDir"] = dir }
        if let c = Int(connField.trimmingCharacters(in: .whitespaces)) {
            settings["defaultConnections"] = min(max(c, 1), 32)
        }
        let proxy = proxyField.trimmingCharacters(in: .whitespaces)
        if proxy.isEmpty || proxy.lowercased() == "none" {
            settings["proxy"] = NSNull() // explicit clear (daemon: null → None)
        } else if let p = RpcProxy.parse(proxy) {
            // tagged enum shape: {"type":"http","host":...,"port":...}
            settings["proxy"] = ["type": p.type, "host": p.host, "port": p.port]
        }
        rpc.setSettings(settings)

        var queue: [String: Any] = [:]
        if let m = Int(maxActiveField.trimmingCharacters(in: .whitespaces)) {
            queue["maxActive"] = min(max(m, 1), 16)
        }
        let start = stopStartField.trimmingCharacters(in: .whitespaces)
        let end = stopEndField.trimmingCharacters(in: .whitespaces)
        if isHHMM(start), isHHMM(end) {
            queue["stopWindow"] = [start, end]
        }
        queue["onQueueDrained"] = shutdownWhenDone ? "shutdown" : "none"
        rpc.setQueue(queue)

        if let gbps = String.parseBytes(glimitField) {
            rpc.setLimits(globalBps: gbps)
        }
    }

    private func isHHMM(_ s: String) -> Bool {
        let parts = s.split(separator: ":")
        guard parts.count == 2,
              let h = Int(parts[0]), let m = Int(parts[1]) else { return false }
        return (0..<24).contains(h) && (0..<60).contains(m)
    }

    // MARK: theme

    func cycleSeed() {
        let idx = Md3Seeds.firstIndex(where: { $0.0 == prefsSeed }) ?? 0
        prefsSeed = Md3Seeds[(idx + 1) % Md3Seeds.count].0
    }

    func toggleDark() {
        prefsDark.toggle()
    }

    // MARK: helpers

    func byteString(_ bytes: UInt64) -> String {
        let units: [(Double, String)] = [
            (1024 * 1024 * 1024, "GiB"), (1024 * 1024, "MiB"), (1024, "KiB"),
        ]
        let d = Double(bytes)
        for (mult, suffix) in units where d >= mult {
            return String(format: "%.1f %@", d / mult, suffix)
        }
        return "\(bytes) B"
    }
}
