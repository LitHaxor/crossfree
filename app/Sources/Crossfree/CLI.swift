import Foundation

/// An error from the `crossfree` command-line tool, with its message.
struct CLIError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// Runs the bundled `crossfree` command-line tool. All launcher logic lives
/// there (Rust, tested); the app only asks it for JSON and starts programs.
struct CLI: Sendable {
    var executable: URL
    var engine: URL?

    /// `$CROSSFREE_CLI`, or the copy inside the app bundle.
    static func locate() -> URL? {
        if let path = ProcessInfo.processInfo.environment["CROSSFREE_CLI"] {
            return URL(fileURLWithPath: path)
        }
        return Bundle.main.url(forResource: "crossfree", withExtension: nil)
    }

    /// The global options (`--engine`, `--log`, `--json`), which the tool
    /// reads only before the command, then `args`.
    func arguments(_ args: [String], json: Bool, log: URL? = nil) -> [String] {
        var all: [String] = []
        if let engine { all += ["--engine", engine.path] }
        if let log { all += ["--log", log.path] }
        if json { all.append("--json") }
        return all + args
    }

    /// Runs a command to completion and returns its standard output.
    ///
    /// Output goes through temporary files rather than pipes: Wine can write
    /// far more to stderr than a pipe buffer holds, which would deadlock.
    func run(_ args: [String], json: Bool = true) async throws -> Data {
        let executable = executable
        let all = arguments(args, json: json)
        return try await Task.detached(priority: .userInitiated) {
            let files = FileManager.default
            let base = files.temporaryDirectory.appendingPathComponent("crossfree-\(UUID().uuidString)")
            let outURL = base.appendingPathExtension("out")
            let errURL = base.appendingPathExtension("err")
            files.createFile(atPath: outURL.path, contents: nil)
            files.createFile(atPath: errURL.path, contents: nil)
            defer {
                try? files.removeItem(at: outURL)
                try? files.removeItem(at: errURL)
            }

            let process = Process()
            process.executableURL = executable
            process.arguments = all
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = try FileHandle(forWritingTo: outURL)
            process.standardError = try FileHandle(forWritingTo: errURL)
            try process.run()
            process.waitUntilExit()

            let output = (try? Data(contentsOf: outURL)) ?? Data()
            guard process.terminationStatus == 0 else {
                let stderr = (try? Data(contentsOf: errURL)).map { String(decoding: $0, as: UTF8.self) } ?? ""
                throw CLIError(message: Self.message(stderr, status: process.terminationStatus))
            }
            return output
        }.value
    }

    /// Starts a long-running command (a Windows program) without waiting.
    ///
    /// The tool appends the program's output to `log` itself (`--log`, in
    /// append mode, so concurrent launches do not overwrite each other). Its
    /// own output, which is only its error when it cannot start the program,
    /// goes to a temporary file that `onExit` receives with the exit status
    /// once the command ends.
    func spawn(
        _ args: [String],
        log: URL,
        onExit: @escaping @Sendable (_ status: Int32, _ output: String) -> Void
    ) throws {
        let files = FileManager.default
        let ownURL = files.temporaryDirectory.appendingPathComponent("crossfree-\(UUID().uuidString).spawn")
        files.createFile(atPath: ownURL.path, contents: nil)
        let own: FileHandle
        do {
            own = try FileHandle(forWritingTo: ownURL)
        } catch {
            try? files.removeItem(at: ownURL)
            throw error
        }

        let process = Process()
        process.executableURL = executable
        process.arguments = arguments(args, json: false, log: log)
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = own
        process.standardError = own
        process.terminationHandler = { finished in
            try? own.close()
            let output = (try? Data(contentsOf: ownURL)).map { String(decoding: $0, as: UTF8.self) } ?? ""
            try? FileManager.default.removeItem(at: ownURL)
            onExit(finished.terminationStatus, output)
        }
        do {
            try process.run()
        } catch {
            try? own.close()
            try? files.removeItem(at: ownURL)
            throw error
        }
    }

    /// The tool's own error (its `crossfree: ...` line and the lines after
    /// it, at most `maxLines` in all, without the prefix), or `nil` if the
    /// output holds none.
    static func ownError(_ output: String, maxLines: Int = 8) -> String? {
        let prefix = "crossfree: "
        let lines = output.split(whereSeparator: \.isNewline)
        guard let first = lines.firstIndex(where: { $0.hasPrefix(prefix) }) else { return nil }
        var own = lines[first...].prefix(maxLines).map(String.init)
        own[0] = String(own[0].dropFirst(prefix.count))
        return own.joined(separator: "\n")
    }

    /// The tool's own error, else the last lines of its output, else its
    /// exit status.
    static func message(_ stderr: String, status: Int32) -> String {
        if let own = ownError(stderr) { return own }
        let tail = stderr.split(whereSeparator: \.isNewline).suffix(3).joined(separator: "\n")
        return tail.isEmpty ? "crossfree exited with status \(status)" : tail
    }

    /// The message for a launch (`start`, `run` or `tool`) that ended with
    /// `status` and the tool's own `output`, or `nil` when it did not fail.
    ///
    /// The tool's own error always counts. A nonzero status without one
    /// counts only when `statusIsFailure`: it does for `start` and the
    /// tools, whose status is `start.exe`'s or the tool's, but not for a
    /// program from `run`, whose status is its own (installers exit nonzero
    /// when cancelled, for example).
    static func launchFailure(
        _ what: String, in bottle: String, status: Int32, output: String, statusIsFailure: Bool
    ) -> String? {
        if let own = ownError(output) {
            return "\(what) in “\(bottle)” could not start: \(own)"
        }
        if status != 0 && statusIsFailure {
            return "\(what) in “\(bottle)” failed (exit status \(status)). Open Log may say why."
        }
        return nil
    }
}
