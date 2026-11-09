import Foundation
import Testing

@testable import Crossfree

/// The parts of `CLI` and the models that read what `crossfree` prints.
struct CLITests {
    @Test func globalOptionsComeBeforeTheCommand() {
        let cli = CLI(executable: URL(fileURLWithPath: "/x/crossfree"), engine: URL(fileURLWithPath: "/e"))
        #expect(cli.arguments(["apps", "b"], json: true) == ["--engine", "/e", "--json", "apps", "b"])
        #expect(
            cli.arguments(["start", "b", "/p/Game.lnk"], json: false, log: URL(fileURLWithPath: "/l/b.log"))
                == ["--engine", "/e", "--log", "/l/b.log", "start", "b", "/p/Game.lnk"]
        )
    }

    @Test func ownErrorKeepsTheToolsLinesWithoutThePrefix() {
        let stderr = "wine: noise\r\ncrossfree: wineboot --init failed (exit status: 3): line one\r\nline two\n"
        #expect(CLI.ownError(stderr) == "wineboot --init failed (exit status: 3): line one\nline two")
        #expect(CLI.ownError("only wine output\n") == nil)
        #expect(CLI.ownError("crossfree: a\n1\n2\n3", maxLines: 2) == "a\n1")
    }

    @Test func messageFallsBackToTheTailThenTheStatus() {
        #expect(CLI.message("crossfree: no prefix named \"x\"\n", status: 2) == "no prefix named \"x\"")
        #expect(CLI.message("a\nb\nc\nd\n", status: 1) == "b\nc\nd")
        #expect(CLI.message("", status: 1) == "crossfree exited with status 1")
    }

    @Test func launchFailures() {
        let own = "crossfree: settings: invalid value \"dxvk\" for d3d9\n"
        #expect(
            CLI.launchFailure("“Game”", in: "b", status: 2, output: own, statusIsFailure: false)
                == "“Game” in “b” could not start: settings: invalid value \"dxvk\" for d3d9"
        )
        // A program's own exit status (from `run`) is not a failure...
        #expect(CLI.launchFailure("“setup.exe”", in: "b", status: 1602, output: "", statusIsFailure: false) == nil)
        // ...but start.exe's or a tool's is.
        #expect(
            CLI.launchFailure("“Game”", in: "b", status: 1, output: "", statusIsFailure: true)?.contains(
                "exit status 1") == true)
        #expect(CLI.launchFailure("“Game”", in: "b", status: 0, output: "", statusIsFailure: true) == nil)
    }

    /// The exact report `crossfree --json engine` prints for an unsigned loader.
    @Test func engineReportDecodesAndNamesItsProblem() throws {
        let json =
            #"{"root":"/e","version":"wine-fake-1.0","signature_valid":false,"team_id":null,"gatekeeper_source":null,"notarized":false}"#
        let info = try JSON.decode(EngineInfo.self, from: Data(json.utf8))
        #expect(info.version == "wine-fake-1.0")
        #expect(info.teamId == nil)
        #expect(info.problem == "The engine's loader has an invalid code signature.")

        let unnotarized = EngineInfo(
            root: "/e", version: "v", signatureValid: true, teamId: "T", gatekeeperSource: nil, notarized: false
        )
        #expect(unnotarized.problem == "The engine's loader is not notarized (Gatekeeper: rejected).")
        let good = EngineInfo(
            root: "/e", version: "v", signatureValid: true, teamId: "T",
            gatekeeperSource: "Notarized Developer ID", notarized: true
        )
        #expect(good.problem == nil)
    }

    @Test func listsDecode() throws {
        let bottles =
            #"[{"name":"b","path":"/p/b","settings":{"d3d9":"auto","dxgi":"dxmt","msync":true,"metal_hud":true}}]"#
        let decoded = try JSON.decode([Bottle].self, from: Data(bottles.utf8))
        #expect(decoded.map(\.name) == ["b"])
        #expect(decoded[0].settings == BottleSettings(d3d9: "auto", dxgi: "dxmt", msync: true, metalHud: true))
        let apps = #"[{"name":"Game","path":"/p/b/Game.lnk","icon":null}]"#
        #expect(try JSON.decode([AppShortcut].self, from: Data(apps.utf8)).map(\.name) == ["Game"])
    }
}
