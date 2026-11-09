import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// A panel over the console: the screen dims and blurs, the card springs in.
struct PanelHost: View {
    @EnvironmentObject private var store: Store
    @ObservedObject var nav: ConsoleNav
    let panel: ConsoleNav.Panel
    let play: (Bottle, AppShortcut) -> Void
    @StateObject private var shown = Local(false)

    var body: some View {
        GeometryReader { proxy in
            ZStack {
                Rectangle()
                    .fill(.ultraThinMaterial)
                    .overlay(Color.black.opacity(0.45))
                    .onTapGesture(perform: close)
                ViewThatFits(in: .vertical) {
                    card
                    ScrollView(.vertical, showsIndicators: false) { card }
                }
                .frame(width: width)
                .frame(maxHeight: proxy.size.height - 110)
                .glass(radius: 30, tint: Theme.baseLift.opacity(0.82))
                .shadow(color: .black.opacity(0.6), radius: 50, y: 24)
                .scaleEffect(shown.value ? 1 : 0.92)
                .opacity(shown.value ? 1 : 0)
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
        }
        .ignoresSafeArea()
        .onAppear { withAnimation(Theme.present) { shown.value = true } }
    }

    private var width: CGFloat {
        if case .gameOptions = panel { return 780 }
        return 580
    }

    private var card: some View {
        content
            .padding(34)
            .frame(width: width, alignment: .leading)
    }

    @ViewBuilder
    private var content: some View {
        switch panel {
        case .install(let alongside):
            InstallPanel(alongside: alongside, close: close)
        case .problem:
            ProblemPanel(close: close)
        case .uninstall(let name):
            UninstallPanel(name: name, close: close)
        case .gameOptions(let id):
            if let item = store.allApps.first(where: { $0.app.id == id }) {
                GameOptionsPanel(bottle: item.bottle, app: item.app, nav: nav, play: play, close: close)
            }
        }
    }

    private func close() {
        withAnimation(Theme.present) { nav.panel = nil }
    }
}

private struct PanelTitle: View {
    let title: String
    let detail: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.system(size: 30, weight: .light)).foregroundStyle(Theme.text)
                .fixedSize(horizontal: false, vertical: true)
            Text(detail).font(.system(size: 15)).foregroundStyle(Theme.secondaryText)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

// MARK: - Add a game

private struct InstallPanel: View {
    @EnvironmentObject private var store: Store
    let alongside: String?
    let close: () -> Void
    /// The install to add to, or "" for one of its own.
    @StateObject private var target = Local("")

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            PanelTitle(
                title: "Add a Game",
                detail:
                    "Choose an installer, such as SteamSetup.exe or a game's setup.exe. It gets a Windows of its own, so it cannot break your other games. What it installs appears in Games."
            )
            if !store.bottles.isEmpty {
                VStack(alignment: .leading, spacing: 10) {
                    ScrollView(.horizontal, showsIndicators: false) {
                        ConsoleChoice(
                            title: "Install",
                            options: [(value: "", label: "On its own")]
                                + store.bottles.map { (value: $0.name, label: "With \($0.name)") },
                            selection: $target.value
                        )
                    }
                    Text("Choose “With…” for a patch, DLC or mod of something you have installed.")
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.tertiaryText)
                }
            }
            HStack(spacing: 12) {
                Spacer()
                Button("Cancel", action: close)
                    .buttonStyle(ConsoleButtonStyle(kind: .secondary, height: 44))
                    .keyboardShortcut(.cancelAction)
                Button(action: pick) { Label("Choose Installer…", systemImage: "folder") }
                    .buttonStyle(ConsoleButtonStyle(kind: .primary, height: 44))
                    .keyboardShortcut(.defaultAction)
                    .disabled(!store.canLaunch)
            }
        }
        .onAppear { target.value = alongside ?? "" }
    }

    private func pick() {
        let existing = store.bottles.first { $0.name == target.value }
        let panel = NSOpenPanel()
        panel.message = existing.map { "Choose an installer to run with \($0.name)." } ?? "Choose an installer."
        panel.allowedContentTypes = ["exe", "msi", "bat"].compactMap { UTType(filenameExtension: $0) }
        panel.canChooseDirectories = false
        if panel.runModal() == .OK, let url = panel.url {
            close()
            Task { await store.install(url, alongside: existing) }
        }
    }
}

// MARK: - Problem

/// Nothing can run: the engine inside crossfree is missing or broken. The
/// only place the engine is ever mentioned.
private struct ProblemPanel: View {
    @EnvironmentObject private var store: Store
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            PanelTitle(
                title: "Games can’t start",
                detail: "crossfree’s Windows engine didn’t work. Reinstalling crossfree usually fixes this."
            )
            if let problem = store.setupProblem {
                Text(problem)
                    .font(.system(size: 13).monospaced())
                    .foregroundStyle(Theme.secondaryText)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(14)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.black.opacity(0.25), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            HStack(spacing: 12) {
                Button("Use Another Engine…") { store.pickEngine() }
                    .buttonStyle(ConsoleButtonStyle(kind: .secondary, height: 44))
                Spacer()
                Button("Close", action: close)
                    .buttonStyle(ConsoleButtonStyle(kind: .secondary, height: 44))
                    .keyboardShortcut(.cancelAction)
                Button("Try Again") {
                    close()
                    Task { await store.reloadAll() }
                }
                .buttonStyle(ConsoleButtonStyle(kind: .primary, height: 44))
                .keyboardShortcut(.defaultAction)
            }
        }
    }
}

// MARK: - Uninstall

private struct UninstallPanel: View {
    @EnvironmentObject private var store: Store
    let name: String
    let close: () -> Void

    var body: some View {
        let apps = store.apps[name] ?? []
        VStack(alignment: .leading, spacing: 24) {
            PanelTitle(title: title(apps), detail: detail(apps))
            if !apps.isEmpty {
                HStack(spacing: 12) {
                    ForEach(apps.prefix(6)) { app in
                        GameArt(app: app).frame(width: 64, height: 96)
                    }
                }
            }
            HStack(spacing: 12) {
                Spacer()
                Button("Cancel", action: close)
                    .buttonStyle(ConsoleButtonStyle(kind: .secondary, height: 44))
                    .keyboardShortcut(.cancelAction)
                Button(action: uninstall) { Label("Uninstall", systemImage: "trash") }
                    .buttonStyle(ConsoleButtonStyle(kind: .danger, height: 44))
                    .disabled(store.busy != nil)
            }
        }
    }

    private func title(_ apps: [AppShortcut]) -> String {
        apps.count == 1 ? "Uninstall “\(apps[0].name)”?" : "Uninstall \(name)?"
    }

    private func detail(_ apps: [AppShortcut]) -> String {
        let what: String
        switch apps.count {
        case 0: what = "everything installed with \(name)"
        case 1: what = apps[0].name
        default: what = apps.map(\.name).joined(separator: ", ")
        }
        return "This removes \(what), saved games and settings included. It cannot be undone."
    }

    private func uninstall() {
        guard let bottle = store.bottles.first(where: { $0.name == name }) else { return close() }
        close()
        Task { await store.deleteBottle(bottle) }
    }
}

// MARK: - Game options

/// Everything about one game: play, graphics, its files, Windows tools,
/// uninstalling. Graphics and tools belong to the game's Windows, which it
/// may share with others (a store and the games it installed).
private struct GameOptionsPanel: View {
    @EnvironmentObject private var store: Store
    let bottle: Bottle
    let app: AppShortcut
    @ObservedObject var nav: ConsoleNav
    let play: (Bottle, AppShortcut) -> Void
    let close: () -> Void

    private var neighbours: [AppShortcut] {
        (store.apps[bottle.name] ?? []).filter { $0.id != app.id }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 28) {
            HStack(alignment: .top, spacing: 24) {
                GameArt(app: app)
                    .frame(width: 112, height: 168)
                    .shadow(color: .black.opacity(0.5), radius: 16, y: 8)
                VStack(alignment: .leading, spacing: 14) {
                    Text(app.name)
                        .font(.system(size: 32, weight: .light))
                        .foregroundStyle(Theme.text)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(sharing)
                        .font(.system(size: 14))
                        .foregroundStyle(Theme.secondaryText)
                        .fixedSize(horizontal: false, vertical: true)
                    Button {
                        play(bottle, app)
                    } label: {
                        Label("Play", systemImage: "play.fill").frame(minWidth: 110)
                    }
                    .buttonStyle(ConsoleButtonStyle(kind: .primary, height: 46))
                    .disabled(!store.canLaunch)
                }
            }

            VStack(alignment: .leading, spacing: 16) {
                SectionTitle("Graphics")
                HStack(alignment: .top, spacing: 24) {
                    ConsoleChoice(
                        title: "Direct3D 9",
                        options: [("auto", "Auto"), ("mtld3d", "Metal"), ("wined3d", "wined3d")],
                        selection: setting(\.d3d9, key: "d3d9")
                    )
                    ConsoleChoice(
                        title: "Direct3D 10–12",
                        options: [("auto", "Auto"), ("dxmt", "DXMT"), ("wined3d", "wined3d")],
                        selection: setting(\.dxgi, key: "dxgi")
                    )
                }
                HStack(spacing: 14) {
                    ConsoleToggle(
                        title: "MSync", detail: "Faster sync; applies on next start.",
                        isOn: toggle(\.msync, key: "msync"))
                    ConsoleToggle(
                        title: "Metal HUD", detail: "Frame-rate overlay.", isOn: toggle(\.metalHud, key: "metal-hud"))
                }
            }
            .disabled(store.busy != nil)

            VStack(alignment: .leading, spacing: 16) {
                SectionTitle("Tools")
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 12), count: 4), spacing: 12) {
                    ActionTile(title: "Files", symbol: "folder") { store.openDriveC(bottle) }
                    ActionTile(title: "Run Program…", symbol: "play.rectangle") { runProgram() }
                        .disabled(!store.canLaunch)
                    ActionTile(title: "Add Patch…", symbol: "plus.square.on.square") {
                        withAnimation(Theme.present) { nav.panel = .install(alongside: bottle.name) }
                    }
                    .disabled(!store.canLaunch)
                    ActionTile(title: "Log", symbol: "doc.text.magnifyingglass") { store.openLog(bottle) }
                    ActionTile(title: "Wine Settings", symbol: "gearshape") { store.tool(bottle, "winecfg") }
                        .disabled(!store.canLaunch)
                    ActionTile(title: "Controllers", symbol: "gamecontroller") { store.tool(bottle, "controllers") }
                        .disabled(!store.canLaunch)
                    ActionTile(title: "Restart", symbol: "arrow.clockwise.circle") { store.tool(bottle, "reboot") }
                        .disabled(!store.canLaunch)
                    ActionTile(title: "Force Quit", symbol: "xmark.circle", tint: Theme.danger) {
                        Task { await store.quitAll(bottle) }
                    }
                    .disabled(!store.canLaunch)
                }
            }

            HStack(spacing: 12) {
                Button {
                    withAnimation(Theme.present) { nav.panel = .uninstall(bottle.name) }
                } label: {
                    Label(neighbours.isEmpty ? "Uninstall" : "Uninstall \(bottle.name)…", systemImage: "trash")
                }
                .buttonStyle(ConsoleButtonStyle(kind: .danger, height: 42))
                .disabled(store.busy != nil)
                Button {
                    NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: app.path)])
                } label: {
                    Label("Show Shortcut", systemImage: "arrow.up.forward.app")
                }
                .buttonStyle(ConsoleButtonStyle(kind: .secondary, height: 42))
                Spacer()
                Button("Close", action: close)
                    .buttonStyle(ConsoleButtonStyle(kind: .secondary, height: 42))
                    .keyboardShortcut(.cancelAction)
            }
        }
    }

    private var sharing: String {
        if neighbours.isEmpty { return "Has a Windows of its own." }
        let names = neighbours.map(\.name).joined(separator: ", ")
        return "Shares its Windows with \(names): graphics settings and tools apply to all of them."
    }

    private func setting(_ field: KeyPath<BottleSettings, String>, key: String) -> Binding<String> {
        Binding(
            get: { bottle.settings[keyPath: field] },
            set: { value in Task { await store.updateSetting(bottle, key, value) } }
        )
    }

    private func toggle(_ field: KeyPath<BottleSettings, Bool>, key: String) -> Binding<Bool> {
        Binding(
            get: { bottle.settings[keyPath: field] },
            set: { on in Task { await store.updateSetting(bottle, key, on ? "on" : "off") } }
        )
    }

    private func runProgram() {
        let panel = NSOpenPanel()
        panel.message = "Choose a Windows program to run with \(app.name)."
        panel.allowedContentTypes = ["exe", "msi", "bat", "lnk"].compactMap { UTType(filenameExtension: $0) }
        panel.canChooseDirectories = false
        if panel.runModal() == .OK, let url = panel.url {
            store.runProgram(bottle, file: url)
        }
    }
}
