import Foundation
import Testing

@testable import Crossfree

/// Telling which games run from Wine's processes.
struct RunningTests {
    /// `KERN_PROCARGS2` bytes: argument count, executable path, padding,
    /// arguments, environment, as the kernel lays them out.
    func procargs(executable: String, arguments: [String], environment: [String]) -> [UInt8] {
        var bytes = withUnsafeBytes(of: Int32(arguments.count).littleEndian, Array.init)
        bytes += Array(executable.utf8) + [0, 0, 0]
        for string in arguments + environment { bytes += Array(string.utf8) + [0] }
        bytes += [0] + Array("executable_path=/x".utf8) + [0]
        return bytes
    }

    /// Wine puts the Windows program first and blanks the rest of the
    /// arguments; the install comes from `WINEPREFIX`.
    @Test func readsWinesProcesses() {
        let bytes = procargs(
            executable: "/c/loaders/k/Steam/lib/wine/aarch64-unix/Steam.app/Contents/MacOS/wine",
            arguments: [#"C:\Program Files (x86)\Steam\steam.exe "#, "", ""],
            environment: ["HOME=/Users/x", "WINEPREFIX=/p/Steam", "WINEDEBUG=-all"]
        )
        let process = WineProcesses.parse(bytes, pid: 42)
        #expect(
            process
                == WineProcesses.Process(
                    pid: 42, program: #"C:\Program Files (x86)\Steam\steam.exe"#, prefix: "/p/Steam"))
        // Not a Wine program, or no install: not a game's process.
        #expect(
            WineProcesses.parse(
                procargs(executable: "/bin/zsh", arguments: ["-zsh"], environment: ["WINEPREFIX=/p"]), pid: 1) == nil)
        #expect(
            WineProcesses.parse(procargs(executable: "/w", arguments: [#"C:\a.exe"#], environment: ["HOME=/"]), pid: 1)
                == nil)
        #expect(WineProcesses.parse([1, 0], pid: 1) == nil)
    }

    @Test func matchesGamesToProcesses() {
        let steam = AppShortcut(
            name: "Steam", path: "/p/Steam.lnk", icon: nil, program: #"C:\Program Files (x86)\Steam\steam.exe"#)
        let game = AppShortcut(
            name: "Rise of Nations", path: "/p/RoN.url", icon: nil, steamAppId: 287450,
            installDir: #"c:\program files (x86)\steam\steamapps\common\Rise of Nations"#
        )
        let other = AppShortcut(name: "Tool", path: "/p/Tool.lnk", icon: nil, program: #"C:\windows\system32\cmd.exe"#)
        let processes = [
            WineProcesses.Process(pid: 1, program: #"C:\Program Files (x86)\Steam\steam.exe"#, prefix: "/p/Steam/"),
            WineProcesses.Process(
                pid: 2, program: #"C:\Program Files (x86)\Steam\bin\cef\cef.win64\steamwebhelper.exe"#,
                prefix: "/p/Steam"),
            WineProcesses.Process(
                pid: 3, program: #"C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations\riseofnations.exe"#,
                prefix: "/p/Steam"),
            WineProcesses.Process(pid: 4, program: #"C:\windows\system32\explorer.exe"#, prefix: "/p/Steam"),
            // The same Steam in another install (CrossOver's, say).
            WineProcesses.Process(pid: 5, program: #"C:\Program Files (x86)\Steam\steam.exe"#, prefix: "/elsewhere"),
        ]
        let pids = { (app: AppShortcut, helpers: Bool) in
            WineProcesses.runs(app, prefix: "/p/Steam", in: processes, helpers: helpers).map(\.pid)
        }
        #expect(pids(steam, false) == [1])
        #expect(pids(game, false) == [3])
        #expect(pids(other, false) == [])
        // Helpers: whatever runs from the program's folder, not Windows'.
        #expect(pids(steam, true) == [1, 2, 3])
        #expect(pids(other, true) == [])
        #expect(WineProcesses.runs(steam, prefix: "/elsewhere", in: processes).map(\.pid) == [5])
        #expect(
            WineProcesses.runs(AppShortcut(name: "x", path: "/p/x.lnk", icon: nil), prefix: "/p/Steam", in: processes)
                .isEmpty)
    }

    /// A Steam that Play brought up quits once no Steam game has run for
    /// ``SteamSession/grace``; a short gap (a launcher handing over to its
    /// game) is not the end.
    @Test func steamQuitsWhenItsGamesHaveEnded() {
        let steam = WineProcesses.Process(pid: 1, program: #"C:\Program Files (x86)\Steam\steam.exe"#, prefix: "/p/b/")
        let game = WineProcesses.Process(
            pid: 2, program: #"C:\Program Files (x86)\Steam\steamapps\common\Some Game\Game.exe"#, prefix: "/p/b")
        let elsewhere = WineProcesses.Process(
            pid: 3, program: #"D:\SteamLibrary\steamapps\common\Other\Other.exe"#, prefix: "/elsewhere")
        let start = Date()
        let at = { (seconds: Double) in start.addingTimeInterval(seconds) }
        var session = SteamSession(prefix: "/p/b")

        // Steam starting, updating or showing its login: no game yet.
        #expect(session.step([], now: at(0)) == .wait)
        #expect(session.step([steam], now: at(60)) == .wait)
        #expect(session.step([steam, game], now: at(61)) == .wait)
        #expect(session.step([steam], now: at(65)) == .wait)
        #expect(session.step([steam, game], now: at(66)) == .wait)
        // Another install's game does not keep this Steam.
        #expect(session.step([steam, elsewhere], now: at(66 + SteamSession.grace)) == .quit(steam.program))
    }

    /// Steam gone (closed by the player) ends the session without a quit;
    /// gone only for a restart (an update) does not.
    @Test func steamGoneOnItsOwnNeedsNoQuit() {
        let steam = WineProcesses.Process(pid: 1, program: #"C:\Steam\Steam.exe"#, prefix: "/p/b")
        let start = Date()
        var session = SteamSession(prefix: "/p/b")
        #expect(session.step([steam], now: start) == .wait)
        #expect(session.step([], now: start.addingTimeInterval(3)) == .wait)
        #expect(session.step([steam], now: start.addingTimeInterval(6)) == .wait)
        #expect(session.step([], now: start.addingTimeInterval(6 + SteamSession.grace)) == .done)
        #expect(SteamSession.steam(in: [steam], prefix: "/p/b") == steam)
        #expect(SteamSession.steam(in: [steam], prefix: "/p/other") == nil)
    }
}
