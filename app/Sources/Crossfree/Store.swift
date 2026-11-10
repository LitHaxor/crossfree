import AppKit
import Foundation

/// App state, and every action the UI can take. Actions go through the
/// `crossfree` command-line tool.
@MainActor
final class Store: ObservableObject {
    /// The engine's report, when the tool could read the engine. It may
    /// still have a ``engineProblem`` (a loader that is not notarized).
    @Published var engine: EngineInfo?
    /// What is wrong with the engine, for a banner: no usable engine (then
    /// ``engine`` is `nil`), or a warning about its loader. `nil` when all is
    /// well or while the first check runs.
    @Published var engineProblem: String?
    @Published var bottles: [Bottle] = []
    @Published var apps: [String: [AppShortcut]] = [:]
    @Published var selection: SidebarItem? = .home
    /// What a long operation (creating or deleting a bottle) is doing; while
    /// it is set, launching and bottle changes are disabled.
    @Published var busy: String?
    /// An error to show in an alert; set it to `nil` to dismiss.
    @Published var errorMessage: String?
    /// Programs started from the app and still running, per bottle.
    @Published var running: [String: Int] = [:]
    /// Set by ⌘N (Add Games…): the console goes to the Directory.
    @Published var installing = false
    /// The game whose window crossfree last put in front (its ID), once it
    /// came up after Play: the launch splash closes then.
    @Published var shownGame: String?
    /// The games running now (their IDs), seen every two seconds.
    @Published private(set) var runningGames: Set<String> = []
    private var runningWatch: Task<Void, Never>?
    /// Games started from Play whose window has not come up yet (their
    /// IDs), and when each started: Play on one waits for its window instead
    /// of starting a second copy.
    private(set) var starting: [String: Date] = [:]
    /// The wait for each game's window (``bringToFront(_:_:)``), by ID.
    private var windowWaits: [String: Task<Void, Never>] = [:]
    /// When each app (by shortcut path) was last opened from crossfree.
    @Published private(set) var lastPlayed: [String: Date] = [:]
    /// The store from the catalog being installed, if any.
    @Published var getting: Getting?

    /// Downloads an installer (`url`, `destination`, progress); tests
    /// replace it.
    var fetch: (URL, URL, @escaping @Sendable (Double?) -> Void) async throws -> Void = Downloader.fetch
    /// Where downloaded installers are kept.
    var installerFolder = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("crossfree/installers")
    private var getTask: Task<Void, Never>?

    private var cli: CLI?
    /// Where ``lastPlayed`` is kept; `nil` keeps it in memory only (tests).
    private var defaults: UserDefaults?
    private static let lastPlayedKey = "lastPlayed"
    /// The last settings change: each change waits for the one before it,
    /// so changes reach the tool, and their results the UI, in order.
    private var settingsChange: Task<Void, Never>?

    init() {
        defaults = .standard
        let stored = UserDefaults.standard.dictionary(forKey: Store.lastPlayedKey) as? [String: Double] ?? [:]
        lastPlayed = stored.mapValues(Date.init(timeIntervalSince1970:))
        configure()
        watchRunning()
    }

    /// A store that runs `cli`, without reading the environment (tests).
    init(cli: CLI) {
        self.cli = cli
    }

    // MARK: State for the views

    /// Whether programs can be started: an engine the tool could read, and
    /// no long operation running. Gates app tiles, Run, Install, the tools,
    /// Quit All and New Bottle.
    var canLaunch: Bool { engine != nil && busy == nil }

    /// Whether Install Application can run: ``canLaunch`` and a bottle to
    /// install into.
    var canInstall: Bool { canLaunch && !bottles.isEmpty }

    /// The bottles' names, for `.task(id:)`: apps reload when this changes.
    var bottleNames: [String] { bottles.map(\.name) }

    /// Why nothing can run, once the engine check has failed. `nil` while
    /// it runs and when the engine works: warnings (a loader that is not
    /// notarized) are not the player's concern, and are not shown.
    var setupProblem: String? { engine == nil ? engineProblem : nil }

    // MARK: Engine

    /// Engine bundle: `$CROSSFREE_ENGINE`, one chosen with Choose Engine…,
    /// the one inside the app (`Contents/SharedSupport/engine`, put there by
    /// tools/build-app.sh, as CrossOver ships its Wine), or
    /// `~/Library/Application Support/crossfree/engine`.
    var enginePath: URL {
        if let path = ProcessInfo.processInfo.environment["CROSSFREE_ENGINE"] {
            return URL(fileURLWithPath: path)
        }
        if let path = UserDefaults.standard.string(forKey: "enginePath") {
            return URL(fileURLWithPath: path)
        }
        if let bundled = Bundle.main.sharedSupportURL?.appendingPathComponent("engine"),
            FileManager.default.fileExists(atPath: bundled.path)
        {
            return bundled
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("crossfree/engine")
    }

    private func configure() {
        guard let executable = CLI.locate() else {
            cli = nil
            engineProblem = "The crossfree command-line tool is missing from the app bundle."
            return
        }
        cli = CLI(executable: executable, engine: enginePath)
    }

    func chooseEngine(_ url: URL) async {
        UserDefaults.standard.set(url.path, forKey: "enginePath")
        configure()
        await reloadAll()
    }

    /// Asks for the engine folder, then uses it.
    func pickEngine() {
        let panel = NSOpenPanel()
        panel.message = "Choose the engine folder (the one containing bin and lib)."
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let url = panel.url {
            Task { await chooseEngine(url) }
        }
    }

    // MARK: Loading

    /// Checks the engine and lists the bottles. The apps reload separately:
    /// ``reloadAll()``, or a view's `.task(id: bottleNames)`.
    func refresh() async {
        guard let cli else { return }
        // At the same time: listing does not need the engine, and checking
        // the engine can take a while (code signature, Gatekeeper, or macOS
        // asking permission for the folder it is in).
        let engineCheck = Task { try await cli.run(["engine"]) }
        let listing = Task { try await cli.run(["prefix", "list"]) }
        do {
            bottles = try JSON.decode([Bottle].self, from: try await listing.value)
        } catch {
            bottles = []
            report(error)
        }
        do {
            let info = try JSON.decode(EngineInfo.self, from: try await engineCheck.value)
            engine = info
            engineProblem = info.problem
        } catch {
            engine = nil
            engineProblem = error.localizedDescription
        }
        if case .bottle(let name)? = selection, !bottles.contains(where: { $0.name == name }) {
            selection = .home
        }
    }

    /// ``refresh()``, then every bottle's apps (the Refresh command).
    func reloadAll() async {
        await refresh()
        await loadAllApps()
    }

    /// Lists the installs and their apps again, without checking the
    /// engine: when the app comes to the front, since a store may have
    /// installed games in the meantime.
    func refreshLibrary() async {
        guard let cli else { return }
        do {
            bottles = try JSON.decode([Bottle].self, from: try await cli.run(["prefix", "list"]))
        } catch {
            report(error)
            return
        }
        await loadAllApps()
    }

    func loadApps(_ bottle: Bottle) async {
        guard let cli else { return }
        do {
            apps[bottle.name] = try JSON.decode([AppShortcut].self, from: try await cli.run(["apps", bottle.name]))
        } catch {
            report(error)
        }
    }

    /// Load apps from every bottle (for the Home view).
    func loadAllApps() async {
        for bottle in bottles {
            await loadApps(bottle)
        }
    }

    /// All apps, most recently played first (as a console's home row), then
    /// by name.
    var orderedApps: [(bottle: Bottle, app: AppShortcut)] {
        Store.order(allApps, lastPlayed: lastPlayed)
    }

    nonisolated static func order(
        _ items: [(bottle: Bottle, app: AppShortcut)],
        lastPlayed: [String: Date]
    ) -> [(bottle: Bottle, app: AppShortcut)] {
        items.enumerated().sorted { a, b in
            switch (lastPlayed[a.element.app.id], lastPlayed[b.element.app.id]) {
            case (let x?, let y?) where x != y: return x > y
            case (_?, nil): return true
            case (nil, _?): return false
            default: return a.offset < b.offset
            }
        }.map(\.element)
    }

    /// All apps across all bottles, paired with their bottle for launching.
    var allApps: [(bottle: Bottle, app: AppShortcut)] {
        var result: [(bottle: Bottle, app: AppShortcut)] = []
        for bottle in bottles {
            for app in apps[bottle.name] ?? [] {
                result.append((bottle: bottle, app: app))
            }
        }
        return result.sorted(by: { a, b in a.app.name.localizedCaseInsensitiveCompare(b.app.name) == .orderedAscending }
        )
    }

    // MARK: Installing

    // People see games, not Wine prefixes ("bottles"): each installer gets
    // a prefix of its own, named after it, so one game cannot break
    // another. Games a store installs (Steam's) go in the store's prefix.

    /// Runs the installer `file`: alongside an installed program (in its
    /// prefix: patches, DLC, mods), or else in a new prefix named after the
    /// installer. What it adds to the Start Menu then shows in the library.
    func install(_ file: URL, alongside existing: Bottle? = nil) async {
        if let existing {
            runProgram(existing, file: file)
            return
        }
        let name = Store.environmentName(for: file, taken: bottleNames)
        guard await createBottle(named: name, for: file.lastPathComponent),
            let bottle = bottles.first(where: { $0.name == name })
        else { return }
        runProgram(bottle, file: file)
    }

    /// A prefix name for the installer at `url`: its file name without
    /// "setup", "installer", versions and the like (`SteamSetup.exe` is
    /// "Steam", `GOG_Galaxy_2.0.exe` "GOG Galaxy"), or its folder's name for
    /// a plain `setup.exe`; made unique among `taken`. Always a name the
    /// launcher accepts.
    nonisolated static func environmentName(for url: URL, taken: [String]) -> String {
        let noise: Set<String> = [
            "setup", "installer", "install", "x64", "x86", "win64", "win32", "windows", "amd64",
            "full", "offline", "online", "web", "launcher",
        ]
        func clean(_ raw: String) -> String {
            var spaced = ""
            var previous: Character?
            for character in raw.folding(options: .diacriticInsensitive, locale: nil) {
                if "_-.".contains(character) {
                    spaced.append(" ")
                } else {
                    // Split "SteamSetup" into "Steam Setup".
                    if let previous, previous.isLowercase, character.isUppercase { spaced.append(" ") }
                    spaced.append(character)
                }
                previous = character
            }
            let words = spaced.split(separator: " ").map(String.init).filter { word in
                let lower = word.lowercased()
                let isVersion = lower.drop(while: { $0 == "v" }).allSatisfy(\.isNumber)
                return !noise.contains(lower) && !isVersion
            }
            let name = words.joined(separator: " ")
                .filter { ($0.isASCII && ($0.isLetter || $0.isNumber)) || $0 == " " }
                .split(separator: " ").joined(separator: " ")
            return String(name.prefix(40)).trimmingCharacters(in: .whitespaces)
        }
        var base = clean(url.deletingPathExtension().lastPathComponent)
        if base.isEmpty { base = clean(url.deletingLastPathComponent().lastPathComponent) }
        if base.isEmpty { base = "Game" }
        let used = Set(taken.map { $0.lowercased() })
        var name = base
        var number = 2
        while used.contains(name.lowercased()) {
            name = "\(base) \(number)"
            number += 1
        }
        return name
    }

    // MARK: Catalog

    /// A store from the catalog on its way in.
    struct Getting: Equatable {
        let id: String
        /// How much of the installer has downloaded; `nil` when unknown.
        var fraction: Double?
        /// Downloaded; its Windows is being made.
        var preparing = false
    }

    enum CatalogState: Equatable {
        case available
        case downloading(Double?)
        case preparing
        /// Its installer is running.
        case installing
        case installed
    }

    /// Where `item` stands: installed once its Windows has a Start Menu
    /// shortcut. A Windows without one (an installer that was closed early)
    /// is available again, and the next try reuses it.
    func state(of item: CatalogItem) -> CatalogState {
        if let getting, getting.id == item.id {
            return getting.preparing ? .preparing : .downloading(getting.fraction)
        }
        guard bottles.contains(where: { $0.name == item.name }) else { return .available }
        if !(apps[item.name] ?? []).isEmpty { return .installed }
        return (running[item.name] ?? 0) > 0 ? .installing : .available
    }

    /// The installed store's own app (its launcher), to open it.
    func app(for item: CatalogItem) -> AppShortcut? {
        let apps = self.apps[item.name] ?? []
        return apps.first { $0.name.localizedCaseInsensitiveContains(item.name) } ?? apps.first
    }

    /// Starts installing `item` (see ``get(_:)``) unless another is.
    func beginGet(_ item: CatalogItem) {
        guard getting == nil else { return }
        getTask = Task { await get(item) }
    }

    /// Stops a download in progress.
    func cancelGet() {
        getTask?.cancel()
    }

    /// Installs a store: downloads its installer, then runs it in the
    /// store's own Windows (made now, or left by an earlier try). One at a
    /// time.
    func get(_ item: CatalogItem) async {
        guard canLaunch, getting == nil else { return }
        getting = Getting(id: item.id)
        defer { getting = nil }
        let file = installerFolder.appendingPathComponent(item.fileName)
        do {
            try await fetch(item.installer, file) { [weak self] fraction in
                Task { @MainActor in
                    if self?.getting?.id == item.id { self?.getting?.fraction = fraction }
                }
            }
            try Task.checkCancellation()
        } catch {
            if !(error is CancellationError) && (error as? URLError)?.code != .cancelled {
                errorMessage = "Couldn’t download \(item.name): \(error.localizedDescription)"
            }
            return
        }
        getting?.preparing = true
        if let existing = bottles.first(where: { $0.name == item.name }) {
            runProgram(existing, file: file)
            return
        }
        guard await createBottle(named: item.name, for: item.name),
            let bottle = bottles.first(where: { $0.name == item.name })
        else { return }
        runProgram(bottle, file: file)
    }

    /// Makes a prefix; `true` if it worked.
    @discardableResult
    func createBottle(named name: String, for installer: String? = nil) async -> Bool {
        guard let cli else { return false }
        busy = "Preparing Windows for “\(installer ?? name)”… (about 15 seconds)"
        defer { busy = nil }
        do {
            _ = try await cli.run(["prefix", "create", name])
            await refresh()
            return true
        } catch {
            report(error)
            await refresh()
            return false
        }
    }

    /// Removes a prefix and everything installed in it.
    func deleteBottle(_ bottle: Bottle) async {
        guard let cli else { return }
        busy = "Removing “\(bottle.name)”…"
        defer { busy = nil }
        do {
            _ = try await cli.run(["prefix", "remove", bottle.name])
            selection = .home
            await refresh()
        } catch {
            report(error)
        }
    }

    /// Changes one setting: `d3d9`, `dxgi`, `msync` or `metal-hud`. Changes
    /// run one at a time, in the order they were asked for, and each
    /// result updates ``bottles``; the tool also locks the settings file.
    func updateSetting(_ bottle: Bottle, _ key: String, _ value: String) async {
        let previous = settingsChange
        let name = bottle.name
        let change = Task {
            await previous?.value
            await self.applySetting(name, key, value)
        }
        settingsChange = change
        await change.value
    }

    private func applySetting(_ name: String, _ key: String, _ value: String) async {
        guard let cli else { return }
        do {
            let data = try await cli.run(["settings", name, "--\(key)", value])
            let settings = try JSON.decode(BottleSettings.self, from: data)
            if let index = bottles.firstIndex(where: { $0.name == name }) {
                bottles[index].settings = settings
            }
        } catch {
            report(error)
        }
    }

    func quitAll(_ bottle: Bottle) async {
        guard let cli else { return }
        do {
            _ = try await cli.run(["kill", bottle.name])
        } catch {
            report(error)
        }
    }

    // MARK: Programs

    /// Runs a Windows program (`.exe`, `.msi`, `.bat`) from its folder;
    /// refreshes the bottle's apps when it exits, since it may have been an
    /// installer. Its own exit status is not an error.
    func runProgram(_ bottle: Bottle, file: URL) {
        spawn(
            bottle, ["run", bottle.name, "--", file.path], what: "“\(file.lastPathComponent)”", statusIsFailure: false)
    }

    /// Opens an app's shortcut, and remembers that it was played. A game
    /// still starting is not started again (closing the splash and pressing
    /// Play would otherwise run a second copy): it is waited for once more.
    func start(_ bottle: Bottle, app: AppShortcut) {
        guard engine != nil else {
            errorMessage = engineProblem ?? "There is no usable engine. Choose one first."
            return
        }
        lastPlayed[app.id] = Date()
        defaults?.set(lastPlayed.mapValues(\.timeIntervalSince1970), forKey: Store.lastPlayedKey)
        if !isStarting(app) {
            starting[app.id] = Date()
            spawn(bottle, ["start", bottle.name, app.path], what: "“\(app.name)”", statusIsFailure: true) {
                [weak self] in
                self?.starting[app.id] = nil
            }
        }
        bringToFront(bottle, app)
    }

    /// Whether a game started from Play is still coming up: no window yet,
    /// and started within ``windowWait`` or still running. A game that
    /// showed its window once is not (Steam closed to the tray starts
    /// again, which brings its window back).
    func isStarting(_ app: AppShortcut, now: Date = Date()) -> Bool {
        guard let since = starting[app.id] else { return false }
        return now.timeIntervalSince(since) < Store.windowWait || runningGames.contains(app.id)
    }

    // MARK: Running games

    /// Looks every two seconds for running games (``runningGames``).
    func watchRunning() {
        runningWatch?.cancel()
        runningWatch = Task { [weak self] in
            while !Task.isCancelled {
                self?.updateRunning()
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    /// Updates ``runningGames`` from Wine's processes, and notes the
    /// starting games whose window came up meanwhile (a slow one may take
    /// longer than ``bringToFront(_:_:)`` waits).
    func updateRunning() {
        let processes = WineProcesses.list()
        let running = Set(
            allApps.filter { item in
                !WineProcesses.runs(item.app, prefix: item.bottle.path, in: processes).isEmpty
            }.map(\.app.id))
        let shown = Set(
            allApps.filter { item in
                starting[item.app.id] != nil && !windows(of: item.app, in: item.bottle, among: processes).isEmpty
            }.map(\.app.id))
        update(running: running, shown: shown)
    }

    /// Records the games that run, and those whose window is up.
    func update(running: Set<String>, shown: Set<String>) {
        for id in shown { starting[id] = nil }
        if running != runningGames { runningGames = running }
    }

    /// The game's processes that have windows: its own and its helpers'
    /// (see ``WineProcesses/runs(_:prefix:in:helpers:)``), or else those
    /// running under its name in the Dock (a program its shortcut does not
    /// name).
    func windows(
        of app: AppShortcut, in bottle: Bottle, among all: [WineProcesses.Process]? = nil
    ) -> [NSRunningApplication] {
        let processes = WineProcesses.runs(app, prefix: bottle.path, in: all ?? WineProcesses.list(), helpers: true)
        let own =
            processes
            .compactMap { NSRunningApplication(processIdentifier: $0.pid) }
            .filter { $0.activationPolicy == .regular }
        return own.isEmpty ? Store.runningApps(named: Store.launchName(app)) : own
    }

    /// Brings a game that already has a window to the front, instead of
    /// starting it again (a second Steam makes the running one shut down
    /// and restart, which shows nothing for a while). `true` if it did.
    func resume(_ bottle: Bottle, _ app: AppShortcut) -> Bool {
        let running = windows(of: app, in: bottle)
        guard !running.isEmpty else { return false }
        lastPlayed[app.id] = Date()
        defaults?.set(lastPlayed.mapValues(\.timeIntervalSince1970), forKey: Store.lastPlayedKey)
        Store.activate(running)
        return true
    }

    /// Puts the game's windows in front. Since macOS 14 an app comes to the
    /// front only when the app in front hands over: crossfree yields to it,
    /// then activates it from itself.
    static func activate(_ apps: [NSRunningApplication]) {
        for app in apps {
            NSApp.yieldActivation(to: app)
            app.activate(from: .current, options: [.activateAllWindows])
        }
    }

    /// The name the launcher runs a game under: its shortcut's file name.
    nonisolated static func launchName(_ app: AppShortcut) -> String {
        app.path.isEmpty ? app.name : URL(fileURLWithPath: app.path).deletingPathExtension().lastPathComponent
    }

    /// How long a started game has to show its window (seconds).
    nonisolated static let windowWait: Double = 30

    /// Brings the game's window to the front once it has one: crossfree's
    /// own window would otherwise stay in front of it. Gives up after
    /// ``windowWait``; waiting again replaces the wait before.
    func bringToFront(_ bottle: Bottle, _ app: AppShortcut) {
        shownGame = nil
        windowWaits[app.id]?.cancel()
        windowWaits[app.id] = Task {
            for _ in 0..<Int(Store.windowWait * 2) {
                if Task.isCancelled { return }
                let running = windows(of: app, in: bottle)
                if !running.isEmpty {
                    // Steam, say, is two: its client and the helper that
                    // draws its window.
                    Store.activate(running)
                    starting[app.id] = nil
                    shownGame = app.id
                    return
                }
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    /// The running, windowed processes of the game started as `name`. The
    /// launcher runs each game's processes as `<name>.app` in a mirror of
    /// the engine under crossfree's caches (so the Dock shows the game's
    /// name), which is how they are told apart.
    nonisolated static func runningApps(named name: String) -> [NSRunningApplication] {
        guard let bundle = dockName(name) else { return [] }
        return NSWorkspace.shared.runningApplications.filter { app in
            guard app.activationPolicy == .regular, let url = app.bundleURL else { return false }
            return url.lastPathComponent == "\(bundle).app" && url.path.contains("/crossfree/loaders/")
        }
    }

    /// The bundle name the launcher gives a game called `name` (its
    /// `bundle_name`): no `/` or `:`, spaces collapsed, no leading dot, at
    /// most 64 characters; `nil` for nothing, or "wine".
    nonisolated static func dockName(_ name: String) -> String? {
        let spaced = String(
            String.UnicodeScalarView(
                name.unicodeScalars.map { scalar in
                    scalar == "/" || scalar == ":" || CharacterSet.controlCharacters.contains(scalar) ? " " : scalar
                }))
        var words = spaced.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        while words.hasPrefix(".") { words.removeFirst() }
        words = words.trimmingCharacters(in: .whitespaces)
        let short = String(String.UnicodeScalarView(words.unicodeScalars.prefix(64)))
            .trimmingCharacters(in: .whitespaces)
        return short.isEmpty || short.lowercased() == "wine" ? nil : short
    }

    /// `winecfg`, `controllers` or `reboot`.
    func tool(_ bottle: Bottle, _ name: String) {
        spawn(bottle, ["tool", bottle.name, name], what: "“\(name)”", statusIsFailure: true)
    }

    func openDriveC(_ bottle: Bottle) {
        NSWorkspace.shared.open(bottle.driveC)
    }

    func logURL(_ bottle: Bottle) -> URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Logs/crossfree/\(bottle.name).log")
    }

    func openLog(_ bottle: Bottle) {
        let log = logURL(bottle)
        if FileManager.default.fileExists(atPath: log.path) {
            NSWorkspace.shared.open(log)
        } else {
            errorMessage = "Nothing has run in “\(bottle.name)” yet, so there is no log."
        }
    }

    /// Starts `args` without waiting. The program's output goes to the
    /// bottle's log; a failure to start it (see ``CLI/launchFailure``) is
    /// reported when the tool exits.
    private func spawn(
        _ bottle: Bottle, _ args: [String], what: String, statusIsFailure: Bool,
        failed: (@MainActor () -> Void)? = nil
    ) {
        guard let cli else { return }
        guard engine != nil else {
            errorMessage = engineProblem ?? "There is no usable engine. Choose one first."
            return
        }
        let name = bottle.name
        running[name, default: 0] += 1
        do {
            try cli.spawn(args, log: logURL(bottle)) { [weak self] status, output in
                Task { @MainActor in
                    guard let self else { return }
                    self.running[name] = max((self.running[name] ?? 1) - 1, 0)
                    if let failure = CLI.launchFailure(
                        what, in: name, status: status, output: output, statusIsFailure: statusIsFailure
                    ) {
                        self.errorMessage = failure
                        failed?()
                    }
                    if let bottle = self.bottles.first(where: { $0.name == name }) {
                        await self.loadApps(bottle)
                    }
                }
            }
        } catch {
            running[name] = max((running[name] ?? 1) - 1, 0)
            report(error)
            failed?()
        }
    }

    private func report(_ error: Error) {
        errorMessage = error.localizedDescription
    }
}
