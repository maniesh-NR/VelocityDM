// VelocityDM macOS frontend — AppKit UI (programmatic, layer-backed for
// the Material You chrome; NSTableView/NSTextField/NSButton/
// NSProgressIndicator are the platform-standard interaction layer).
//
// The frontend drives the shared Rust core (daemon binary) over the
// loopback JSON-RPC protocol; Material You colors come from the Md3
// Swift port of the shared `vdm-md3` token engine.

import AppKit

final class AppController: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    let window: NSWindow
    private let rpc = RpcClient()
    private var prefsSeed: UInt32 = Md3DefaultSeed
    private var prefsDark: Bool = true
    private var scheme: Md3Scheme = Md3Scheme.fromSeedHex(Md3DefaultSeed, dark: true)
    private var tasks: [TaskSnapshot] = []
    private var refreshTimer: Timer?

    // UI elements
    private let rootStack = NSStackView()
    private let speedLabel = NSTextField(labelWithString: "idle")
    private let urlField = NSTextField()
    private let qualityField = NSTextField()
    private let limitField = NSTextField()
    private let seedButton = NSButton()
    private let darkButton = NSButton()
    private let settingsButton = NSButton()
    private let settingsStack = NSStackView()
    private let dirField = NSTextField()
    private let connField = NSTextField()
    private let maxActiveField = NSTextField()
    private let glimitField = NSTextField()
    private let proxyField = NSTextField()
    private let stopStartField = NSTextField()
    private let stopEndField = NSTextField()
    private let shutdownCheck = NSButton()
    private let tableView = NSTableView()
    private let emptyLabel = NSTextField(labelWithString: "No downloads yet.\nPaste a link above or use the browser extension.")

    override init() {
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 980, height: 640),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        super.init()
        loadPrefs()
        scheme = Md3Scheme.fromSeedHex(prefsSeed, dark: prefsDark)
        buildUi()
        applyTheme()
        window.center()
        refresh()
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in
            self?.refresh()
        }
    }

    // MARK: - UI construction

    private func buildUi() {
        window.title = "VelocityDM"
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 980, height: 640))
        window.contentView = content
        content.wantsLayer = true

        rootStack.orientation = .vertical
        rootStack.alignment = .leading
        rootStack.spacing = 8
        rootStack.translatesAutoresizingMaskIntoConstraints = false
        rootStack.edgeInsets = NSEdgeInsets(top: 12, left: 16, bottom: 16, right: 16)
        content.addSubview(rootStack)
        NSLayoutConstraint.activate([
            rootStack.topAnchor.constraint(equalTo: content.topAnchor, constant: 8),
            rootStack.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            rootStack.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            rootStack.bottomAnchor.constraint(equalTo: content.bottomAnchor),
        ])

        // Header
        let header = NSStackView()
        header.orientation = .horizontal
        header.spacing = 10
        let title = NSTextField(labelWithString: "VelocityDM")
        title.font = NSFont.boldSystemFont(ofSize: 20)
        title.translatesAutoresizingMaskIntoConstraints = false
        header.addArrangedSubview(title)
        configureButton(settingsButton, title: "Settings") { [weak self] in
            self?.toggleSettings()
        }
        header.addArrangedSubview(settingsButton)
        header.addArrangedSubview(NSView()) // spacer
        speedLabel.font = NSFont.systemFont(ofSize: 12)
        speedLabel.translatesAutoresizingMaskIntoConstraints = false
        header.addArrangedSubview(speedLabel)
        configureButton(seedButton, title: md3SeedName(prefsSeed)) { [weak self] in
            self?.cycleSeed()
        }
        header.addArrangedSubview(seedButton)
        configureButton(darkButton, title: prefsDark ? "Dark" : "Light") { [weak self] in
            self?.toggleDark()
        }
        header.addArrangedSubview(darkButton)
        rootStack.addArrangedSubview(header)
        header.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: rootStack.leadingAnchor),
            header.trailingAnchor.constraint(equalTo: rootStack.trailingAnchor),
        ])

        // Settings (hidden by default)
        settingsStack.orientation = .vertical
        settingsStack.spacing = 8
        settingsStack.translatesAutoresizingMaskIntoConstraints = false
        settingsStack.isHidden = true
        let row1 = NSStackView()
        row1.orientation = .horizontal
        row1.spacing = 8
        row1.addArrangedSubview(labeledField("Folder", dirField, width: 260))
        row1.addArrangedSubview(labeledField("Conn", connField, width: 60))
        row1.addArrangedSubview(labeledField("Max", maxActiveField, width: 60))
        row1.addArrangedSubview(labeledField("Limit", glimitField, width: 90))
        row1.addArrangedSubview(labeledField("Proxy", proxyField, width: 220))
        settingsStack.addArrangedSubview(row1)
        let row2 = NSStackView()
        row2.orientation = .horizontal
        row2.spacing = 8
        row2.addArrangedSubview(labeledField("Stop", stopStartField, width: 70))
        row2.addArrangedSubview(labeledField("to", stopEndField, width: 70))
        shutdownCheck.setButtonType(.switch)
        shutdownCheck.title = "Shut down when queue drains"
        shutdownCheck.translatesAutoresizingMaskIntoConstraints = false
        row2.addArrangedSubview(shutdownCheck)
        let apply = NSButton()
        configureButton(apply, title: "Apply") { [weak self] in
            self?.applySettings()
        }
        row2.addArrangedSubview(apply)
        rootStack.addArrangedSubview(settingsStack)
        settingsStack.leadingAnchor.constraint(equalTo: rootStack.leadingAnchor).isActive = true
        settingsStack.trailingAnchor.constraint(equalTo: rootStack.trailingAnchor).isActive = true

        // Add bar
        let addBar = NSStackView()
        addBar.orientation = .horizontal
        addBar.spacing = 8
        addBar.translatesAutoresizingMaskIntoConstraints = false
        setupInput(urlField, placeholder: "Paste a URL, magnet: or path to .torrent")
        setupInput(qualityField, placeholder: "best")
        setupInput(limitField, placeholder: "off")
        urlField.translatesAutoresizingMaskIntoConstraints = false
        qualityField.translatesAutoresizingMaskIntoConstraints = false
        limitField.translatesAutoresizingMaskIntoConstraints = false
        addBar.addArrangedSubview(urlField)
        addBar.addArrangedSubview(qualityField)
        addBar.addArrangedSubview(limitField)
        let addButton = NSButton()
        configureButton(addButton, title: "Add") { [weak self] in
            self?.addDownload()
        }
        addBar.addArrangedSubview(addButton)
        rootStack.addArrangedSubview(addBar)
        NSLayoutConstraint.activate([
            addBar.leadingAnchor.constraint(equalTo: rootStack.leadingAnchor),
            addBar.trailingAnchor.constraint(equalTo: rootStack.trailingAnchor),
            urlField.widthAnchor.constraint(greaterThanOrEqualToConstant: 560),
        ])

        // Table
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("task"))
        column.width = 940
        tableView.addTableColumn(column)
        tableView.headerView = nil
        tableView.dataSource = self
        tableView.delegate = self
        tableView.selectionHighlightStyle = .none
        tableView.backgroundColor = .clear
        let scroll = NSScrollView()
        scroll.documentView = tableView
        scroll.hasVerticalScroller = true
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.wantsLayer = true
        rootStack.addArrangedSubview(scroll)
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: rootStack.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: rootStack.trailingAnchor),
            scroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 300),
        ])

        emptyLabel.alignment = .center
        emptyLabel.isHidden = false
        emptyLabel.translatesAutoresizingMaskIntoConstraints = false
        emptyLabel.lineBreakMode = .byWordWrapping
        emptyLabel.maximumNumberOfLines = 2
        content.addSubview(emptyLabel)
        NSLayoutConstraint.activate([
            emptyLabel.centerXAnchor.constraint(equalTo: content.centerXAnchor),
            emptyLabel.centerYAnchor.constraint(equalTo: content.centerYAnchor, constant: 60),
        ])

        // Enter in the URL field adds the download (the classic IDM flow)
        urlField.action = #selector(addDownload)
        urlField.target = self
    }

    private func setupInput(_ field: NSTextField, placeholder: String) {
        field.placeholderString = placeholder
        field.bezelStyle = .roundedBezel
        field.translatesAutoresizingMaskIntoConstraints = false
    }

    private func labeledField(_ label: String, _ field: NSTextField, width: CGFloat) -> NSView {
        setupInput(field, placeholder: "")
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = 4
        let l = NSTextField(labelWithString: label)
        l.font = NSFont.systemFont(ofSize: 11)
        stack.addArrangedSubview(l)
        stack.addArrangedSubview(field)
        field.widthAnchor.constraint(equalToConstant: width).isActive = true
        return stack
    }

    private func configureButton(_ button: NSButton, title: String, action: @escaping () -> Void) {
        button.title = title
        button.bezelStyle = .rounded
        button.wantsLayer = true
        button.layer?.cornerRadius = 14
        button.translatesAutoresizingMaskIntoConstraints = false
        let handler = ButtonHandler { action() }
        button.target = handler
        button.action = #selector(ButtonHandler.fire)
        buttonAssociatedHandlers.append(handler)
    }

    private var buttonAssociatedHandlers: [ButtonHandler] = []

    // MARK: - Data + refresh

    @objc private func refresh() {
        rpc.list { [weak self] result in
            guard let self = self else { return }
            switch result {
            case .failure(let e):
                self.speedLabel.stringValue = "daemon unreachable (\(e))"
            case .success(let tasks):
                self.tasks = tasks
                let total = tasks.compactMap { $0.speed ?? 0 }.reduce(0, +)
                self.speedLabel.stringValue = total > 0 ? "\(byteString(total))/s total" : "idle"
                self.tableView.reloadData()
            }
            self.emptyLabel.isHidden = !self.tasks.isEmpty
            let reachable = { () -> Bool in
                if case .success = result { return true }
                return false
            }()
            if reachable, self.settingsStack.isHidden {
                // lazily pull settings once for the hidden panel
                self.rpc.getSettings { _ in }
            }
        }
    }

    // MARK: - Actions

    @objc private func addDownload() {
        let url = urlField.stringValue.trimmingCharacters(in: .whitespaces)
        guard !url.isEmpty else { return }
        let limit = String.parseBytes(limitField.stringValue) ?? 0
        if url.hasPrefix("magnet:") {
            rpc.addTorrent(magnet: url) { [weak self] result in
                if case .failure(let e) = result {
                    self?.urlField.stringValue = "! \(e)"
                } else {
                    self?.urlField.stringValue = ""
                }
            }
            return
        }
        rpc.add(url: url, quality: qualityField.stringValue, limitBps: limit) { [weak self] result in
            switch result {
            case .failure(let e):
                self?.urlField.stringValue = "! \(e)"
            case .success:
                self?.urlField.stringValue = ""
            }
        }
    }

    private func toggleSettings() {
        settingsStack.isHidden.toggle()
        if !settingsStack.isHidden {
            rpc.getSettings { [weak self] result in
                if case .success(let s) = result {
                    self?.dirField.stringValue = s.downloadDir ?? ""
                    if let c = s.defaultConnections { self?.connField.stringValue = String(c) }
                    self?.proxyField.stringValue = s.proxy?.display ?? ""
                }
            }
            rpc.getLimits { [weak self] result in
                if case .success(let l) = result, let gbps = l.globalBps, gbps > 0 {
                    self?.glimitField.stringValue = String(gbps)
                }
            }
            rpc.getQueue { [weak self] result in
                if case .success(let q) = result {
                    if let m = q.maxActive { self?.maxActiveField.stringValue = String(m) }
                    if let w = q.stopWindow {
                        self?.stopStartField.stringValue = w.first ?? ""
                        self?.stopEndField.stringValue = w.count > 1 ? w[1] : ""
                    }
                    if let drained = q.onQueueDrained {
                        self?.shutdownCheck.state = drained == "shutdown" ? .on : .off
                    }
                }
            }
        }
    }

    private func applySettings() {
        var settings: [String: Any] = [:]
        let dir = dirField.stringValue.trimmingCharacters(in: .whitespaces)
        if !dir.isEmpty { settings["downloadDir"] = dir }
        if let c = Int(connField.stringValue) { settings["defaultConnections"] = c }
        let proxy = proxyField.stringValue.trimmingCharacters(in: .whitespaces)
        if proxy.isEmpty || proxy.lowercased() == "none" {
            settings["proxy"] = NSNull()
        } else if let p = RpcProxy.parse(proxy) {
            // tagged enum shape: {"type":"http","host":...,"port":...}
            settings["proxy"] = [
                "type": p.type, "host": p.host, "port": p.port,
            ]
        }
        rpc.setSettings(settings)
        var queue: [String: Any] = [:]
        if let m = Int(maxActiveField.stringValue) { queue["maxActive"] = m }
        let start = stopStartField.stringValue.trimmingCharacters(in: .whitespaces)
        let end = stopEndField.stringValue.trimmingCharacters(in: .whitespaces)
        if isHHMM(start), isHHMM(end) {
            queue["stopWindow"] = [start, end]
        }
        queue["onQueueDrained"] = shutdownCheck.state == .on ? "shutdown" : "none"
        rpc.setQueue(queue)
        if let gbps = String.parseBytes(glimitField.stringValue) {
            rpc.setLimits(globalBps: gbps)
        }
    }

    private func cycleSeed() {
        let idx = Md3Seeds.firstIndex(where: { $0.0 == prefsSeed }) ?? 0
        prefsSeed = Md3Seeds[(idx + 1) % Md3Seeds.count].0
        savePrefs()
        retheme()
    }

    private func toggleDark() {
        prefsDark.toggle()
        savePrefs()
        retheme()
    }

    private func retheme() {
        scheme = Md3Scheme.fromSeedHex(prefsSeed, dark: prefsDark)
        seedButton.title = md3SeedName(prefsSeed)
        darkButton.title = prefsDark ? "Dark" : "Light"
        applyTheme()
    }

    // MARK: - Preferences

    private func loadPrefs() {
        let defaults = UserDefaults.standard
        if let hex = defaults.object(forKey: "vdm.seed") as? Int {
            prefsSeed = UInt32(truncatingIfNeeded: hex)
        }
        if let dark = defaults.object(forKey: "vdm.dark") as? Bool {
            prefsDark = dark
        }
    }

    private func savePrefs() {
        let defaults = UserDefaults.standard
        defaults.set(Int(prefsSeed), forKey: "vdm.seed")
        defaults.set(prefsDark, forKey: "vdm.dark")
    }

    // MARK: - Theming

    private func applyTheme() {
        let s = scheme
        window.backgroundColor = .md3(s.surfaceContainerLow)
        window.contentView?.layer?.backgroundColor = CGColor.md3(s.surface)
        speedLabel.textColor = .md3(s.onSurfaceVariant)
        emptyLabel.textColor = .md3(s.onSurfaceVariant)
        for b in [settingsButton, seedButton, darkButton] {
            b.contentTintColor = .md3(s.onSecondaryContainer)
            b.layer?.backgroundColor = CGColor.md3(s.secondaryContainer)
        }
        for f in [urlField, qualityField, limitField, dirField, connField, maxActiveField, glimitField, proxyField, stopStartField, stopEndField] {
            f.backgroundColor = .md3(s.surfaceContainerHigh)
            f.textColor = .md3(s.onSurface)
            f.wantsLayer = true
            f.layer?.cornerRadius = 8
        }
        shutdownCheck.contentTintColor = .md3(s.onSurface)
        tableView.reloadData()
    }

    // MARK: - NSTableView

    func numberOfRows(in tableView: NSTableView) -> Int {
        tasks.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let task = tasks[safe: row] else { return nil }
        let rowView = TaskRowView(frame: NSRect(x: 0, y: 0, width: max(tableView.bounds.width - 32, 200), height: 96))
        rowView.configure(with: task, scheme: scheme) { [weak self] action in
            self?.performAction(action, on: task.id)
        }
        return rowView
    }

    private func performAction(_ action: TaskRowAction, on id: UInt64) {
        switch action {
        case .pause: rpc.pause(id: id)
        case .resume: rpc.resume(id: id)
        case .remove: rpc.remove(id: id)
        }
    }

    // MARK: - helpers

    private func byteString(_ bytes: UInt64) -> String {
        let units: [(Double, String)] = [
            (1024 * 1024 * 1024, "GiB"), (1024 * 1024, "MiB"), (1024, "KiB"),
        ]
        let d = Double(bytes)
        for (mult, suffix) in units where d >= mult {
            return String(format: "%.1f %@", d / mult, suffix)
        }
        return "\(bytes) B"
    }

    private func isHHMM(_ s: String) -> Bool {
        let parts = s.split(separator: ":")
        guard parts.count == 2,
              let h = Int(parts[0]), let m = Int(parts[1]) else { return false }
        return (0..<24).contains(h) && (0..<60).contains(m)
    }
}

// MARK: - Task row view

private enum TaskRowAction {
    case pause, resume, remove
}

private final class TaskRowView: NSView {
    private let chip = NSTextField(labelWithString: "")
    private let name = NSTextField(labelWithString: "")
    private let stats = NSTextField(labelWithString: "")
    private let progress = NSProgressIndicator()
    private let detail = NSTextField(labelWithString: "")
    private let pauseButton = NSButton()
    private let resumeButton = NSButton()
    private let removeButton = NSButton()
    private var handlers: [ButtonHandler] = []
    private var actionSink: ((TaskRowAction) -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 12
        build()
    }

    required init?(coder: NSCoder) {
        nil
    }

    private func build() {
        let top = NSStackView()
        top.orientation = .horizontal
        top.spacing = 8
        top.translatesAutoresizingMaskIntoConstraints = false
        chip.font = NSFont.systemFont(ofSize: 11, weight: .semibold)
        chip.translatesAutoresizingMaskIntoConstraints = false
        chip.alignment = .center
        chip.wantsLayer = true
        chip.layer?.cornerRadius = 8
        top.addArrangedSubview(chip)
        name.font = NSFont.systemFont(ofSize: 14, weight: .semibold)
        name.lineBreakMode = .byTruncatingMiddle
        name.translatesAutoresizingMaskIntoConstraints = false
        top.addArrangedSubview(name)
        let spacer = NSView()
        top.addArrangedSubview(spacer)
        stats.font = NSFont.systemFont(ofSize: 12)
        stats.translatesAutoresizingMaskIntoConstraints = false
        top.addArrangedSubview(stats)
        for (b, t) in [(pauseButton, "Pause"), (resumeButton, "Resume"), (removeButton, "Remove")] {
            b.title = t
            b.bezelStyle = .rounded
            b.wantsLayer = true
            b.layer?.cornerRadius = 14
            b.font = NSFont.systemFont(ofSize: 12)
            b.translatesAutoresizingMaskIntoConstraints = false
            top.addArrangedSubview(b)
        }
        addSubview(top)

        progress.isIndeterminate = false
        progress.style = .bar
        progress.minValue = 0
        progress.maxValue = 100
        progress.wantsLayer = true
        progress.translatesAutoresizingMaskIntoConstraints = false
        addSubview(progress)

        detail.font = NSFont.systemFont(ofSize: 11)
        detail.lineBreakMode = .byTruncatingMiddle
        detail.translatesAutoresizingMaskIntoConstraints = false
        addSubview(detail)

        NSLayoutConstraint.activate([
            top.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            top.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            top.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            progress.topAnchor.constraint(equalTo: top.bottomAnchor, constant: 6),
            progress.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            progress.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -280),
            progress.heightAnchor.constraint(equalToConstant: 8),
            detail.topAnchor.constraint(equalTo: progress.bottomAnchor, constant: 4),
            detail.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            detail.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            detail.bottomAnchor.constraint(lessThanOrEqualTo: bottomAnchor, constant: -6),
            spacer.widthAnchor.constraint(greaterThanOrEqualToConstant: 10),
        ])
    }

    func configure(with task: TaskSnapshot, scheme: Md3Scheme, actions: @escaping (TaskRowAction) -> Void) {
        actionSink = actions
        let state = task.state ?? ""
        let chipColor: UInt32
        switch state {
        case "completed": chipColor = scheme.tertiary
        case "failed": chipColor = scheme.error
        case "queued", "paused": chipColor = scheme.onSurfaceVariant
        default: chipColor = scheme.primary
        }
        chip.stringValue = state.isEmpty ? "-" : state
        chip.textColor = .md3(scheme.onSurface)
        chip.layer?.backgroundColor = CGColor.md3(chipColor, alpha: 0.25)
        name.stringValue = "#\(task.id)  \(task.name ?? "")"
        name.textColor = .md3(scheme.onSurface)
        stats.textColor = .md3(scheme.onSurfaceVariant)
        detail.textColor = .md3(scheme.onSurfaceVariant)
        layer?.backgroundColor = CGColor.md3(scheme.surfaceContainer)
        layer?.borderColor = CGColor.md3(scheme.outlineVariant)
        layer?.borderWidth = 1.0

        let done = task.done ?? 0
        let size = task.size ?? 0
        let speed = task.speed ?? 0
        let eta = (task.etaSecs ?? 0) > 0 && state == "downloading" ? "\(task.etaSecs ?? 0)s" : "-"
        var extra = ""
        if let p = task.peers, p > 0 {
            extra = " \(p)p/\(task.seeds ?? 0)s"
        }
        let speedTxt = speed > 0 ? "\(byteString(speed))/s" : "-"
        stats.stringValue = "\(byteString(done)) / \(size > 0 ? byteString(size) : "?")  \(speedTxt)  \(eta)\(extra)"

        progress.doubleValue = size > 0 ? Double(done) / Double(size) * 100.0 : 0.0
        progress.layer?.backgroundColor = CGColor.md3(scheme.surfaceContainerHighest)
        progress.layer?.cornerRadius = 4
        progress.layer?.masksToBounds = true

        if let err = task.error, !err.isEmpty {
            detail.stringValue = err
            detail.textColor = .md3(scheme.error)
        } else if state == "completed" {
            detail.stringValue = task.outPath ?? ""
        } else {
            detail.stringValue = ""
        }

        let canPause = ["downloading", "probing", "queued"].contains(state)
        let canResume = ["paused", "failed"].contains(state)
        pauseButton.isHidden = !canPause
        resumeButton.isHidden = !canResume
        for b in [pauseButton, resumeButton] where !b.isHidden {
            b.layer?.backgroundColor = CGColor.md3(scheme.secondaryContainer)
            b.contentTintColor = .md3(scheme.onSecondaryContainer)
        }
        removeButton.layer?.backgroundColor = CGColor.md3(scheme.surfaceContainerLow)
        removeButton.contentTintColor = .md3(scheme.onSurfaceVariant)

        let h1 = ButtonHandler { actions(.pause) }
        let h2 = ButtonHandler { actions(.resume) }
        let h3 = ButtonHandler { actions(.remove) }
        pauseButton.target = h1
        pauseButton.action = #selector(ButtonHandler.fire)
        resumeButton.target = h2
        resumeButton.action = #selector(ButtonHandler.fire)
        removeButton.target = h3
        removeButton.action = #selector(ButtonHandler.fire)
        handlers = [h1, h2, h3]
    }

    private func byteString(_ bytes: UInt64) -> String {
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

/// Retains closures for NSButton targets (AppKit target/action needs
/// Objective-C selectors; this wrapper keeps Swift closures alive).
private final class ButtonHandler: NSObject {
    private let handler: () -> Void
    init(_ handler: @escaping () -> Void) {
        self.handler = handler
        super.init()
    }
    @objc func fire() {
        handler()
    }
}

extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
