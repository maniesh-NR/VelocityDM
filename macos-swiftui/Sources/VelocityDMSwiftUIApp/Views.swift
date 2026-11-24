// VelocityDM SwiftUI frontend — views.
//
// macOS 12 floor (S12-mac2 spec rule 9): no NavigationStack/Grid/
// LabeledContent/.scrollContentBackground (13-only APIs are forbidden
// unguarded — the CI witness builds against the 12 target). All
// daemon-relayed strings render through `VDMText` (rule 2); alerts and
// tooltips are not used, so no String-title LocalizedStringKey axis
// exists. Styling consumes `Md3Scheme` fields only; no daemon string
// ever reaches a color.

import SwiftUI

// MARK: - Entry

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model = AppModel()

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Tear down the SSE stream so no orphaned URLSession task
        // survives exit (S12-mac2 rule 8; the AppKit tree's spec §6.4
        // analog).
        model.stopLiveUpdates()
    }
}

@main
struct VelocityDMApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var adaptor

    var body: some Scene {
        WindowGroup("VelocityDM") {
            RootView(model: adaptor.model)
                .frame(minWidth: 860, minHeight: 580)
        }
    }
}

// MARK: - Root

struct RootView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        let s = model.scheme
        VStack(alignment: .leading, spacing: 8) {
            header(s)
            addBar(s)
            torrentBar(s)
            if let e = model.addError {
                VDMText(e)
                    .font(.system(size: 12))
                    .foregroundColor(Color(md3Hex: s.error))
                    .padding(.horizontal, 4)
            }
            if model.settingsVisible {
                SettingsPanel(model: model, scheme: s)
            }
            taskList(s)
        }
        .padding(16)
        .background(Color(md3Hex: s.surfaceContainerLow))
        .task { model.start() }
    }

    private func header(_ s: Md3Scheme) -> some View {
        HStack(spacing: 10) {
            Text(verbatim: "VelocityDM")
                .font(.system(size: 20, weight: .bold))
                .foregroundColor(Color(md3Hex: s.onSurface))
            VDMButton(title: "Settings", scheme: s) { model.toggleSettings() }
            Spacer()
            VDMText(model.banner)
                .font(.system(size: 12))
                .foregroundColor(Color(md3Hex: s.onSurfaceVariant))
            VDMButton(title: md3SeedName(model.prefsSeed), scheme: s) { model.cycleSeed() }
            VDMButton(title: model.prefsDark ? "Dark" : "Light", scheme: s) { model.toggleDark() }
        }
    }

    private func addBar(_ s: Md3Scheme) -> some View {
        HStack(spacing: 8) {
            TextField("Paste a URL or magnet:", text: $model.urlField)
                .textFieldStyle(.roundedBorder)
                .onSubmit { model.addDownload() }
            TextField("filename", text: $model.filenameField)
                .textFieldStyle(.roundedBorder)
                .frame(width: 130)
                .onSubmit { model.addDownload() }
            TextField("best", text: $model.qualityField)
                .textFieldStyle(.roundedBorder)
                .frame(width: 70)
                .onSubmit { model.addDownload() }
            TextField("off", text: $model.limitField)
                .textFieldStyle(.roundedBorder)
                .frame(width: 80)
                .onSubmit { model.addDownload() }
            VDMButton(title: "Add", scheme: s) { model.addDownload() }
        }
    }

    private func torrentBar(_ s: Md3Scheme) -> some View {
        HStack(spacing: 8) {
            TextField("Path to a local .torrent", text: $model.torrentPathField)
                .textFieldStyle(.roundedBorder)
                .onSubmit { model.addTorrentFile() }
            VDMButton(title: "Add torrent", scheme: s) { model.addTorrentFile() }
        }
    }

    private func taskList(_ s: Md3Scheme) -> some View {
        ZStack {
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(model.tasks, id: \.id) { task in
                        TaskRowView(
                            task: task,
                            scheme: s,
                            onPause: { model.pause(id: task.id) },
                            onResume: { model.resume(id: task.id) },
                            onRemove: { model.remove(id: task.id) }
                        )
                    }
                }
                .padding(.horizontal, 4)
                .padding(.vertical, 8)
            }
            if model.tasks.isEmpty {
                VDMText("No downloads yet. Paste a link above or use the browser extension.")
                    .font(.system(size: 13))
                    .foregroundColor(Color(md3Hex: s.onSurfaceVariant))
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 40)
            }
        }
    }
}

// MARK: - Settings panel (§7)

struct SettingsPanel: View {
    @ObservedObject var model: AppModel
    let scheme: Md3Scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                labeled("Folder", $model.dirField, width: 260, scheme: scheme)
                labeled("Conn", $model.connField, width: 60, scheme: scheme)
                labeled("Max", $model.maxActiveField, width: 60, scheme: scheme)
                labeled("Limit", $model.glimitField, width: 90, scheme: scheme)
                labeled("Proxy", $model.proxyField, width: 220, scheme: scheme)
            }
            HStack(spacing: 8) {
                labeled("Stop", $model.stopStartField, width: 70, scheme: scheme)
                labeled("to", $model.stopEndField, width: 70, scheme: scheme)
                Toggle("Shut down when queue drains", isOn: $model.shutdownWhenDone)
                    .foregroundColor(Color(md3Hex: scheme.onSurface))
                VDMButton(title: "Apply", scheme: scheme) { model.applySettings() }
            }
        }
        .padding(8)
        .background(Color(md3Hex: scheme.surfaceContainer))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private func labeled(_ label: String, _ binding: Binding<String>, width: CGFloat, scheme: Md3Scheme) -> some View {
        HStack(spacing: 4) {
            Text(verbatim: label)
                .font(.system(size: 11))
                .foregroundColor(Color(md3Hex: scheme.onSurfaceVariant))
            TextField("", text: binding)
                .textFieldStyle(.roundedBorder)
                .frame(width: width)
        }
    }
}

// MARK: - Task row (§6)

struct TaskRowView: View {
    let task: TaskSnapshot
    let scheme: Md3Scheme
    let onPause: () -> Void
    let onResume: () -> Void
    let onRemove: () -> Void

    private var state: String { task.state ?? "" }

    private var chipColor: UInt32 {
        switch state {
        case "completed": return scheme.tertiary
        case "failed": return scheme.error
        case "queued", "paused": return scheme.onSurfaceVariant
        default: return scheme.primary
        }
    }

    /// Progress fraction clamped 0...1 (spec §6).
    private var fraction: Double {
        let size = Double(task.size ?? 0)
        guard size > 0 else { return 0 }
        return min(1.0, max(0.0, Double(task.done ?? 0) / size))
    }

    private var statsText: String {
        let done = task.done ?? 0
        let size = task.size ?? 0
        let speed = task.speed ?? 0
        let speedTxt = speed > 0 ? "\(Self.bytes(speed))/s" : "-"
        let eta = (task.etaSecs ?? 0) > 0 && state == "downloading" ? "\(task.etaSecs ?? 0)s" : "-"
        var extra = ""
        if let p = task.peers, p > 0 {
            extra = " \(p)p/\(task.seeds ?? 0)s"
        }
        return "\(Self.bytes(done)) / \(size > 0 ? Self.bytes(size) : "?")  \(speedTxt)  \(eta)\(extra)"
    }

    private var canPause: Bool { ["downloading", "probing", "queued"].contains(state) }
    private var canResume: Bool { ["paused", "failed"].contains(state) }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                VDMText(state.isEmpty ? "-" : state)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundColor(Color(md3Hex: scheme.onSurface))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(Color(md3Hex: chipColor, alpha: 0.25))
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                VDMText("#\(task.id)  \(task.name ?? "")")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundColor(Color(md3Hex: scheme.onSurface))
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer()
                VDMText(statsText)
                    .font(.system(size: 12))
                    .foregroundColor(Color(md3Hex: scheme.onSurfaceVariant))
                if canPause {
                    VDMButton(title: "Pause", scheme: scheme, action: onPause)
                }
                if canResume {
                    VDMButton(title: "Resume", scheme: scheme, action: onResume)
                }
                VDMButton(title: "Remove", scheme: scheme, surface: true, action: onRemove)
            }
            ProgressView(value: fraction)
                .progressViewStyle(.linear)
                .tint(Color(md3Hex: scheme.primary))
            if let err = task.error, !err.isEmpty {
                VDMText(err)
                    .font(.system(size: 11))
                    .foregroundColor(Color(md3Hex: scheme.error))
                    .lineLimit(2)
            }
            if let notice = task.notice, !notice.isEmpty {
                VDMText(notice)
                    .font(.system(size: 11))
                    .foregroundColor(Color(md3Hex: scheme.onSurfaceVariant))
                    .lineLimit(2)
            }
            if state == "completed" && (task.error ?? "").isEmpty {
                VDMText(task.outPath ?? "")
                    .font(.system(size: 11))
                    .foregroundColor(Color(md3Hex: scheme.onSurfaceVariant))
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
        .padding(10)
        .background(Color(md3Hex: scheme.surfaceContainer))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color(md3Hex: scheme.outlineVariant), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    static func bytes(_ bytes: UInt64) -> String {
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

// MARK: - MD3-styled button

struct VDMButton: View {
    let title: String
    let scheme: Md3Scheme
    var surface = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(verbatim: title)
                .font(.system(size: 12))
                .padding(.horizontal, 12)
                .padding(.vertical, 5)
                .background(Color(md3Hex: surface ? scheme.surfaceContainerLow : scheme.secondaryContainer))
                .foregroundColor(Color(md3Hex: surface ? scheme.onSurfaceVariant : scheme.onSecondaryContainer))
                .clipShape(RoundedRectangle(cornerRadius: 14))
        }
        .buttonStyle(.plain)
    }
}
