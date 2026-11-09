import AppKit
import SwiftUI

/// The size of the cards in Games' and the Directory's rows.
enum CardSize {
    static let normal = CGSize(width: 220, height: 136)
    static let focused = CGSize(width: 264, height: 163)
    static let radius: CGFloat = 20
}

/// The console's card: dark glass lit from behind in `glow`, an icon or
/// logo above a name. The Directory's stores and Games' programs without
/// store art both use it. It fills the frame it is given.
struct GlassCard: View {
    let title: String
    let image: NSImage?
    let glow: Color
    /// Focused: the glow brightens.
    var lit = false

    var body: some View {
        GeometryReader { proxy in
            let size = proxy.size
            let side = min(size.height * 0.46, size.width * 0.5)
            ZStack {
                Theme.baseLift.opacity(0.9)
                RadialGradient(
                    colors: [glow.opacity(lit ? 0.75 : 0.5), .clear],
                    center: UnitPoint(x: 0.5, y: 0.42), startRadius: 0, endRadius: max(size.width, size.height) * 0.62
                )
                VStack(spacing: size.height * 0.07) {
                    Group {
                        if let image {
                            Image(nsImage: image).resizable().interpolation(.high).scaledToFit()
                                .shadow(color: .black.opacity(0.4), radius: side * 0.1, y: side * 0.05)
                        } else {
                            Monogram(name: title, radius: side * 0.22)
                        }
                    }
                    .frame(width: side, height: side)
                    Text(title)
                        .font(.system(size: max(size.width * 0.064, 9), weight: .semibold))
                        .foregroundStyle(.white.opacity(lit ? 1 : 0.85))
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                        .padding(.horizontal, size.width * 0.06)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .frame(width: size.width, height: size.height)
        }
    }
}

/// On a running game's card.
struct RunningBadge: View {
    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(Theme.running).frame(width: 7, height: 7)
                .shadow(color: Theme.running, radius: 4)
            Text("Running")
        }
        .font(.system(size: 11, weight: .semibold))
        .foregroundStyle(.white)
        .padding(.horizontal, 9)
        .frame(height: 24)
        .background(.black.opacity(0.5), in: Capsule())
    }
}

extension AppShortcut {
    /// The glow behind the app's card and on the backdrop: its icon's colour.
    var glow: Color { Color(hue: hue, saturation: 0.6, brightness: 0.75) }
}
