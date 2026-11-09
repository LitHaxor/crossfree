import AppKit
import SwiftUI

/// Which screen, which game has focus, and what floats above.
@MainActor
final class ConsoleNav: ObservableObject {
    enum Tab: Hashable, CaseIterable {
        case games, directory, settings

        var title: String {
            switch self {
            case .games: "Games"
            case .directory: "Directory"
            case .settings: "Settings"
            }
        }
    }

    enum Panel: Equatable {
        /// Add a game; optionally alongside an installed one (its prefix).
        case install(alongside: String?)
        /// Nothing can run (the engine check failed): why, and what to do.
        case problem
        /// Uninstall a prefix (by name) and everything in it.
        case uninstall(String)
        case gameOptions(String)
    }

    @Published var tab: Tab = .games
    /// The focused game's ID.
    @Published var focusedGame: String?
    /// The focused store's ID in the Directory, or ``fileTile``.
    @Published var focusedStore: String?
    @Published var panel: Panel?
    /// The game being opened, for the launch splash.
    @Published var launching: Launching?

    /// The From a File tile at the end of the Directory's row.
    static let fileTile = "crossfree.file-tile"
}

/// A game being opened.
struct Launching: Equatable {
    let app: AppShortcut
}

/// The console: full-window artwork, the Games and Settings screens,
/// panels, notifications and the launch splash. Driven by mouse, keyboard
/// or a game controller.
struct ContentView: View {
    @EnvironmentObject private var store: Store
    @StateObject private var nav = ConsoleNav()
    @StateObject private var pads = Gamepads()
    @FocusState private var keyboard: Bool
    @Namespace private var launchSpace

    var body: some View {
        ZStack {
            Backdrop(scene: backdropScene)

            VStack(spacing: 0) {
                TopBar(nav: nav, switchTab: switchTab)
                ZStack {
                    switch nav.tab {
                    case .games:
                        GamesView(nav: nav, launchSpace: launchSpace, play: play)
                            .transition(
                                .asymmetric(
                                    insertion: .opacity.combined(with: .offset(x: -50)),
                                    removal: .opacity.combined(with: .offset(x: -30))
                                ))
                    case .directory:
                        DirectoryView(nav: nav)
                            .transition(.opacity.combined(with: .offset(y: 24)))
                    case .settings:
                        SettingsView(nav: nav, pads: pads)
                            .transition(
                                .asymmetric(
                                    insertion: .opacity.combined(with: .offset(x: 50)),
                                    removal: .opacity.combined(with: .offset(x: 30))
                                ))
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                HintBar(tab: nav.tab, glyphs: pads.glyphs, hasGames: !store.orderedApps.isEmpty)
            }

            Toasts(showsDownload: nav.tab != .directory)

            if let panel = nav.panel {
                PanelHost(nav: nav, panel: panel, play: play)
                    .transition(.opacity)
                    .zIndex(2)
            }

            if let launching = nav.launching {
                LaunchSplash(launching: launching, launchSpace: launchSpace, dismiss: closeSplash)
                    .transition(.opacity)
                    .zIndex(3)
            }
        }
        .ignoresSafeArea()
        .background(Theme.base)
        .preferredColorScheme(.dark)
        .focusable()
        .focusEffectDisabled()
        .focused($keyboard)
        .onMoveCommand { direction in
            switch direction {
            case .left: handle(.left)
            case .right: handle(.right)
            case .up: handle(.up)
            case .down: handle(.down)
            @unknown default: break
            }
        }
        .onKeyPress(.return) {
            handle(.confirm)
            return .handled
        }
        .onKeyPress(.space) {
            handle(.menu)
            return .handled
        }
        .onKeyPress(.tab, phases: .down) { press in
            handle(press.modifiers.contains(.shift) ? .previousTab : .nextTab)
            return .handled
        }
        .onExitCommand { handle(.back) }
        .onAppear {
            keyboard = true
            pads.onCommand = { command in handle(command) }
        }
        .task(id: store.bottleNames) { await store.loadAllApps() }
        // A store may have installed games while crossfree was in the back.
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            Task { await store.refreshLibrary() }
        }
        .onChange(of: nav.panel) { _, panel in
            if panel == nil { keyboard = true }
        }
        // The game's window is up (and in front), or it could not start.
        .onChange(of: store.shownGame) { _, id in
            if id != nil, id == nav.launching?.app.id { closeSplash() }
        }
        .onChange(of: store.errorMessage) { _, error in
            if error != nil, nav.launching != nil { closeSplash() }
        }
        .onChange(of: store.installing) { _, on in
            if on {
                present(nil)
                switchTab(to: .directory)
                store.installing = false
            }
        }
    }

    // MARK: Backdrop

    private var backdropScene: BackdropScene {
        switch nav.tab {
        case .games:
            if let item = focusedGame {
                return BackdropScene(key: item.app.id, hero: item.app.heroImage, icon: nil, hue: item.app.hue)
            }
            return BackdropScene(key: "games", hero: nil, icon: nil, hue: 0.6)
        case .directory:
            let item = Catalog.items.first { $0.id == nav.focusedStore }
            return BackdropScene(key: "install." + (item?.id ?? "file"), hero: nil, icon: nil, hue: item?.hue ?? 0.6)
        case .settings:
            return BackdropScene(key: "settings", hero: nil, icon: nil, hue: 0.6)
        }
    }

    private var focusedGame: (bottle: Bottle, app: AppShortcut)? {
        store.orderedApps.first { $0.app.id == nav.focusedGame }
    }

    // MARK: Commands

    private func handle(_ command: ConsoleCommand) {
        if nav.launching != nil {
            // Only Escape or B cancels the splash: Return or A again (the
            // same press that started the game) must not.
            if command == .back { closeSplash() }
            return
        }
        if nav.panel != nil {
            if command == .back { present(nil) }
            return
        }
        switch command {
        case .previousTab, .nextTab:
            let tabs = ConsoleNav.Tab.allCases
            let index = tabs.firstIndex(of: nav.tab) ?? 0
            let step = command == .nextTab ? 1 : tabs.count - 1
            switchTab(to: tabs[(index + step) % tabs.count])
        case .back:
            if nav.tab != .games { switchTab(to: .games) }
        case .left, .right:
            let step = command == .left ? -1 : 1
            switch nav.tab {
            case .games: moveGame(step)
            case .directory: moveStore(step)
            case .settings: break
            }
        case .confirm:
            switch nav.tab {
            case .games:
                if let item = focusedGame { play(item.bottle, item.app) }
            case .directory:
                Installing.confirm(nav.focusedStore, store: store, nav: nav)
            case .settings:
                break
            }
        case .menu:
            if nav.tab == .games, let item = focusedGame { present(.gameOptions(item.app.id)) }
        case .up, .down:
            break
        }
    }

    private func moveGame(_ step: Int) {
        let ids = store.orderedApps.map(\.app.id)
        guard !ids.isEmpty else { return }
        let current = nav.focusedGame.flatMap { ids.firstIndex(of: $0) } ?? 0
        let next = min(max(current + step, 0), ids.count - 1)
        withAnimation(Theme.focus) { nav.focusedGame = ids[next] }
    }

    private func closeSplash() {
        withAnimation(.spring(response: 0.55, dampingFraction: 0.86)) { nav.launching = nil }
    }

    private func moveStore(_ step: Int) {
        let ids = Catalog.items.map(\.id) + [ConsoleNav.fileTile]
        let current = nav.focusedStore.flatMap { ids.firstIndex(of: $0) } ?? 0
        let next = min(max(current + step, 0), ids.count - 1)
        withAnimation(Theme.focus) { nav.focusedStore = ids[next] }
    }

    private func switchTab(to tab: ConsoleNav.Tab) {
        guard tab != nav.tab else { return }
        withAnimation(Theme.present) { nav.tab = tab }
    }

    private func present(_ panel: ConsoleNav.Panel?) {
        withAnimation(Theme.present) { nav.panel = panel }
    }

    /// Opens a game: the splash grows out of its cover while it starts.
    private func play(_ bottle: Bottle, _ app: AppShortcut) {
        guard store.canLaunch else {
            if store.setupProblem != nil { present(.problem) }
            return
        }
        // Already open: show it, rather than start it a second time.
        if store.resume(bottle, app) { return }
        withAnimation(.spring(response: 0.6, dampingFraction: 0.84)) {
            nav.panel = nil
            nav.tab = .games
            nav.focusedGame = app.id
            nav.launching = Launching(app: app)
        }
        store.start(bottle, app: app)
    }
}

// MARK: - Top bar

private struct TopBar: View {
    @EnvironmentObject private var store: Store
    @ObservedObject var nav: ConsoleNav
    let switchTab: (ConsoleNav.Tab) -> Void
    @Namespace private var underline

    var body: some View {
        HStack(alignment: .center, spacing: 30) {
            ForEach(ConsoleNav.Tab.allCases, id: \.self) { tab($0.title, $0) }
            Spacer()
            if store.setupProblem != nil {
                ProblemPill { withAnimation(Theme.present) { nav.panel = .problem } }
                    .transition(.opacity.combined(with: .scale(scale: 0.9)))
            }
            TimelineView(.everyMinute) { context in
                Text(context.date, style: .time)
                    .font(.system(size: 17, weight: .medium))
                    .foregroundStyle(Theme.text)
                    .monospacedDigit()
            }
        }
        .animation(Theme.present, value: store.setupProblem)
        .padding(.leading, 96)  // the window's traffic lights
        .padding(.trailing, 32)
        .padding(.top, 18)
        .frame(height: 76)
    }

    private func tab(_ title: String, _ tab: ConsoleNav.Tab) -> some View {
        let selected = nav.tab == tab
        return Button {
            switchTab(tab)
        } label: {
            VStack(spacing: 6) {
                Text(title)
                    .font(.system(size: 22, weight: selected ? .semibold : .regular))
                    .foregroundStyle(selected ? Theme.text : Theme.secondaryText)
                ZStack {
                    if selected {
                        Capsule().fill(.white).frame(height: 3)
                            .matchedGeometryEffect(id: "underline", in: underline)
                    }
                }
                .frame(width: 26, height: 3)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .animation(Theme.focus, value: nav.tab)
    }
}

/// Shown only when nothing can run; opens the panel that says why.
private struct ProblemPill: View {
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            HStack(spacing: 8) {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Theme.danger)
                Text("Games can’t start").font(.system(size: 13, weight: .medium))
            }
            .foregroundStyle(Theme.text)
            .padding(.horizontal, 14)
            .frame(height: 32)
            .background(Theme.danger.opacity(0.16), in: Capsule())
            .overlay(Capsule().strokeBorder(Theme.danger.opacity(0.5)))
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Hints

/// Button hints along the bottom, with the connected controller's glyphs.
private struct HintBar: View {
    let tab: ConsoleNav.Tab
    let glyphs: Gamepads.Glyphs?
    let hasGames: Bool

    var body: some View {
        HStack(spacing: 26) {
            if let glyphs {
                if tab == .games && hasGames {
                    hint(Image(systemName: glyphs.confirm), "Play")
                    hint(Image(systemName: glyphs.menu), "Options")
                }
                if tab == .directory { hint(Image(systemName: glyphs.confirm), "Select") }
                if tab != .games { hint(Image(systemName: glyphs.back), "Back") }
                hint(Image(systemName: glyphs.next), nextTab.title)
            } else {
                if tab == .games && hasGames {
                    hint(Image(systemName: "return"), "Play")
                    hint(Image(systemName: "space"), "Options")
                }
                if tab == .directory { hint(Image(systemName: "return"), "Select") }
                if (tab == .games && hasGames) || tab == .directory {
                    hint(Image(systemName: "arrow.left.arrow.right"), "Browse")
                }
            }
            Spacer()
        }
        .font(.system(size: 13, weight: .medium))
        .foregroundStyle(Theme.tertiaryText)
        .padding(.horizontal, 64)
        .padding(.bottom, 22)
        .padding(.top, 8)
        .animation(Theme.focus, value: glyphs)
        .animation(Theme.focus, value: tab)
    }

    private var nextTab: ConsoleNav.Tab {
        let tabs = ConsoleNav.Tab.allCases
        return tabs[((tabs.firstIndex(of: tab) ?? 0) + 1) % tabs.count]
    }

    private func hint(_ glyph: Image, _ label: String) -> some View {
        HStack(spacing: 8) {
            glyph.font(.system(size: 15, weight: .semibold)).foregroundStyle(Theme.secondaryText)
            Text(label)
        }
    }
}

// MARK: - Notifications

/// Console-style notifications in the top-right corner: what is in
/// progress, and errors (which dismiss themselves after a while).
private struct Toasts: View {
    @EnvironmentObject private var store: Store
    /// Whether a store's download shows; the Directory shows its own.
    let showsDownload: Bool

    var body: some View {
        VStack(alignment: .trailing, spacing: 12) {
            if showsDownload, let getting = store.getting, !getting.preparing,
                let item = Catalog.items.first(where: { $0.id == getting.id })
            {
                Toast(tint: Theme.accent, title: "Downloading \(item.name)", message: nil, dismiss: nil) {
                    ProgressView().controlSize(.small).tint(.white)
                }
                .transition(.move(edge: .trailing).combined(with: .opacity))
            }
            if let busy = store.busy {
                Toast(tint: Theme.accent, title: busy, message: nil, dismiss: nil) {
                    ProgressView().controlSize(.small).tint(.white)
                }
                .transition(.move(edge: .trailing).combined(with: .opacity))
            }
            if let error = store.errorMessage {
                Toast(
                    tint: Theme.danger, title: "Something went wrong", message: error,
                    dismiss: { store.errorMessage = nil }
                ) {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Theme.danger)
                }
                .transition(.move(edge: .trailing).combined(with: .opacity))
                .task(id: error) {
                    try? await Task.sleep(for: .seconds(12))
                    if store.errorMessage == error { store.errorMessage = nil }
                }
            }
        }
        .padding(.top, 84)
        .padding(.trailing, 28)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
        .animation(Theme.present, value: store.busy)
        .animation(Theme.present, value: store.errorMessage)
        .animation(Theme.present, value: store.getting?.preparing)
        .animation(Theme.present, value: showsDownload)
        .allowsHitTesting(store.busy != nil || store.errorMessage != nil)
    }
}

private struct Toast<Icon: View>: View {
    let tint: Color
    let title: String
    let message: String?
    let dismiss: (() -> Void)?
    @ViewBuilder let icon: Icon

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            icon.frame(width: 22, height: 22)
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.system(size: 14, weight: .semibold)).foregroundStyle(Theme.text)
                if let message {
                    Text(message).font(.system(size: 13)).foregroundStyle(Theme.secondaryText)
                        .lineLimit(6).textSelection(.enabled)
                }
            }
            Spacer(minLength: 0)
            if let dismiss {
                Button(action: dismiss) {
                    Image(systemName: "xmark").font(.system(size: 11, weight: .bold)).foregroundStyle(
                        Theme.secondaryText
                    )
                    .frame(width: 22, height: 22)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(16)
        .frame(width: 380, alignment: .leading)
        .glass(radius: 18, tint: Theme.baseLift.opacity(0.7))
        .overlay(alignment: .leading) {
            Capsule().fill(tint).frame(width: 3).padding(.vertical, 14)
        }
        .shadow(color: .black.opacity(0.45), radius: 24, y: 10)
    }
}
