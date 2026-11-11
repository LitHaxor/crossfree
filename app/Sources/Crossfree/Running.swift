import AppKit
import Darwin

/// The Windows programs running in crossfree's installs. Wine names each
/// process after the Windows program it runs (`C:\…\steam.exe`), and every
/// process carries the `WINEPREFIX` it was started in, so a game is running
/// when one of its install's processes runs its program, or runs from its
/// folder (a Steam game Steam started).
enum WineProcesses {
    struct Process: Equatable {
        let pid: pid_t
        /// The Windows program (`C:\…\game.exe`).
        let program: String
        /// The install's folder (`WINEPREFIX`).
        let prefix: String
    }

    /// Wine processes of crossfree's engine: the apps macOS knows whose
    /// bundle is a Wine loader (`…/lib/wine/aarch64-unix/<name>.app`, the
    /// engine's or a mirror's). Games have windows, so macOS knows them.
    static func list() -> [Process] {
        NSWorkspace.shared.runningApplications.compactMap { app in
            guard let bundle = app.bundleURL?.path, bundle.contains("/lib/wine/aarch64-unix/") else { return nil }
            return process(app.processIdentifier)
        }
    }

    /// `app`'s processes among `processes`, `app` being installed in the
    /// prefix at `prefix`: those of the prefix that run its program, or run
    /// from its install folder (a Steam game Steam started). With `helpers`,
    /// also those from its program's folder (Steam's window belongs to
    /// `bin\cef\…\steamwebhelper.exe`), unless that is Windows' own.
    /// Windows paths compare without regard to case.
    static func runs(_ app: AppShortcut, prefix: String, in processes: [Process], helpers: Bool = false) -> [Process] {
        let prefix = normalized(prefix)
        let program = app.program?.lowercased()
        var folders = [app.installDir].compactMap { $0.map(folder) }
        if helpers, let program, let cut = program.lastIndex(of: "\\") {
            let home = String(program[..<cut]) + "\\"
            if !home.hasPrefix("c:\\windows\\") { folders.append(home) }
        }
        guard program != nil || !folders.isEmpty else { return [] }
        return processes.filter { process in
            guard normalized(process.prefix) == prefix else { return false }
            let running = process.program.lowercased()
            return running == program || folders.contains { running.hasPrefix($0) }
        }
    }

    /// A Windows folder as a prefix to compare paths with: lower case, one
    /// `\` at the end.
    private static func folder(_ path: String) -> String {
        path.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "\\")) + "\\"
    }

    static func normalized(_ path: String) -> String {
        URL(fileURLWithPath: path).standardizedFileURL.path
    }

    /// The process's first argument (where Wine puts the Windows program)
    /// and its `WINEPREFIX`, from the kernel's copy of its arguments and
    /// environment (`KERN_PROCARGS2`: argument count, executable path,
    /// then NUL-separated arguments and environment).
    static func process(_ pid: pid_t) -> Process? {
        var mib: [Int32] = [CTL_KERN, KERN_PROCARGS2, pid]
        var size = 0
        guard sysctl(&mib, 3, nil, &size, nil, 0) == 0, size > 8 else { return nil }
        var buffer = [UInt8](repeating: 0, count: size)
        guard sysctl(&mib, 3, &buffer, &size, nil, 0) == 0 else { return nil }
        return parse(Array(buffer.prefix(size)), pid: pid)
    }

    /// Reads `KERN_PROCARGS2` data. Wine rewrites the argument area in place
    /// (the Windows program first, the rest blank), so the environment is
    /// found by its `WINEPREFIX=` entry rather than by counting.
    static func parse(_ bytes: [UInt8], pid: pid_t) -> Process? {
        guard bytes.count > 4 else { return nil }
        var strings: [String] = []
        var start = 4
        for index in 4..<bytes.count where bytes[index] == 0 {
            if index > start { strings.append(String(decoding: bytes[start..<index], as: UTF8.self)) }
            start = index + 1
        }
        // strings[0] is the executable's path; then the first argument.
        guard strings.count > 2,
            let prefix = strings.dropFirst(2).first(where: { $0.hasPrefix("WINEPREFIX=") })?.dropFirst(
                "WINEPREFIX=".count)
        else { return nil }
        let program = strings[1].trimmingCharacters(in: .whitespaces)
        guard program.count > 3, program.dropFirst().hasPrefix(":\\") else { return nil }
        return Process(pid: pid, program: program, prefix: String(prefix))
    }
}

/// A Steam that came up only because Play started a Steam game in an
/// install where Steam was not running. Quitting the game ends it too, as
/// on a console: otherwise Steam stays in the Dock under the game's name
/// (the launcher ran it as the game). A Steam the player had open stays.
struct SteamSession: Equatable {
    enum Step: Equatable {
        case wait
        /// Ask Steam (its program, `C:\…\steam.exe`) to quit.
        case quit(String)
        /// Steam is gone: nothing left to do.
        case done
    }

    /// The install (`WINEPREFIX`).
    let prefix: String
    /// When a Steam game, and Steam, last ran.
    private(set) var lastGame: Date?
    private(set) var lastSteam: Date?

    /// How long nothing may run before it counts as ended: a game's own
    /// launcher can exit before the game shows up, and Steam restarts
    /// itself when it updates.
    static let grace: TimeInterval = 10

    init(prefix: String) {
        self.prefix = prefix
    }

    /// What to do now that `processes` run. Steam's games run from a
    /// `steamapps` folder; any of them keeps Steam (another one started
    /// from Steam meanwhile, say).
    mutating func step(_ processes: [WineProcesses.Process], now: Date) -> Step {
        let own = Self.inPrefix(prefix, processes)
        let steam = Self.steam(in: own, prefix: prefix)
        if own.contains(where: { $0.program.lowercased().contains(#"\steamapps\"#) }) { lastGame = now }
        if steam != nil { lastSteam = now }
        let quiet = { (since: Date?) in since.map { now.timeIntervalSince($0) >= Self.grace } ?? false }
        if let steam {
            return quiet(lastGame) ? .quit(steam.program) : .wait
        }
        return quiet([lastGame, lastSteam].compactMap { $0 }.max()) ? .done : .wait
    }

    /// Steam's client (`steam.exe`) among the processes of the install at
    /// `prefix`.
    static func steam(in processes: [WineProcesses.Process], prefix: String) -> WineProcesses.Process? {
        inPrefix(prefix, processes).first { $0.program.lowercased().hasSuffix(#"\steam.exe"#) }
    }

    private static func inPrefix(_ prefix: String, _ processes: [WineProcesses.Process]) -> [WineProcesses.Process] {
        let prefix = WineProcesses.normalized(prefix)
        return processes.filter { WineProcesses.normalized($0.prefix) == prefix }
    }
}
