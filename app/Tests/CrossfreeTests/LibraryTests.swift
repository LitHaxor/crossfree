import Foundation
import Testing

@testable import Crossfree

/// The library row's order and the artwork the launcher reports.
struct LibraryTests {
    let bottle = Bottle(
        name: "Steam", path: "/p/Steam",
        settings: BottleSettings(d3d9: "auto", dxgi: "auto", msync: true, metalHud: false)
    )

    func app(_ name: String) -> AppShortcut {
        AppShortcut(name: name, path: "/p/Steam/\(name).lnk", icon: nil)
    }

    /// Most recently played first, then never-played games in their
    /// original (alphabetical) order.
    @Test func recentlyPlayedComeFirst() {
        let items = ["Alpha", "Beta", "Gamma", "Delta"].map { (bottle: bottle, app: app($0)) }
        let now = Date()
        let played = [
            app("Gamma").id: now.addingTimeInterval(-60),
            app("Delta").id: now,
        ]
        let order = Store.order(items, lastPlayed: played).map(\.app.name)
        #expect(order == ["Delta", "Gamma", "Alpha", "Beta"])
        #expect(Store.order(items, lastPlayed: [:]).map(\.app.name) == ["Alpha", "Beta", "Gamma", "Delta"])
    }

    /// Names for an installer's Windows: the product, not the installer.
    @Test func installerNames() {
        func name(_ path: String, _ taken: [String] = []) -> String {
            Store.environmentName(for: URL(fileURLWithPath: path), taken: taken)
        }
        #expect(name("/Downloads/SteamSetup.exe") == "Steam")
        #expect(name("/Downloads/GOG_Galaxy_2.0.exe") == "GOG Galaxy")
        #expect(name("/Downloads/EpicInstaller-15.17.1.msi") == "Epic")
        #expect(name("/Downloads/setup_witcher3_v4.04_x64.exe") == "witcher3")
        #expect(name("/Volumes/Rise of Nations/setup.exe") == "Rise of Nations")
        #expect(name("/setup.exe") == "Game")
        #expect(name("/Downloads/Größe-Setup.exe") == "Groe")
        #expect(name("/Downloads/SteamSetup.exe", ["steam"]) == "Steam 2")
        #expect(name("/Downloads/SteamSetup.exe", ["Steam", "Steam 2"]) == "Steam 3")
        let long = name("/Downloads/" + String(repeating: "Very Long Name ", count: 10) + ".exe")
        #expect(long.count <= 40 && !long.hasSuffix(" ") && isValidBottleName(long))
    }

    /// `apps --json` with Steam artwork, and without (older launchers too).
    @Test func appsDecodeWithAndWithoutArtwork() throws {
        let json = """
            [{"name":"Rise of Nations","path":"/p/RoN.url","icon":"/c/1.ico","steam_app_id":287450,
              "hero":"/a/library_hero.jpg","logo":"/a/logo.png","cover":"/a/library_600x900.jpg","header":null},
             {"name":"Steam","path":"/p/Steam.lnk","icon":null}]
            """
        let apps = try JSON.decode([AppShortcut].self, from: Data(json.utf8))
        #expect(apps[0].steamAppId == 287450)
        #expect(apps[0].hero == "/a/library_hero.jpg")
        #expect(apps[0].logo == "/a/logo.png")
        #expect(apps[0].cover == "/a/library_600x900.jpg")
        #expect(apps[0].header == nil)
        #expect(apps[1].steamAppId == nil && apps[1].hero == nil && apps[1].cover == nil)
    }
}

/// The app finds a running game by the bundle name the launcher gives it
/// (`bundle_name` in crates/launcher/src/dock.rs); the two must agree.
@Test func dockNamesMatchTheLaunchers() {
    #expect(Store.dockName("Steam") == "Steam")
    #expect(Store.dockName("  Rise of Nations:  Extended/Edition ") == "Rise of Nations Extended Edition")
    #expect(Store.dockName("..hidden") == "hidden")
    #expect(Store.dockName(String(repeating: "x", count: 100))?.count == 64)
    for nothing in ["", "  ", "/", ".", "wine", "WINE"] {
        #expect(Store.dockName(nothing) == nil, "\(nothing)")
    }
    #expect(Store.runningApps(named: "No Such Game Is Running").isEmpty)
}

/// A game runs under its shortcut's file name, which may differ from its
/// display name; a game that is not running is not resumed.
@MainActor
@Test func gamesRunUnderTheirShortcutsName() async throws {
    let app = AppShortcut(name: "Steam", path: "/p/Steam/Start Menu/Steam.lnk", icon: nil)
    #expect(Store.launchName(app) == "Steam")
    #expect(Store.launchName(AppShortcut(name: "Game", path: "/p/x/Game Launcher.url", icon: nil)) == "Game Launcher")
    let fake = try FakeCLI()
    let store = Store(cli: fake.cli)
    let bottle = Bottle(
        name: "b", path: "/p/b", settings: BottleSettings(d3d9: "auto", dxgi: "auto", msync: true, metalHud: false))
    #expect(!store.resume(bottle, AppShortcut(name: "Nothing", path: "/p/x/No Such Running Game.lnk", icon: nil)))
    #expect(store.lastPlayed.isEmpty)
}
