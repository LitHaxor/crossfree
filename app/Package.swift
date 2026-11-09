// swift-tools-version:6.0
import PackageDescription

// The crossfree Mac app: a SwiftUI front end over the `crossfree`
// command-line tool (crates/launcher), which it runs with `--json`.
let package = Package(
    name: "Crossfree",
    platforms: [.macOS(.v15)],
    targets: [
        .executableTarget(
            name: "Crossfree",
            path: "Sources/Crossfree",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        // The app's logic (Store, CLI, Models) against a fake `crossfree`;
        // swift-testing, which the Command Line Tools ship (XCTest they do not).
        // Their build system does not find its macro plugin by itself, so run
        //   swift test --package-path app -Xswiftc -plugin-path \
        //     -Xswiftc /Library/Developer/CommandLineTools/usr/lib/swift/host/plugins/testing
        .testTarget(
            name: "CrossfreeTests",
            dependencies: ["Crossfree"],
            path: "Tests/CrossfreeTests",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
