import AppKit
import Foundation
import Testing

@testable import Crossfree

struct CatalogTests {
    /// Every store gets a Windows named after it, so its name must be one
    /// the launcher accepts; its installer comes from its publisher over
    /// HTTPS, and is a type Wine can run.
    @Test func catalogEntriesAreUsable() {
        let items = Catalog.items
        #expect(items.count == 7)
        #expect(Set(items.map(\.id)).count == items.count)
        #expect(Set(items.map { $0.name.lowercased() }).count == items.count)
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: " ._-"))
        for item in items {
            #expect(!item.name.isEmpty && item.name.count <= 64, "\(item.name)")
            #expect(item.name.unicodeScalars.allSatisfy { $0.isASCII && allowed.contains($0) }, "\(item.name)")
            #expect(item.installer.scheme == "https", "\(item.id)")
            #expect(
                ["exe", "msi"].contains(URL(fileURLWithPath: item.fileName).pathExtension.lowercased()), "\(item.id)")
            #expect(!item.fileName.contains("/"))
        }
    }

    /// Every store has its logo (made by tools/store-logos.py), which the
    /// app bundle carries in Resources/Stores.
    @Test func everyStoreHasALogo() {
        let app = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        for item in Catalog.items {
            let path = app.appendingPathComponent("Resources/Stores/\(item.id).png").path
            let logo = NSImage(contentsOfFile: path)
            #expect(logo?.isValid == true, "\(item.id)")
            #expect((logo?.representations.first?.pixelsWide ?? 0) >= 128, "\(item.id)")
        }
    }

    /// The README and the landing page list the stores by name, so a store
    /// added to the catalog must be added to them too.
    @Test func theReadmeAndTheSiteNameEveryStore() throws {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent()
        for page in ["README.md", "site/index.html"] {
            let text = try String(contentsOf: root.appendingPathComponent(page), encoding: .utf8)
            for item in Catalog.items {
                #expect(text.contains(item.name), "\(page) doesn't name \(item.name)")
            }
        }
    }

    /// Grey brands (Epic) get the default backdrop hue, not red.
    @Test func greyBrandsGetTheDefaultHue() {
        let epic = Catalog.items.first { $0.id == "epic" }!
        let steam = Catalog.items.first { $0.id == "steam" }!
        #expect(epic.hue == 0.6)
        #expect(steam.hue > 0.5 && steam.hue < 0.65)
    }

    /// The downloader saves what it fetched where it was asked, replacing
    /// an older download.
    @Test func downloadsReplaceTheDestination() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("crossfree-dl-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let source = dir.appendingPathComponent("source.exe")
        try Data(repeating: 0x4D, count: 200_000).write(to: source)
        let destination = dir.appendingPathComponent("installers/Setup.exe")
        try FileManager.default.createDirectory(
            at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("old".utf8).write(to: destination)

        try await Downloader.fetch(source, to: destination) { _ in }
        #expect(try Data(contentsOf: destination) == Data(repeating: 0x4D, count: 200_000))
    }
}

@MainActor
struct CatalogStoreTests {
    let steam = Catalog.items.first { $0.id == "steam" }!

    /// A fake download: writes the installer, reporting progress.
    static func fakeFetch(_ url: URL, _ destination: URL, _ progress: @escaping @Sendable (Double?) -> Void)
        async throws
    {
        progress(nil)
        progress(0.5)
        try FileManager.default.createDirectory(
            at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("MZ".utf8).write(to: destination)
        progress(1)
    }

    func store(_ fake: FakeCLI) -> Store {
        let store = Store(cli: fake.cli)
        store.installerFolder = fake.dir.appendingPathComponent("installers")
        store.fetch = CatalogStoreTests.fakeFetch
        return store
    }

    /// Install downloads the store's installer, makes its Windows, named
    /// after it, and runs the installer there; once its shortcut shows up,
    /// the store is installed and opens its own app.
    @Test func installingAStore() async throws {
        let fake = try FakeCLI()
        let store = store(fake)
        await store.refresh()
        #expect(store.state(of: steam) == .available)

        await store.get(steam)
        let installer = store.installerFolder.appendingPathComponent("SteamSetup.exe").path
        #expect(FileManager.default.fileExists(atPath: installer))
        #expect(store.bottleNames == ["b", "Steam"])
        #expect(store.getting == nil && store.busy == nil && store.errorMessage == nil)
        #expect(await eventually { fake.lines.contains { $0.hasSuffix(" run Steam -- \(installer)") } })

        let launcher = AppShortcut(name: "Steam", path: "/p/Steam/Steam.lnk", icon: nil)
        let support = AppShortcut(name: "Steam Support Center", path: "/p/Steam/Support.url", icon: nil)
        store.apps["Steam"] = [support, launcher]
        #expect(store.state(of: steam) == .installed)
        #expect(store.app(for: steam) == support)  // the first whose name has "Steam"
        store.apps["Steam"] = [launcher, support]
        #expect(store.app(for: steam) == launcher)
    }

    /// An installer closed before it finished leaves an empty Windows: the
    /// store is available again, and the next try reuses that Windows.
    @Test func anUnfinishedInstallIsRetriedInPlace() async throws {
        let fake = try FakeCLI()
        try "Steam\n".write(to: fake.created, atomically: true, encoding: .utf8)
        let store = store(fake)
        await store.refresh()
        #expect(store.bottleNames == ["b", "Steam"])
        #expect(store.state(of: steam) == .available)

        await store.get(steam)
        #expect(await eventually { fake.lines.contains { $0.contains(" run Steam -- ") } })
        #expect(!fake.lines.contains { $0.hasSuffix("prefix create Steam") })
        #expect(store.bottleNames == ["b", "Steam"])
    }

    /// A failed download says so, and makes nothing.
    @Test func aFailedDownloadMakesNothing() async throws {
        let fake = try FakeCLI()
        let store = store(fake)
        store.fetch = { _, _, _ in throw URLError(.notConnectedToInternet) }
        await store.refresh()

        await store.get(steam)
        #expect(store.errorMessage?.hasPrefix("Couldn’t download Steam") == true)
        #expect(store.getting == nil)
        #expect(store.bottleNames == ["b"])
        #expect(!fake.lines.contains { $0.contains("prefix create") })
    }

    /// Cancelling stops the download quietly.
    @Test func cancellingADownloadIsQuiet() async throws {
        let fake = try FakeCLI()
        let store = store(fake)
        store.fetch = { _, _, _ in try await Task.sleep(for: .seconds(30)) }
        await store.refresh()

        store.beginGet(steam)
        #expect(await eventually { store.state(of: steam) == .downloading(nil) })
        store.cancelGet()
        #expect(await eventually { store.getting == nil })
        #expect(store.errorMessage == nil)
        #expect(store.state(of: steam) == .available)
        #expect(!fake.lines.contains { $0.contains("prefix create") })
    }

    /// Nothing installs before the engine check has passed.
    @Test func installingNeedsAWorkingEngine() async throws {
        let fake = try FakeCLI()
        let store = store(fake)
        await store.get(steam)
        #expect(store.getting == nil)
        #expect(!FileManager.default.fileExists(atPath: store.installerFolder.path))
    }

    /// Players see the engine only when nothing can run: a loader that is
    /// not notarized is not a problem for them.
    @Test func onlyAFailedEngineIsAProblem() async throws {
        let fake = try FakeCLI(notarized: false)
        let store = Store(cli: fake.cli)
        #expect(store.setupProblem == nil)  // still checking
        await store.refresh()
        #expect(store.engineProblem != nil)
        #expect(store.setupProblem == nil)

        store.engine = nil
        store.engineProblem = "missing bin/wine"
        #expect(store.setupProblem == "missing bin/wine")
    }
}
