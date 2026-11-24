// VelocityDM macOS frontend — entry point (Swift + AppKit).
//
// Run the shared Rust core first (the daemon binary), then this app:
//   velocitydm daemon --addr 127.0.0.1:36990
//   swift run VelocityDMMacApp        (or the built .app bundle)

import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    var controller: AppController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        controller = AppController()
        controller?.window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Tear down the SSE stream so no orphaned URLSession task
        // survives exit (spec §6.4).
        controller?.stopLiveUpdates()
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)

// Minimal menu so the app gets standard macOS behavior (Quit).
let mainMenu = NSMenu()
let appMenuItem = NSMenuItem(title: "VelocityDM", action: nil, keyEquivalent: "")
let appSubMenu = NSMenu()
appSubMenu.addItem(withTitle: "Quit VelocityDM", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
appMenuItem.submenu = appSubMenu
mainMenu.addItem(appMenuItem)
app.mainMenu = mainMenu

app.run()
