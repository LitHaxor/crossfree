import AppKit
import SwiftUI

/// The Games screen: the focused game large over its art (its logo, Play,
/// details), and the library row of covers along the bottom.
struct GamesView: View {
    @EnvironmentObject private var store: Store
    @ObservedObject var nav: ConsoleNav
    let launchSpace: Namespace.ID
    let play: (Bottle, AppShortcut) -> Void

    var body: some View {
        let games = store.orderedApps
        Group {
            if games.isEmpty {
                // Games are added from Install: nothing else to do here.
                Text("No games yet")
                    .font(.system(size: 28, weight: .light))
                    .foregroundStyle(Theme.tertiaryText)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    Spacer(minLength: 12)
                    detail(games)
                        .padding(.horizontal, 64)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Spacer(minLength: 20)
                    LibraryRow(games: games, nav: nav, launchSpace: launchSpace, play: play)
                }
            }
        }
        .onAppear { focusFirstIfNeeded(games) }
        .onChange(of: games.map(\.app.id)) { _, _ in focusFirstIfNeeded(store.orderedApps) }
    }

    @ViewBuilder
    private func detail(_ games: [(bottle: Bottle, app: AppShortcut)]) -> some View {
        ZStack(alignment: .bottomLeading) {
            if let item = games.first(where: { $0.app.id == nav.focusedGame }) {
                GameDetail(bottle: item.bottle, app: item.app, nav: nav, play: play)
                    .id(item.app.id)
                    .transition(
                        .asymmetric(
                            insertion: .opacity.combined(with: .offset(y: 18)),
                            removal: .opacity
                        ))
            }
        }
        .frame(maxWidth: 680, maxHeight: .infinity, alignment: .bottomLeading)
        .animation(Theme.focus, value: nav.focusedGame)
    }

    private func focusFirstIfNeeded(_ games: [(bottle: Bottle, app: AppShortcut)]) {
        let ids = games.map(\.app.id)
        if nav.focusedGame.map({ !ids.contains($0) }) ?? true {
            nav.focusedGame = games.first?.app.id
        }
    }
}

// MARK: - Detail

/// The focused game: logo (or title), facts, Play.
private struct GameDetail: View {
    @EnvironmentObject private var store: Store
    let bottle: Bottle
    let app: AppShortcut
    @ObservedObject var nav: ConsoleNav
    let play: (Bottle, AppShortcut) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            if let logo = app.logoImage {
                Image(nsImage: logo)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: 460, maxHeight: 170, alignment: .bottomLeading)
                    .shadow(color: .black.opacity(0.7), radius: 24, y: 8)
            } else {
                HStack(spacing: 22) {
                    if let icon = app.iconImage {
                        Image(nsImage: icon).resizable().interpolation(.high).scaledToFit()
                            .frame(width: 78, height: 78)
                            .shadow(color: .black.opacity(0.45), radius: 14, y: 6)
                    }
                    Text(app.name)
                        .font(.system(size: 54, weight: .light))
                        .foregroundStyle(Theme.text)
                        .lineLimit(2)
                        .minimumScaleFactor(0.6)
                        .shadow(color: .black.opacity(0.5), radius: 16)
                }
            }
            HStack(spacing: 14) {
                Button {
                    play(bottle, app)
                } label: {
                    Label(store.runningGames.contains(app.id) ? "Resume" : "Play", systemImage: "play.fill")
                        .frame(minWidth: 150)
                }
                .buttonStyle(ConsoleButtonStyle(kind: .primary, height: 56))
                .disabled(!store.canLaunch)
                RoundIconButton(symbol: "ellipsis", size: 56, help: "Options") {
                    withAnimation(Theme.present) { nav.panel = .gameOptions(app.id) }
                }
            }
        }
    }

}

// MARK: - Library row

/// Covers in a row; the focused one larger, ringed and glowing. A click
/// focuses a cover, and a click on the focused cover plays it.
private struct LibraryRow: View {
    @EnvironmentObject private var store: Store
    let games: [(bottle: Bottle, app: AppShortcut)]
    @ObservedObject var nav: ConsoleNav
    let launchSpace: Namespace.ID
    let play: (Bottle, AppShortcut) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Library").font(.system(size: 17, weight: .semibold)).foregroundStyle(Theme.text)
                .padding(.horizontal, 64)
            ScrollViewReader { scroller in
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(alignment: .bottom, spacing: 22) {
                        ForEach(games, id: \.app.id) { item in
                            CoverTile(
                                app: item.app,
                                focused: nav.focusedGame == item.app.id,
                                running: store.runningGames.contains(item.app.id),
                                hidden: nav.launching?.app.id == item.app.id,
                                launchSpace: launchSpace
                            ) {
                                if nav.focusedGame == item.app.id {
                                    play(item.bottle, item.app)
                                } else {
                                    withAnimation(Theme.focus) { nav.focusedGame = item.app.id }
                                }
                            }
                            .id(item.app.id)
                        }
                    }
                    .padding(.horizontal, 64)
                    .padding(.vertical, 22)
                }
                .onChange(of: nav.focusedGame) { _, id in
                    guard let id else { return }
                    withAnimation(Theme.focus) { scroller.scrollTo(id, anchor: UnitPoint(x: 0.12, y: 0.5)) }
                }
            }
        }
    }
}

private struct CoverTile: View {
    let app: AppShortcut
    let focused: Bool
    let running: Bool
    let hidden: Bool
    let launchSpace: Namespace.ID
    let action: () -> Void
    @StateObject private var hover = Local(false)

    var body: some View {
        let size = focused ? CardSize.focused : CardSize.normal
        GameArt(app: app, lit: focused)
            .matchedGeometryEffect(id: app.id, in: launchSpace, isSource: !hidden)
            .frame(width: size.width, height: size.height)
            .overlay(alignment: .topTrailing) {
                if running {
                    RunningBadge().padding(10).transition(.opacity.combined(with: .scale(scale: 0.8)))
                }
            }
            .animation(Theme.focus, value: running)
            .focusRing(focused, radius: CardSize.radius)
            .shadow(color: .black.opacity(0.5), radius: focused ? 22 : 10, y: focused ? 12 : 6)
            .offset(y: hover.value && !focused ? -6 : 0)
            .opacity(hidden ? 0 : 1)
            .animation(Theme.focus, value: focused)
            .animation(Theme.focus, value: hover.value)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
            .onHover { hover.value = $0 }
            .help(app.name)
    }
}

// MARK: - Art

/// A game's card, filling whatever frame it is given. Steam games show
/// Steam's own art: the wide capsule (or banner with the logo) in a wide
/// frame, the portrait cover in a tall one. Other programs show the glass
/// card, with their icon in their icon's colour.
struct GameArt: View {
    let app: AppShortcut
    var lit = false

    var body: some View {
        GeometryReader { proxy in
            let size = proxy.size
            let radius = min(CardSize.radius, min(size.width, size.height) * 0.12)
            ZStack {
                if size.width > size.height, let header = Images.image(atPath: app.header) {
                    Image(nsImage: header).resizable().scaledToFill()
                } else if size.width <= size.height, let cover = Images.image(atPath: app.cover) {
                    Image(nsImage: cover).resizable().scaledToFill()
                } else if let hero = app.heroImage {
                    Image(nsImage: hero).resizable().scaledToFill()
                        .overlay(Color.black.opacity(0.25))
                    if let logo = app.logoImage {
                        Image(nsImage: logo).resizable().scaledToFit().padding(size.width * 0.12)
                    }
                } else {
                    GlassCard(title: app.name, image: app.iconImage, glow: app.glow, lit: lit)
                }
            }
            .frame(width: size.width, height: size.height)
            .clipShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: radius, style: .continuous).strokeBorder(Theme.glassStroke))
        }
    }
}
