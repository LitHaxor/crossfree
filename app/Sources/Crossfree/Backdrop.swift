import AppKit
import SwiftUI

/// What fills the window behind everything: the focused game's art.
struct BackdropScene: Equatable {
    /// Changes when the scene does; the art cross-fades on change.
    let key: String
    let hero: NSImage?
    let icon: NSImage?
    /// Colour of the ambient glow.
    let hue: Double

    static func == (a: BackdropScene, b: BackdropScene) -> Bool { a.key == b.key }
}

/// The full-window backdrop: a slowly drifting hero banner (Steam games),
/// or the app's icon blown up and blurred, over a coloured glow, under
/// scrims that keep text readable.
struct Backdrop: View {
    let scene: BackdropScene

    var body: some View {
        GeometryReader { proxy in
            ZStack {
                Theme.base
                Glow(hue: scene.hue)
                    .id("glow:\(scene.key)")
                    .transition(.opacity)
                Group {
                    if let hero = scene.hero {
                        DriftingArt(
                            image: hero, size: CGSize(width: proxy.size.width, height: proxy.size.height * 0.74))
                    } else if let icon = scene.icon {
                        Image(nsImage: icon)
                            .resizable()
                            .scaledToFill()
                            .frame(width: proxy.size.width * 0.9, height: proxy.size.height * 0.9)
                            .blur(radius: 90)
                            .saturation(1.5)
                            .opacity(0.45)
                            .offset(x: proxy.size.width * 0.25, y: -proxy.size.height * 0.2)
                    }
                }
                .frame(width: proxy.size.width, height: proxy.size.height, alignment: .top)
                .id("art:\(scene.key)")
                .transition(.opacity)
                Scrims()
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
            .clipped()
        }
        .animation(Theme.fade, value: scene.key)
        .ignoresSafeArea()
        .allowsHitTesting(false)
    }
}

/// A hero banner with a slow push-in, faded out towards the bottom.
private struct DriftingArt: View {
    let image: NSImage
    let size: CGSize
    @StateObject private var drift = Local(false)

    var body: some View {
        Image(nsImage: image)
            .resizable()
            .scaledToFill()
            .frame(width: size.width, height: size.height)
            .scaleEffect(drift.value ? 1.08 : 1.0, anchor: .center)
            .clipped()
            .mask(
                LinearGradient(
                    stops: [
                        .init(color: .black, location: 0), .init(color: .black, location: 0.55),
                        .init(color: .clear, location: 1),
                    ],
                    startPoint: .top,
                    endPoint: .bottom
                )
            )
            .onAppear {
                withAnimation(.easeInOut(duration: 28).repeatForever(autoreverses: true)) { drift.value = true }
            }
    }
}

/// Two soft pools of colour.
private struct Glow: View {
    let hue: Double

    var body: some View {
        ZStack {
            RadialGradient(
                colors: [Color(hue: hue, saturation: 0.7, brightness: 0.55).opacity(0.55), .clear],
                center: UnitPoint(x: 0.85, y: 0.15), startRadius: 0, endRadius: 760
            )
            RadialGradient(
                colors: [
                    Color(hue: (hue + 0.08).truncatingRemainder(dividingBy: 1), saturation: 0.6, brightness: 0.4)
                        .opacity(0.4), .clear,
                ],
                center: UnitPoint(x: 0.1, y: 0.95), startRadius: 0, endRadius: 620
            )
        }
    }
}

/// Darkening at the top (title bar), on the left (text) and at the bottom
/// (the library row).
private struct Scrims: View {
    var body: some View {
        ZStack {
            LinearGradient(
                colors: [.black.opacity(0.55), .clear], startPoint: .top, endPoint: UnitPoint(x: 0.5, y: 0.2))
            LinearGradient(
                stops: [
                    .init(color: Theme.base.opacity(0.88), location: 0),
                    .init(color: Theme.base.opacity(0.35), location: 0.45), .init(color: .clear, location: 0.75),
                ],
                startPoint: .leading,
                endPoint: .trailing
            )
            LinearGradient(
                stops: [
                    .init(color: .clear, location: 0.4), .init(color: Theme.base.opacity(0.85), location: 0.78),
                    .init(color: Theme.base, location: 1),
                ],
                startPoint: .top,
                endPoint: .bottom
            )
        }
    }
}
