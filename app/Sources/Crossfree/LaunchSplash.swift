import AppKit
import SwiftUI

/// Opening a game: its cover flies out of the library row to the centre,
/// the art fades up behind it with a burst of light, and a shimmer runs
/// while the game starts. It stays until the game's window comes up (the
/// store then puts it in front, and the console closes this), or for at
/// most half a minute. Escape, B or its Cancel button cancel it; other
/// clicks do nothing, since a second click on the card that started the
/// game must not close it. (The console ignores commands other than back.)
struct LaunchSplash: View {
    let launching: Launching
    let launchSpace: Namespace.ID
    let dismiss: () -> Void
    @StateObject private var shown = Local(false)
    @StateObject private var burst = Local(false)

    var body: some View {
        GeometryReader { proxy in
            ZStack {
                Theme.base.opacity(0.94)
                art(in: proxy.size)
                LinearGradient(colors: [.clear, Theme.base.opacity(0.9)], startPoint: .center, endPoint: .bottom)
                Circle()
                    .fill(
                        RadialGradient(
                            colors: [.white.opacity(0.6), Theme.accent.opacity(0.25), .clear],
                            center: .center, startRadius: 0, endRadius: 260)
                    )
                    .frame(width: 520, height: 520)
                    .scaleEffect(burst.value ? 3.4 : 0.3)
                    .opacity(burst.value ? 0 : 1)
                    .blendMode(.plusLighter)
                    .allowsHitTesting(false)
                VStack(spacing: 30) {
                    GameArt(app: launching.app, lit: true)
                        .matchedGeometryEffect(id: launching.app.id, in: launchSpace, isSource: true)
                        .frame(width: 416, height: 257)
                        .shadow(color: Theme.accent.opacity(shown.value ? 0.55 : 0), radius: 46)
                        .shadow(color: .black.opacity(0.6), radius: 24, y: 16)
                    VStack(spacing: 16) {
                        if let logo = launching.app.logoImage {
                            Image(nsImage: logo).resizable().scaledToFit().frame(maxWidth: 340, maxHeight: 110)
                        } else {
                            Text(launching.app.name)
                                .font(.system(size: 34, weight: .light))
                                .foregroundStyle(Theme.text)
                                .multilineTextAlignment(.center)
                        }
                        Text("Starting…")
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(Theme.secondaryText)
                        Shimmer().frame(width: 240, height: 4)
                    }
                    .opacity(shown.value ? 1 : 0)
                    .offset(y: shown.value ? 0 : 24)
                }
                CancelHint(cancel: dismiss)
                    .padding(.bottom, 30)
                    .frame(maxHeight: .infinity, alignment: .bottom)
                    .opacity(shown.value ? 1 : 0)
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
        }
        .ignoresSafeArea()
        // Swallows clicks, so none reaches the library underneath.
        .contentShape(Rectangle())
        .onTapGesture {}
        .onAppear {
            withAnimation(.easeOut(duration: 0.9).delay(0.1)) { shown.value = true }
            withAnimation(.easeOut(duration: 1.3).delay(0.12)) { burst.value = true }
        }
        .task {
            try? await Task.sleep(for: .seconds(Store.windowWait))
            dismiss()
        }
    }

    @ViewBuilder
    private func art(in size: CGSize) -> some View {
        if let hero = launching.app.heroImage {
            Image(nsImage: hero)
                .resizable()
                .scaledToFill()
                .frame(width: size.width, height: size.height)
                .scaleEffect(shown.value ? 1.0 : 1.18)
                .blur(radius: shown.value ? 0 : 26)
                .opacity(shown.value ? 0.5 : 0)
                .clipped()
        } else if let icon = launching.app.iconImage {
            Image(nsImage: icon)
                .resizable()
                .scaledToFill()
                .frame(width: size.width, height: size.height)
                .blur(radius: 100)
                .saturation(1.6)
                .opacity(shown.value ? 0.5 : 0)
                .clipped()
        }
    }
}

/// A thin bar with a light running across it.
/// "esc Cancel": turns red and grows under the pointer, and cancels on a
/// click (the one click the splash takes).
private struct CancelHint: View {
    let cancel: () -> Void
    @StateObject private var hover = Local(false)

    var body: some View {
        Button(action: cancel) {
            HStack(spacing: 8) {
                Image(systemName: "escape").font(.system(size: 15, weight: .semibold))
                Text("Cancel")
            }
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(hover.value ? Theme.danger : Theme.tertiaryText)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .contentShape(Rectangle())
            .scaleEffect(hover.value ? 1.15 : 1)
            .animation(Theme.focus, value: hover.value)
        }
        .buttonStyle(.plain)
        .onHover { hover.value = $0 }
    }
}

private struct Shimmer: View {
    @StateObject private var phase = Local(false)

    var body: some View {
        GeometryReader { proxy in
            Capsule()
                .fill(Color.white.opacity(0.12))
                .overlay(alignment: .leading) {
                    Capsule()
                        .fill(
                            LinearGradient(colors: [.clear, .white, .clear], startPoint: .leading, endPoint: .trailing)
                        )
                        .frame(width: proxy.size.width * 0.4)
                        .offset(x: phase.value ? proxy.size.width : -proxy.size.width * 0.4)
                }
                .clipShape(Capsule())
        }
        .onAppear {
            withAnimation(.easeInOut(duration: 1.1).repeatForever(autoreverses: false)) { phase.value = true }
        }
    }
}
