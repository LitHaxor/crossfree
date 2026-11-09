import AppKit
import Foundation

/// `crossfree --json engine`.
struct EngineInfo: Decodable, Equatable {
    let root: String
    let version: String
    let signatureValid: Bool
    let teamId: String?
    let gatekeeperSource: String?
    let notarized: Bool

    /// The version without the `wine-` that Wine puts in front of it.
    var displayVersion: String {
        version.hasPrefix("wine-") ? String(version.dropFirst(5)) : version
    }

    /// What is wrong with the engine's signed loader, for the person to see,
    /// or `nil` when it is validly signed and notarized. The engine may
    /// still run; this is a warning.
    var problem: String? {
        if !signatureValid {
            return "The engine's loader has an invalid code signature."
        }
        if !notarized {
            return "The engine's loader is not notarized (Gatekeeper: \(gatekeeperSource ?? "rejected"))."
        }
        return nil
    }
}

/// A bottle's settings (`crossfree --json settings NAME`).
struct BottleSettings: Codable, Hashable {
    /// `auto`, `mtld3d` or `wined3d`.
    var d3d9: String
    /// `auto`, `dxmt` or `wined3d`.
    var dxgi: String
    var msync: Bool
    var metalHud: Bool
}

/// A Wine prefix (`crossfree --json prefix list`).
struct Bottle: Decodable, Identifiable, Hashable {
    let name: String
    let path: String
    var settings: BottleSettings
    var id: String { name }

    var driveC: URL { URL(fileURLWithPath: path).appendingPathComponent("drive_c") }
}

/// A program installed in a bottle (`crossfree --json apps NAME`).
struct AppShortcut: Decodable, Identifiable, Hashable {
    let name: String
    let path: String
    /// The program's icon: an `.ico` file holding every size, which the
    /// launcher extracts from the shortcut's program or icon file.
    let icon: String?
    /// Steam games: the app ID and the library artwork Steam has cached.
    var steamAppId: Int? = nil
    /// Wide background banner.
    var hero: String? = nil
    /// The game's logo on a transparent background.
    var logo: String? = nil
    /// Portrait cover.
    var cover: String? = nil
    /// Small horizontal banner.
    var header: String? = nil
    /// The Windows program a `.lnk` runs (`C:\…\steam.exe`): Wine names
    /// the program's process after it.
    var program: String? = nil
    /// The Windows folder a Steam game is installed in; its processes run
    /// from there.
    var installDir: String? = nil
    var id: String { path }

    var iconImage: NSImage? { Images.image(atPath: icon) }
    var heroImage: NSImage? { Images.image(atPath: hero) }
    var logoImage: NSImage? { Images.image(atPath: logo) }
    /// Background art for a tile or splash: hero, cover or header.
    var artImage: NSImage? { heroImage ?? Images.image(atPath: cover) ?? Images.image(atPath: header) }
}

/// What the sidebar selects.
enum SidebarItem: Hashable {
    case home
    case bottle(String)
}

enum JSON {
    static func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(type, from: data)
    }
}

/// Bottle names the launcher accepts: 1–64 letters, digits, spaces, `.`,
/// `_` or `-`, not starting with `.` or a space.
func isValidBottleName(_ name: String) -> Bool {
    guard !name.isEmpty, name.count <= 64, let first = name.first, first != ".", first != " " else {
        return false
    }
    return name.allSatisfy { ($0.isASCII && ($0.isLetter || $0.isNumber)) || " ._-".contains($0) }
}

/// Local view state. Stands in for SwiftUI's `@State`, which the macOS 27 SDK
/// implements as a macro whose plugin ships only with Xcode, not with the
/// Command Line Tools this project builds with. Use as
/// `@StateObject private var flag = Local(false)` and bind `$flag.value`.
final class Local<Value>: ObservableObject {
    @Published var value: Value
    init(_ value: Value) { self.value = value }
}
