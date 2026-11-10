import Foundation
import Testing

@testable import Crossfree

/// A fake `crossfree`: a shell script in a temporary folder that answers the
/// commands the store sends and records them. No Wine and no real bottle.
final class FakeCLI {
    let dir: URL
    let script: URL
    let record: URL
    /// Prefixes made with `prefix create`, one name per line.
    let created: URL

    /// `startFails`: `start` answers as for a missing prefix.
    init(notarized: Bool = true, startFails: Bool = true) throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("crossfree-tests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        script = dir.appendingPathComponent("crossfree")
        record = dir.appendingPathComponent("record")
        created = dir.appendingPathComponent("created")
        let text = """
            #!/bin/sh
            echo "argv $*" >> '\(record.path)'
            while [ $# -gt 0 ]; do
                case "$1" in --engine|--log) shift 2;; --json) shift;; *) break;; esac
            done
            case "$1" in
            engine) echo '{"root":"/e","version":"wine-test","signature_valid":true,"team_id":"T","gatekeeper_source":null,"notarized":\(notarized)}';;
            prefix)
                if [ "$2" = create ]; then echo "$3" >> '\(created.path)'; exit 0; fi
                printf '[{"name":"b","path":"/p/b","settings":{"d3d9":"auto","dxgi":"auto","msync":true,"metal_hud":false}}'
                if [ -f '\(created.path)' ]; then
                    while read -r name; do
                        printf ',{"name":"%s","path":"/p/%s","settings":{"d3d9":"auto","dxgi":"auto","msync":true,"metal_hud":false}}' "$name" "$name"
                    done < '\(created.path)'
                fi
                printf ']\\n';;
            apps) echo '[{"name":"Game","path":"/p/b/Game.lnk","icon":null}]';;
            settings)
                # The first change is the slowest: unserialised, it would finish last.
                case "$4" in wined3d) delay=0.4;; mtld3d) delay=0.2;; *) delay=0;; esac
                echo "begin $4" >> '\(record.path)'
                sleep $delay
                echo "end $4" >> '\(record.path)'
                printf '{"d3d9":"%s","dxgi":"auto","msync":true,"metal_hud":false}\\n' "$4";;
            start) \(startFails ? "echo 'crossfree: no prefix named \"b\"' >&2; exit 2" : "exit 0");;
            tool) exit 3;;
            run) echo "program output"; exit 7;;
            esac
            """
        try text.write(to: script, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
    }

    var cli: CLI { CLI(executable: script, engine: URL(fileURLWithPath: "/e")) }

    /// The recorded lines.
    var lines: [String] {
        ((try? String(contentsOf: record, encoding: .utf8)) ?? "").split(separator: "\n").map(String.init)
    }

    deinit {
        try? FileManager.default.removeItem(at: dir)
    }
}

/// Waits up to `seconds` for `condition`.
@MainActor
func eventually(_ seconds: Double = 5, _ condition: () -> Bool) async -> Bool {
    let deadline = Date().addingTimeInterval(seconds)
    while !condition() {
        if Date() > deadline { return false }
        try? await Task.sleep(nanoseconds: 20_000_000)
    }
    return true
}

@MainActor
struct StoreTests {
    let bottle = Bottle(
        name: "b", path: "/p/b",
        settings: BottleSettings(d3d9: "auto", dxgi: "auto", msync: true, metalHud: false)
    )
    let game = AppShortcut(name: "Game", path: "/p/b/Game.lnk", icon: nil)

    /// ⌘R reloads the engine, the bottles and their apps; a loader that is
    /// not notarized is a warning that leaves the engine usable.
    @Test func reloadAllLoadsAppsAndKeepsAnUnnotarizedEngine() async throws {
        let fake = try FakeCLI(notarized: false)
        let store = Store(cli: fake.cli)
        await store.reloadAll()
        #expect(store.engine?.version == "wine-test")
        #expect(store.engineProblem == "The engine's loader is not notarized (Gatekeeper: rejected).")
        #expect(store.canLaunch)
        #expect(store.bottleNames == ["b"])
        #expect(store.apps["b"] == [game])
        #expect(store.allApps.map { $0.app } == [game])
    }

    @Test func launchingNeedsAnEngineAndNoLongOperation() async throws {
        let fake = try FakeCLI()
        let store = Store(cli: fake.cli)
        #expect(!store.canLaunch && !store.canInstall)
        store.start(bottle, app: game)
        #expect(store.errorMessage != nil)
        #expect(!fake.lines.contains { $0.contains("start") })

        await store.refresh()
        #expect(store.canLaunch && store.canInstall && store.engineProblem == nil)
        store.busy = "Deleting"
        #expect(!store.canLaunch && !store.canInstall)
    }

    /// Settings changes run one at a time, in order, and the last asked for
    /// is what the store keeps, though the first is the slowest.
    @Test func settingsChangesAreSerialised() async throws {
        let fake = try FakeCLI()
        let store = Store(cli: fake.cli)
        store.bottles = [bottle]
        let values = ["wined3d", "mtld3d", "auto"]
        var changes: [Task<Void, Never>] = []
        for value in values {
            changes.append(Task { await store.updateSetting(bottle, "d3d9", value) })
            await Task.yield()
        }
        for change in changes { await change.value }

        let steps = fake.lines.filter { $0.hasPrefix("begin ") || $0.hasPrefix("end ") }
        #expect(steps == values.flatMap { ["begin \($0)", "end \($0)"] })
        #expect(store.bottles[0].settings.d3d9 == "auto")
        #expect(store.errorMessage == nil)
    }

    /// The tool's own error when it cannot start a program reaches the
    /// alert; so does a failed start.exe or tool; a program's own exit
    /// status does not. The program's output goes to the log (`--log`).
    @Test func launchFailuresAreReported() async throws {
        let fake = try FakeCLI()
        let store = Store(cli: fake.cli)
        await store.refresh()

        store.start(bottle, app: game)
        #expect(await eventually { store.errorMessage != nil })
        #expect(store.errorMessage == "“Game” in “b” could not start: no prefix named \"b\"")
        #expect(
            fake.lines.contains {
                $0.hasPrefix("argv --engine /e --log ") && $0.hasSuffix("b.log start b /p/b/Game.lnk")
            })
        store.errorMessage = nil

        store.tool(bottle, "winecfg")
        #expect(await eventually { store.errorMessage != nil })
        #expect(store.errorMessage?.contains("“winecfg” in “b” failed (exit status 3)") == true)
        store.errorMessage = nil

        store.runProgram(bottle, file: URL(fileURLWithPath: "/games/setup.exe"))
        #expect(
            await eventually {
                store.running["b", default: 0] == 0 && fake.lines.filter { $0.contains(" run b ") }.count == 1
            })
        try? await Task.sleep(nanoseconds: 200_000_000)
        #expect(store.errorMessage == nil)
    }

    /// Play on a game that is still starting (its window not up yet, say
    /// after the splash was closed) waits for it again rather than start a
    /// second copy; it is starting until its window comes up, or for
    /// ``Store/windowWait`` if it does not run.
    @Test func aGameStillStartingIsNotStartedTwice() async throws {
        let fake = try FakeCLI(startFails: false)
        let store = Store(cli: fake.cli)
        await store.refresh()
        let starts = { fake.lines.filter { $0.hasSuffix("start b /p/b/Game.lnk") }.count }

        store.start(bottle, app: game)
        store.start(bottle, app: game)
        #expect(await eventually { store.running["b", default: 0] == 0 && starts() == 1 })
        try? await Task.sleep(nanoseconds: 200_000_000)
        #expect(starts() == 1)
        #expect(store.isStarting(game))

        let later = Date().addingTimeInterval(Store.windowWait + 1)
        #expect(!store.isStarting(game, now: later))
        store.update(running: [game.id], shown: [])
        #expect(store.isStarting(game, now: later))

        // Its window came up: Play starts it again from now on (Steam in
        // the tray has no window, and starting it shows its window).
        store.update(running: [game.id], shown: [game.id])
        #expect(!store.isStarting(game))
        store.start(bottle, app: game)
        #expect(await eventually { starts() == 2 })
    }

    /// A start that fails is not starting any more: Play tries again.
    @Test func aFailedStartCanBeRetried() async throws {
        let fake = try FakeCLI()
        let store = Store(cli: fake.cli)
        await store.refresh()

        store.start(bottle, app: game)
        #expect(await eventually { store.errorMessage != nil })
        #expect(!store.isStarting(game))
        store.errorMessage = nil
        store.start(bottle, app: game)
        #expect(await eventually { fake.lines.filter { $0.hasSuffix("start b /p/b/Game.lnk") }.count == 2 })
    }

    /// An installer gets a Windows of its own, named after it; one for a
    /// patch runs alongside what it patches.
    @Test func installersGetTheirOwnPrefix() async throws {
        let fake = try FakeCLI()
        let store = Store(cli: fake.cli)
        await store.refresh()

        await store.install(URL(fileURLWithPath: "/Downloads/SteamSetup.exe"))
        #expect(store.bottleNames == ["b", "Steam"])
        #expect(store.busy == nil)
        #expect(await eventually { fake.lines.contains { $0.hasSuffix(" run Steam -- /Downloads/SteamSetup.exe") } })
        let create = fake.lines.firstIndex { $0.hasSuffix("prefix create Steam") }
        let run = fake.lines.firstIndex { $0.hasSuffix(" run Steam -- /Downloads/SteamSetup.exe") }
        #expect(create != nil && run != nil && create! < run!)

        // The same installer again gets a second, separate Windows.
        await store.install(URL(fileURLWithPath: "/Downloads/SteamSetup.exe"))
        #expect(store.bottleNames == ["b", "Steam", "Steam 2"])

        // A patch goes alongside: no new prefix.
        await store.install(URL(fileURLWithPath: "/Downloads/patch.exe"), alongside: bottle)
        #expect(await eventually { fake.lines.contains { $0.hasSuffix(" run b -- /Downloads/patch.exe") } })
        #expect(!fake.lines.contains { $0.hasSuffix("prefix create patch") })
    }
}
