import Foundation

/// A store or game launcher crossfree installs in one step: its publisher's
/// own installer, downloaded and run in a Windows of its own. Games bought
/// in it install inside it, and show up in Games.
struct CatalogItem: Identifiable, Equatable, Sendable {
    let id: String
    /// Its name, which is also the name of the Windows (prefix) it gets.
    let name: String
    let maker: String
    let summary: String
    /// The publisher's download link for its current installer.
    let installer: URL
    /// What the installer is saved as; its type decides how it runs.
    let fileName: String
    /// The brand's colours, top and bottom, as 0xRRGGBB.
    let top: UInt32
    let bottom: UInt32
}

enum Catalog {
    /// The stores, most used first. Every link is the publisher's own and
    /// always serves the current installer.
    static let items: [CatalogItem] = [
        CatalogItem(
            id: "steam", name: "Steam", maker: "Valve",
            summary: "Valve’s PC game store, with your Steam library, friends and Workshop.",
            installer: URL(string: "https://cdn.akamai.steamstatic.com/client/installer/SteamSetup.exe")!,
            fileName: "SteamSetup.exe", top: 0x2A475E, bottom: 0x111823
        ),
        CatalogItem(
            id: "epic", name: "Epic Games", maker: "Epic Games",
            summary: "The Epic Games Store and your Epic library.",
            installer: URL(
                string:
                    "https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/installer/download/EpicGamesLauncherInstaller.msi"
            )!,
            fileName: "EpicGamesLauncherInstaller.msi", top: 0x3A3A3A, bottom: 0x0E0E0E
        ),
        CatalogItem(
            id: "gog", name: "GOG Galaxy", maker: "GOG",
            summary: "GOG’s DRM-free store, and the launcher for your GOG games.",
            installer: URL(string: "https://webinstallers.gog-statics.com/download/GOG_Galaxy_2.0.exe")!,
            fileName: "GOG_Galaxy_2.0.exe", top: 0x8E3B92, bottom: 0x2A0E30
        ),
        CatalogItem(
            id: "battlenet", name: "Battle.net", maker: "Blizzard",
            summary: "Blizzard’s launcher: World of Warcraft, Diablo, Overwatch and more.",
            installer: URL(
                string:
                    "https://downloader.battle.net/download/getInstallerForGame?os=win&gameProgram=BATTLENET_APP&version=Live"
            )!,
            fileName: "Battle.net-Setup.exe", top: 0x148EFF, bottom: 0x082A5C
        ),
        CatalogItem(
            id: "ea", name: "EA app", maker: "Electronic Arts",
            summary: "EA’s launcher for games such as The Sims and Battlefield.",
            installer: URL(
                string: "https://origin-a.akamaihd.net/EA-Desktop-Client-Download/installer-releases/EAappInstaller.exe"
            )!,
            fileName: "EAappInstaller.exe", top: 0xFF4747, bottom: 0x5C0A16
        ),
        CatalogItem(
            id: "ubisoft", name: "Ubisoft Connect", maker: "Ubisoft",
            summary: "Ubisoft’s launcher for Assassin’s Creed, Far Cry and more.",
            installer: URL(string: "https://static3.cdn.ubi.com/orbit/launcher_installer/UbisoftConnectInstaller.exe")!,
            fileName: "UbisoftConnectInstaller.exe", top: 0x2F6BFF, bottom: 0x0A1640
        ),
        CatalogItem(
            id: "rockstar", name: "Rockstar Games", maker: "Rockstar Games",
            summary: "Rockstar’s launcher for Grand Theft Auto and Red Dead Redemption.",
            installer: URL(
                string: "https://gamedownloads.rockstargames.com/public/installer/Rockstar-Games-Launcher.exe")!,
            fileName: "Rockstar-Games-Launcher.exe", top: 0xE8920C, bottom: 0x4A2300
        ),
    ]
}

/// Downloads installers, reporting progress.
enum Downloader {
    struct Failed: LocalizedError {
        let status: Int
        var errorDescription: String? { "The download server answered with status \(status)." }
    }

    /// Downloads `url` to `destination`, replacing it. `progress` gets the
    /// fraction done (`nil` while the size is unknown), once per percent.
    /// Cancelling the calling task cancels the download.
    static func fetch(_ url: URL, to destination: URL, progress: @escaping @Sendable (Double?) -> Void) async throws {
        try FileManager.default.createDirectory(
            at: destination.deletingLastPathComponent(), withIntermediateDirectories: true
        )
        let download = Download(progress: progress)
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                // The temporary file is gone once the handler returns, so
                // it is moved into place here.
                let task = URLSession.shared.downloadTask(with: url) { temporary, response, error in
                    download.finish()
                    done.resume(with: Result { try finish(temporary, response, error, destination) })
                }
                download.start(task)
            }
        } onCancel: {
            download.cancel()
        }
    }

    private static func finish(_ temporary: URL?, _ response: URLResponse?, _ error: Error?, _ destination: URL) throws
    {
        if let error { throw error }
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            throw Failed(status: http.statusCode)
        }
        guard let temporary else { throw URLError(.cannotCreateFile) }
        try? FileManager.default.removeItem(at: destination)
        try FileManager.default.moveItem(at: temporary, to: destination)
    }
}

/// One download's task and progress, shared with the cancellation handler.
/// Progress is read from the task's byte counts four times a second: the
/// task's `Progress` stops sending change notifications part way through
/// some downloads (Epic's installer stuck at 5%, then jumped to 100%).
private final class Download: @unchecked Sendable {
    private let lock = NSLock()
    private let progress: @Sendable (Double?) -> Void
    private var task: URLSessionDownloadTask?
    private var timer: DispatchSourceTimer?
    private var cancelled = false
    /// The last percent reported; -1 for "size unknown".
    private var reported: Int?

    init(progress: @escaping @Sendable (Double?) -> Void) {
        self.progress = progress
    }

    func start(_ task: URLSessionDownloadTask) {
        let timer = DispatchSource.makeTimerSource(queue: .global(qos: .utility))
        timer.schedule(deadline: .now(), repeating: .milliseconds(250))
        timer.setEventHandler { [weak self] in self?.poll() }
        lock.lock()
        self.task = task
        self.timer = timer
        let cancelled = cancelled
        lock.unlock()
        timer.resume()
        task.resume()
        if cancelled { task.cancel() }
    }

    /// Stops reporting; the download is over.
    func finish() {
        lock.lock()
        let timer = timer
        self.timer = nil
        lock.unlock()
        timer?.cancel()
    }

    func cancel() {
        lock.lock()
        cancelled = true
        let task = task
        lock.unlock()
        task?.cancel()
    }

    private func poll() {
        lock.lock()
        guard let task, timer != nil else {
            lock.unlock()
            return
        }
        let received = task.countOfBytesReceived
        let expected = task.countOfBytesExpectedToReceive
        let fraction = expected > 0 ? min(Double(received) / Double(expected), 1) : nil
        let percent = fraction.map { Int($0 * 100) } ?? -1
        let changed = percent != reported
        reported = percent
        lock.unlock()
        if changed { progress(fraction) }
    }
}
