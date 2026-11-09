import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins
import SwiftUI

/// crossfree's console look: colours, type and motion.
enum Theme {
    /// The deep blue-black everything sits on.
    static let base = Color(red: 0.024, green: 0.031, blue: 0.063)
    static let baseLift = Color(red: 0.063, green: 0.082, blue: 0.145)
    /// Focus glow and active states.
    static let accent = Color(red: 0.40, green: 0.74, blue: 1.0)
    static let running = Color(red: 0.32, green: 0.86, blue: 0.50)
    static let warning = Color(red: 1.0, green: 0.72, blue: 0.28)
    static let danger = Color(red: 1.0, green: 0.38, blue: 0.40)

    static let text = Color.white
    static let secondaryText = Color.white.opacity(0.66)
    static let tertiaryText = Color.white.opacity(0.42)
    static let glass = Color.white.opacity(0.07)
    static let glassStroke = Color.white.opacity(0.13)

    /// Focus moves: quick, with a little settle.
    static let focus = Animation.spring(response: 0.34, dampingFraction: 0.82)
    /// Backdrop cross-fades.
    static let fade = Animation.easeInOut(duration: 0.5)
    /// Panels and toasts appearing.
    static let present = Animation.spring(response: 0.42, dampingFraction: 0.86)

    /// A stable colour for a name (Swift's `hashValue` changes every launch).
    static func hue(for name: String) -> Double {
        let hash = name.unicodeScalars.reduce(0) { ($0 &* 31 &+ Int($1.value)) & 0xffff }
        return Double(hash % 360) / 360
    }
}

/// Decoded images by file path, so redrawing does not read files again.
/// The launcher names icon files after their source's path, size and date,
/// and Steam's artwork files are named per game, so a path's content does
/// not change while the app runs.
enum Images {
    private static let cache = NSCache<NSString, NSImage>()

    static func image(atPath path: String?) -> NSImage? {
        guard let path else { return nil }
        if let image = cache.object(forKey: path as NSString) { return image }
        guard let image = NSImage(contentsOfFile: path) else { return nil }
        cache.setObject(image, forKey: path as NSString)
        return image
    }

    /// The widest representation of `image`, in pixels.
    static func pixelWidth(_ image: NSImage) -> Int {
        image.representations.map(\.pixelsWide).max() ?? 0
    }

    private static var averages: [String: NSColor] = [:]
    private static let context = CIContext(options: [.workingColorSpace: NSNull()])

    /// The average colour of the opaque parts of the image at `path`.
    static func averageColor(atPath path: String?) -> NSColor? {
        guard let path else { return nil }
        if let color = averages[path] { return color }
        guard let image = image(atPath: path),
            let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
        else { return nil }
        let input = CIImage(cgImage: cgImage)
        let filter = CIFilter.areaAverage()
        filter.inputImage = input
        filter.extent = input.extent
        guard let output = filter.outputImage else { return nil }
        var pixel = [UInt8](repeating: 0, count: 4)
        context.render(
            output, toBitmap: &pixel, rowBytes: 4, bounds: CGRect(x: 0, y: 0, width: 1, height: 1),
            format: .RGBA8, colorSpace: CGColorSpaceCreateDeviceRGB())
        let alpha = CGFloat(pixel[3]) / 255
        guard alpha > 0.05 else { return nil }
        // The average is premultiplied: divide out the transparent parts.
        let channel = { (value: UInt8) in min(CGFloat(value) / 255 / alpha, 1) }
        let color = NSColor(deviceRed: channel(pixel[0]), green: channel(pixel[1]), blue: channel(pixel[2]), alpha: 1)
        averages[path] = color
        return color
    }
}

extension AppShortcut {
    /// The hue of the app's icon, or one derived from its name.
    var hue: Double {
        if let color = Images.averageColor(atPath: icon), color.saturationComponent > 0.12 {
            return Double(color.hueComponent)
        }
        return Theme.hue(for: name)
    }
}

// MARK: - Surfaces

/// Frosted glass: the panel material everything floats on.
struct Glass: ViewModifier {
    var radius: CGFloat = 22
    var tint: Color = Theme.glass

    func body(content: Content) -> some View {
        content
            .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .background(tint, in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: radius, style: .continuous).strokeBorder(Theme.glassStroke))
    }
}

extension View {
    func glass(radius: CGFloat = 22, tint: Color = Theme.glass) -> some View {
        modifier(Glass(radius: radius, tint: tint))
    }

    /// The console focus ring: a white outline with a soft glow.
    func focusRing(_ active: Bool, radius: CGFloat) -> some View {
        overlay {
            RoundedRectangle(cornerRadius: radius + 5, style: .continuous)
                .strokeBorder(Color.white, lineWidth: 3)
                .padding(-5)
                .shadow(color: Theme.accent.opacity(0.9), radius: 14)
                .shadow(color: .white.opacity(0.35), radius: 4)
                .opacity(active ? 1 : 0)
        }
    }
}

// MARK: - Buttons

/// Console buttons: a bright pill for the main action, glass for the rest.
struct ConsoleButtonStyle: ButtonStyle {
    enum Kind { case primary, secondary, danger }
    var kind: Kind = .secondary
    var height: CGFloat = 48

    func makeBody(configuration: Configuration) -> some View {
        ConsoleButtonBody(configuration: configuration, kind: kind, height: height)
    }
}

private struct ConsoleButtonBody: View {
    let configuration: ButtonStyleConfiguration
    let kind: ConsoleButtonStyle.Kind
    let height: CGFloat
    @Environment(\.isEnabled) private var isEnabled
    @StateObject private var hover = Local(false)

    var body: some View {
        configuration.label
            .font(.system(size: height > 44 ? 17 : 14, weight: .semibold))
            .foregroundStyle(foreground)
            .padding(.horizontal, height * 0.5)
            .frame(height: height)
            .background(background, in: Capsule())
            .overlay(Capsule().strokeBorder(kind == .primary ? .clear : Theme.glassStroke))
            .shadow(color: kind == .primary && hover.value ? .white.opacity(0.35) : .clear, radius: 16)
            .scaleEffect(configuration.isPressed ? 0.95 : hover.value ? 1.03 : 1)
            .opacity(isEnabled ? 1 : 0.4)
            .animation(Theme.focus, value: configuration.isPressed)
            .animation(Theme.focus, value: hover.value)
            .contentShape(Capsule())
            .onHover { hover.value = $0 && isEnabled }
    }

    private var foreground: Color {
        switch kind {
        case .primary: Theme.base
        case .secondary: Theme.text
        case .danger: Theme.danger
        }
    }

    private var background: AnyShapeStyle {
        switch kind {
        case .primary: AnyShapeStyle(Color.white)
        case .secondary: AnyShapeStyle(Color.white.opacity(hover.value ? 0.16 : 0.09))
        case .danger: AnyShapeStyle(Theme.danger.opacity(hover.value ? 0.22 : 0.12))
        }
    }
}

/// A round glass icon button (more, close, settings).
struct RoundIconButton: View {
    let symbol: String
    var size: CGFloat = 48
    var help: String = ""
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: size * 0.36, weight: .semibold))
                .frame(width: size, height: size)
        }
        .buttonStyle(RoundIconStyle())
        .help(help)
    }
}

private struct RoundIconStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        RoundIconBody(configuration: configuration)
    }
}

private struct RoundIconBody: View {
    let configuration: ButtonStyleConfiguration
    @StateObject private var hover = Local(false)

    var body: some View {
        configuration.label
            .foregroundStyle(Theme.text)
            .background(Color.white.opacity(hover.value ? 0.18 : 0.09), in: Circle())
            .overlay(Circle().strokeBorder(Theme.glassStroke))
            .scaleEffect(configuration.isPressed ? 0.92 : hover.value ? 1.05 : 1)
            .animation(Theme.focus, value: configuration.isPressed)
            .animation(Theme.focus, value: hover.value)
            .contentShape(Circle())
            .onHover { hover.value = $0 }
    }
}

// MARK: - Controls

/// A console toggle: a wide row with a sliding knob.
struct ConsoleToggle: View {
    let title: String
    let detail: String
    @Binding var isOn: Bool

    var body: some View {
        Button {
            isOn.toggle()
        } label: {
            HStack(spacing: 16) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(title).font(.system(size: 16, weight: .medium)).foregroundStyle(Theme.text)
                    Text(detail).font(.system(size: 13)).foregroundStyle(Theme.secondaryText)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 12)
                Capsule()
                    .fill(isOn ? Theme.accent : Color.white.opacity(0.16))
                    .frame(width: 52, height: 30)
                    .overlay(alignment: isOn ? .trailing : .leading) {
                        Circle().fill(.white).padding(3).shadow(radius: 2, y: 1)
                    }
                    .animation(Theme.focus, value: isOn)
            }
            .padding(.horizontal, 18)
            .padding(.vertical, 14)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .glass(radius: 18)
    }
}

/// A console segmented choice: pills, the chosen one bright.
struct ConsoleChoice: View {
    let title: String
    let options: [(value: String, label: String)]
    @Binding var selection: String
    @Namespace private var pill

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.system(size: 13, weight: .semibold)).foregroundStyle(Theme.secondaryText)
                .textCase(.uppercase).tracking(1.2)
            HStack(spacing: 4) {
                ForEach(options, id: \.value) { option in
                    let chosen = option.value == selection
                    Button {
                        selection = option.value
                    } label: {
                        Text(option.label)
                            .font(.system(size: 14, weight: chosen ? .semibold : .regular))
                            .foregroundStyle(chosen ? Theme.base : Theme.text)
                            .padding(.horizontal, 16)
                            .frame(height: 36)
                            .background {
                                if chosen {
                                    Capsule().fill(.white).matchedGeometryEffect(id: "pill", in: pill)
                                }
                            }
                            .contentShape(Capsule())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(4)
            .background(Color.white.opacity(0.08), in: Capsule())
            .animation(Theme.focus, value: selection)
        }
    }
}

/// A square glass action: a symbol over a label.
struct ActionTile: View {
    let title: String
    let symbol: String
    var tint: Color = Theme.text
    let action: () -> Void
    @Environment(\.isEnabled) private var isEnabled
    @StateObject private var hover = Local(false)

    var body: some View {
        Button(action: action) {
            VStack(spacing: 10) {
                Image(systemName: symbol).font(.system(size: 22, weight: .light))
                Text(title).font(.system(size: 13, weight: .medium)).lineLimit(1).minimumScaleFactor(0.8)
            }
            .foregroundStyle(tint)
            .frame(maxWidth: .infinity)
            .frame(height: 84)
            .background(
                Color.white.opacity(hover.value ? 0.14 : 0.07),
                in: RoundedRectangle(cornerRadius: 18, style: .continuous)
            )
            .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).strokeBorder(Theme.glassStroke))
            .scaleEffect(hover.value ? 1.03 : 1)
            .opacity(isEnabled ? 1 : 0.4)
            .animation(Theme.focus, value: hover.value)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover.value = $0 && isEnabled }
    }
}

/// A small uppercase section title.
struct SectionTitle: View {
    let title: String
    init(_ title: String) { self.title = title }

    var body: some View {
        Text(title)
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(Theme.secondaryText)
            .textCase(.uppercase)
            .tracking(1.4)
    }
}

/// A letter on a gradient: the tile for a program without an icon.
struct Monogram: View {
    let name: String
    var radius: CGFloat

    var body: some View {
        let hue = Theme.hue(for: name)
        RoundedRectangle(cornerRadius: radius, style: .continuous)
            .fill(
                LinearGradient(
                    colors: [
                        Color(hue: hue, saturation: 0.55, brightness: 0.9),
                        Color(hue: hue, saturation: 0.8, brightness: 0.45),
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
            .overlay(
                GeometryReader { proxy in
                    Text(String(name.prefix(1)).uppercased())
                        .font(.system(size: proxy.size.width * 0.44, weight: .bold, design: .rounded))
                        .foregroundStyle(.white)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            )
    }
}
