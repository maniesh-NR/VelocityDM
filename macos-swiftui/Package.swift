// swift-tools-version:5.9
// VelocityDM SwiftUI frontend (S12-mac2, preview) — the "macOS newer"
// stack of the platform matrix: SwiftUI @main App lifecycle over the
// shared token-engine port, a remote JSON-RPC client of the daemon's
// documented loopback protocol, and SSE event-stream consumption.
// Zero third-party dependencies (no Package.resolved is written).
// Deployment floor macOS 12: SwiftUI @main App needs 11+; .task/
// .onSubmit/.refreshable need 12+; 13-only APIs are forbidden
// unguarded (docs/SPECS/S12-MAC2-SWIFTUI.md rule 9). This does NOT
// raise macos-app's 10.15 floor — Monterey users keep the AppKit app.

import PackageDescription

let package = Package(
    name: "VelocityDMSwiftUIApp",
    platforms: [
        .macOS(.v12)
    ],
    targets: [
        .executableTarget(
            name: "VelocityDMSwiftUIApp",
            path: "Sources/VelocityDMSwiftUIApp"
        ),
        .testTarget(
            name: "VelocityDMSwiftUIAppTests",
            dependencies: ["VelocityDMSwiftUIApp"],
            path: "Tests/VelocityDMSwiftUIAppTests"
        ),
    ]
)
