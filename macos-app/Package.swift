// swift-tools-version:5.9
// VelocityDM macOS frontend — Swift + AppKit (the macOS stack of the
// platform matrix: Swift language, AppKit UI for older macOS versions;
// a SwiftUI variant for newer systems is a tracked sub-stage).
// Swift Package Manager; talk to the shared Rust core over the daemon's
// documented loopback JSON-RPC protocol (docs/RPC.md).

import PackageDescription

let package = Package(
    name: "VelocityDMMacApp",
    platforms: [
        .macOS(.v10_15)
    ],
    targets: [
        .executableTarget(
            name: "VelocityDMMacApp",
            path: "Sources/VelocityDMMacApp"
        )
    ]
)
