import AppKit
import SwiftUI

/// Settings: the controller, and storage (what is installed, how much
/// space it takes, uninstalling). The engine is part of crossfree, and is
/// not a setting.
struct SettingsView: View {
    @EnvironmentObject private var store: Store
    @ObservedObject var nav: ConsoleNav
    @ObservedObject var pads: Gamepads
    @StateObject private var sizes = Local<[String: Int64]>([:])

    var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 34) {
                Text("Settings")
                    .font(.system(size: 46, weight: .light))
                    .foregroundStyle(Theme.text)
                VStack(alignment: .leading, spacing: 14) {
                    SectionTitle("Controller")
                    ControllerCard(pads: pads)
                        .padding(22)
                        .frame(maxWidth: 560, alignment: .leading)
                        .glass(radius: 24)
                }
                VStack(alignment: .leading, spacing: 14) {
                    SectionTitle("Storage")
                    if store.bottles.isEmpty {
                        Text("Nothing installed yet.")
                            .font(.system(size: 15))
                            .foregroundStyle(Theme.secondaryText)
                    }
                    ForEach(store.bottles) { bottle in
                        StorageRow(
                            bottle: bottle, apps: store.apps[bottle.name] ?? [], size: sizes.value[bottle.name],
                            nav: nav)
                    }
                }
            }
            .padding(.horizontal, 64)
            .padding(.top, 12)
            .padding(.bottom, 24)
        }
        .task(id: store.bottleNames) { await measure() }
    }

    /// Measures each install's folder, off the main thread.
    private func measure() async {
        for bottle in store.bottles {
            let url = URL(fileURLWithPath: bottle.path)
            let size = await Task.detached(priority: .utility) { SettingsView.folderSize(url) }.value
            sizes.value[bottle.name] = size
        }
    }

    /// Bytes on disk under `url`, without following links (a prefix's
    /// `dosdevices` links to `/`).
    nonisolated static func folderSize(_ url: URL) -> Int64 {
        let keys: [URLResourceKey] = [.totalFileAllocatedSizeKey, .isSymbolicLinkKey, .isRegularFileKey]
        guard let walker = FileManager.default.enumerator(at: url, includingPropertiesForKeys: keys) else { return 0 }
        var total: Int64 = 0
        for case let item as URL in walker {
            guard let values = try? item.resourceValues(forKeys: Set(keys)) else { continue }
            if values.isSymbolicLink == true {
                walker.skipDescendants()
                continue
            }
            if values.isRegularFile == true { total += Int64(values.totalFileAllocatedSize ?? 0) }
        }
        return total
    }
}

private struct ControllerCard: View {
    @ObservedObject var pads: Gamepads

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
                Image(systemName: pads.controllerName == nil ? "gamecontroller" : "gamecontroller.fill")
                    .font(.system(size: 26))
                    .foregroundStyle(pads.controllerName == nil ? Theme.tertiaryText : Theme.accent)
                Text(pads.controllerName ?? "No controller")
                    .font(.system(size: 18, weight: .medium))
                    .foregroundStyle(Theme.text)
            }
            if let glyphs = pads.glyphs {
                HStack(spacing: 18) {
                    glyph(glyphs.confirm, "Play")
                    glyph(glyphs.back, "Back")
                    glyph(glyphs.next, "Switch")
                }
            } else {
                Text(
                    "Connect a PlayStation, Xbox or other controller over Bluetooth or USB to browse and play from the couch."
                )
                .font(.system(size: 14))
                .foregroundStyle(Theme.secondaryText)
                .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func glyph(_ symbol: String, _ label: String) -> some View {
        HStack(spacing: 6) {
            Image(systemName: symbol).font(.system(size: 17))
            Text(label).font(.system(size: 13))
        }
        .foregroundStyle(Theme.secondaryText)
    }
}

/// One install: its games, its size; open its files or uninstall it.
private struct StorageRow: View {
    @EnvironmentObject private var store: Store
    let bottle: Bottle
    let apps: [AppShortcut]
    let size: Int64?
    @ObservedObject var nav: ConsoleNav

    var body: some View {
        HStack(spacing: 18) {
            ZStack(alignment: .leading) {
                if apps.isEmpty {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(Color.white.opacity(0.08))
                        .frame(width: 48, height: 72)
                        .overlay(Image(systemName: "square.dashed").foregroundStyle(Theme.tertiaryText))
                }
                ForEach(Array(apps.prefix(3).enumerated()), id: \.element.id) { index, app in
                    GameArt(app: app)
                        .frame(width: 48, height: 72)
                        .shadow(color: .black.opacity(0.5), radius: 4, x: 2)
                        .offset(x: CGFloat(index) * 26)
                        .zIndex(Double(3 - index))
                }
            }
            .frame(width: 110, alignment: .leading)
            VStack(alignment: .leading, spacing: 4) {
                Text(bottle.name).font(.system(size: 18, weight: .medium)).foregroundStyle(Theme.text)
                Text(summary).font(.system(size: 14)).foregroundStyle(Theme.secondaryText)
            }
            Spacer()
            Button {
                store.openDriveC(bottle)
            } label: {
                Label("Files", systemImage: "folder")
            }
            .buttonStyle(ConsoleButtonStyle(kind: .secondary, height: 38))
            Button {
                withAnimation(Theme.present) { nav.panel = .uninstall(bottle.name) }
            } label: {
                Label("Uninstall", systemImage: "trash")
            }
            .buttonStyle(ConsoleButtonStyle(kind: .danger, height: 38))
            .disabled(store.busy != nil)
        }
        .padding(16)
        .glass(radius: 20)
    }

    private var summary: String {
        let names = apps.isEmpty ? "Nothing installed" : apps.map(\.name).joined(separator: ", ")
        let bytes = size.map { ByteCountFormatter.string(fromByteCount: $0, countStyle: .file) } ?? "Measuring…"
        return "\(names) · \(bytes)"
    }
}
