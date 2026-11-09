import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// The Directory: stores crossfree sets up in one step, and installing from
/// a file. As in Games, the focused one is shown large above the row.
struct DirectoryView: View {
    @EnvironmentObject private var store: Store
    @ObservedObject var nav: ConsoleNav

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Spacer(minLength: 12)
            ZStack(alignment: .bottomLeading) {
                if let item = Catalog.items.first(where: { $0.id == nav.focusedStore }) {
                    StoreDetail(item: item, nav: nav)
                        .id(item.id)
                        .transition(.asymmetric(insertion: .opacity.combined(with: .offset(y: 18)), removal: .opacity))
                } else if nav.focusedStore == ConsoleNav.fileTile {
                    FileDetail()
                        .transition(.asymmetric(insertion: .opacity.combined(with: .offset(y: 18)), removal: .opacity))
                }
            }
            .frame(maxWidth: 680, maxHeight: .infinity, alignment: .bottomLeading)
            .animation(Theme.focus, value: nav.focusedStore)
            .padding(.horizontal, 64)
            .frame(maxWidth: .infinity, alignment: .leading)
            Spacer(minLength: 20)
            StoreRow(nav: nav)
        }
        .onAppear {
            if nav.focusedStore == nil { nav.focusedStore = Catalog.items.first?.id }
        }
    }
}

/// What selecting a tile does, for clicks, Return and the controller.
@MainActor
enum Installing {
    static func confirm(_ id: String?, store: Store, nav: ConsoleNav) {
        if id == ConsoleNav.fileTile {
            chooseFile(store: store)
            return
        }
        guard let item = Catalog.items.first(where: { $0.id == id }) else { return }
        switch store.state(of: item) {
        case .available:
            if store.canLaunch { store.beginGet(item) }
        case .installed:
            if let app = store.app(for: item) { open(app, nav: nav) }
        case .downloading, .preparing, .installing:
            break
        }
    }

    /// Shows an installed store in Games.
    static func open(_ app: AppShortcut, nav: ConsoleNav) {
        withAnimation(Theme.present) {
            nav.tab = .games
            nav.focusedGame = app.id
        }
    }

    /// Asks for an installer, then runs it in a Windows of its own.
    static func chooseFile(store: Store) {
        guard store.canLaunch else { return }
        let panel = NSOpenPanel()
        panel.message = "Choose a game’s installer, such as setup.exe."
        panel.allowedContentTypes = ["exe", "msi", "bat"].compactMap { UTType(filenameExtension: $0) }
        panel.canChooseDirectories = false
        if panel.runModal() == .OK, let url = panel.url {
            Task { await store.install(url) }
        }
    }
}

// MARK: - Detail

private struct StoreDetail: View {
    @EnvironmentObject private var store: Store
    let item: CatalogItem
    @ObservedObject var nav: ConsoleNav

    var body: some View {
        let state = store.state(of: item)
        VStack(alignment: .leading, spacing: 20) {
            HStack(spacing: 22) {
                if let logo = item.logo {
                    Image(nsImage: logo).resizable().interpolation(.high).scaledToFit()
                        .frame(width: 78, height: 78)
                        .shadow(color: .black.opacity(0.45), radius: 14, y: 6)
                }
                Text(item.name)
                    .font(.system(size: 54, weight: .light))
                    .foregroundStyle(Theme.text)
                    .shadow(color: .black.opacity(0.5), radius: 16)
            }
            Text(item.summary)
                .font(.system(size: 17))
                .foregroundStyle(Theme.secondaryText)
                .frame(maxWidth: 560, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
            actions(state)
                .frame(height: 56, alignment: .leading)
                .animation(Theme.focus, value: state)
        }
    }

    @ViewBuilder
    private func actions(_ state: Store.CatalogState) -> some View {
        switch state {
        case .available:
            Button {
                store.beginGet(item)
            } label: {
                Label("Install", systemImage: "arrow.down").frame(minWidth: 150)
            }
            .buttonStyle(ConsoleButtonStyle(kind: .primary, height: 56))
            .disabled(!store.canLaunch || store.getting != nil)
        case .downloading(let fraction):
            HStack(spacing: 16) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(fraction.map { "Downloading… \(Int($0 * 100))%" } ?? "Downloading…")
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(Theme.text)
                        .monospacedDigit()
                    Bar(fraction: fraction).frame(width: 300)
                }
                Button("Cancel") { store.cancelGet() }
                    .buttonStyle(ConsoleButtonStyle(kind: .secondary, height: 44))
            }
        case .preparing:
            status("Getting Windows ready for \(item.name)…")
        case .installing:
            status("Finish setting up \(item.name) in its installer’s window.")
        case .installed:
            Button {
                if let app = store.app(for: item) { Installing.open(app, nav: nav) }
            } label: {
                Label("Show in Games", systemImage: "square.grid.2x2").frame(minWidth: 150)
            }
            .buttonStyle(ConsoleButtonStyle(kind: .primary, height: 56))
        }
    }

    private func status(_ text: String) -> some View {
        HStack(spacing: 12) {
            ProgressView().controlSize(.small).tint(.white)
            Text(text).font(.system(size: 15, weight: .medium)).foregroundStyle(Theme.text)
        }
    }
}

private struct FileDetail: View {
    @EnvironmentObject private var store: Store

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("From a file")
                .font(.system(size: 54, weight: .light))
                .foregroundStyle(Theme.text)
            Button {
                Installing.chooseFile(store: store)
            } label: {
                Label("Choose Installer…", systemImage: "folder").frame(minWidth: 150)
            }
            .buttonStyle(ConsoleButtonStyle(kind: .primary, height: 56))
            .disabled(!store.canLaunch)
        }
    }
}

/// A download's progress; it sweeps while the size is unknown.
private struct Bar: View {
    let fraction: Double?

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule().fill(Color.white.opacity(0.16))
                if let fraction {
                    Capsule().fill(Color.white)
                        .frame(width: max(proxy.size.width * fraction, 6))
                        .animation(.easeOut(duration: 0.25), value: fraction)
                } else {
                    TimelineView(.animation) { context in
                        let phase =
                            context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1.4) / 1.4
                        Capsule().fill(Color.white)
                            .frame(width: proxy.size.width * 0.3)
                            .offset(x: (proxy.size.width * 1.3) * phase - proxy.size.width * 0.3)
                    }
                    .clipShape(Capsule())
                }
            }
        }
        .frame(height: 6)
    }
}

// MARK: - Row

private struct StoreRow: View {
    @EnvironmentObject private var store: Store
    @ObservedObject var nav: ConsoleNav

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Stores").font(.system(size: 17, weight: .semibold)).foregroundStyle(Theme.text)
                .padding(.horizontal, 64)
            ScrollViewReader { scroller in
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(alignment: .bottom, spacing: 22) {
                        ForEach(Catalog.items) { item in
                            StoreTile(
                                item: item,
                                state: store.state(of: item),
                                logo: item.logo ?? store.app(for: item)?.iconImage,
                                focused: nav.focusedStore == item.id
                            ) { select(item.id) }
                            .id(item.id)
                        }
                        FileTile(focused: nav.focusedStore == ConsoleNav.fileTile) { select(ConsoleNav.fileTile) }
                            .id(ConsoleNav.fileTile)
                    }
                    .padding(.horizontal, 64)
                    .padding(.vertical, 22)
                }
                .onChange(of: nav.focusedStore) { _, id in
                    guard let id else { return }
                    withAnimation(Theme.focus) { scroller.scrollTo(id, anchor: UnitPoint(x: 0.12, y: 0.5)) }
                }
            }
        }
    }

    /// A click focuses a tile; a click on the focused one selects it.
    private func select(_ id: String) {
        if nav.focusedStore == id {
            Installing.confirm(id, store: store, nav: nav)
        } else {
            withAnimation(Theme.focus) { nav.focusedStore = id }
        }
    }
}

/// A store: its logo on dark glass, lit from behind in its brand's colour.
private struct StoreTile: View {
    let item: CatalogItem
    let state: Store.CatalogState
    let logo: NSImage?
    let focused: Bool
    let action: () -> Void
    @StateObject private var hover = Local(false)

    var body: some View {
        let size = focused ? CardSize.focused : CardSize.normal
        ZStack(alignment: .topTrailing) {
            GlassCard(title: item.name, image: logo, glow: Color(hex: item.top), lit: focused)
            badge.padding(10)
            progress
        }
        .frame(width: size.width, height: size.height)
        .clipShape(RoundedRectangle(cornerRadius: CardSize.radius, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: CardSize.radius, style: .continuous).strokeBorder(Theme.glassStroke))
        .focusRing(focused, radius: CardSize.radius)
        .shadow(color: .black.opacity(0.5), radius: focused ? 22 : 10, y: focused ? 12 : 6)
        .offset(y: hover.value && !focused ? -6 : 0)
        .animation(Theme.focus, value: focused)
        .animation(Theme.focus, value: hover.value)
        .animation(Theme.focus, value: state)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
        .onHover { hover.value = $0 }
        .help(item.name)
    }

    @ViewBuilder
    private var badge: some View {
        switch state {
        case .installed:
            Label("Installed", systemImage: "checkmark.circle.fill")
                .labelStyle(BadgeLabel())
        case .installing, .preparing:
            Label("Installing", systemImage: "hourglass")
                .labelStyle(BadgeLabel())
        case .available, .downloading:
            EmptyView()
        }
    }

    @ViewBuilder
    private var progress: some View {
        if case .downloading(let fraction) = state {
            Bar(fraction: fraction)
                .padding(.horizontal, 16)
                .padding(.bottom, 12)
                .frame(maxHeight: .infinity, alignment: .bottom)
        }
    }
}

private struct BadgeLabel: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 5) {
            configuration.icon
            configuration.title
        }
        .font(.system(size: 11, weight: .semibold))
        .foregroundStyle(.white)
        .padding(.horizontal, 9)
        .frame(height: 24)
        .background(.black.opacity(0.35), in: Capsule())
    }
}

private struct FileTile: View {
    let focused: Bool
    let action: () -> Void

    var body: some View {
        let size = focused ? CardSize.focused : CardSize.normal
        RoundedRectangle(cornerRadius: CardSize.radius, style: .continuous)
            .fill(Color.white.opacity(focused ? 0.14 : 0.07))
            .overlay(
                RoundedRectangle(cornerRadius: CardSize.radius, style: .continuous)
                    .strokeBorder(Theme.glassStroke, style: StrokeStyle(lineWidth: 1.5, dash: [6, 5]))
            )
            .overlay {
                VStack(spacing: 10) {
                    Image(systemName: "folder.badge.plus").font(.system(size: 28, weight: .light))
                    Text("From a File…").font(.system(size: 14, weight: .medium))
                }
                .foregroundStyle(Theme.secondaryText)
            }
            .frame(width: size.width, height: size.height)
            .focusRing(focused, radius: CardSize.radius)
            .animation(Theme.focus, value: focused)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }
}

extension Color {
    /// A colour from 0xRRGGBB.
    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255
        )
    }
}

extension CatalogItem {
    /// The store's logo, from its installer's icon (tools/store-logos.py),
    /// in the app's `Contents/Resources/Stores`.
    var logo: NSImage? {
        Images.image(atPath: Bundle.main.path(forResource: id, ofType: "png", inDirectory: "Stores"))
    }

    /// The brand colour's hue, for the backdrop; the default blue for a
    /// grey brand (whose hue means nothing).
    var hue: Double {
        let color = NSColor(
            srgbRed: CGFloat((top >> 16) & 0xFF) / 255,
            green: CGFloat((top >> 8) & 0xFF) / 255,
            blue: CGFloat(top & 0xFF) / 255,
            alpha: 1
        )
        return color.saturationComponent < 0.15 ? 0.6 : Double(color.hueComponent)
    }
}
