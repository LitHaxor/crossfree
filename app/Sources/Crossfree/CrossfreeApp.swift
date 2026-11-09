import SwiftUI

@main
struct CrossfreeApp: App {
    @StateObject private var store = Store()

    var body: some Scene {
        WindowGroup("crossfree") {
            ContentView()
                .environmentObject(store)
                .frame(minWidth: 1120, minHeight: 720)
                .task { await store.refresh() }
        }
        // A console screen: art edge to edge, no title bar.
        .windowStyle(.hiddenTitleBar)
        .windowBackgroundDragBehavior(.enabled)
        .defaultSize(width: 1440, height: 900)
        .commands {
            // One window; ⌘N goes to the Directory rather than opening a window.
            CommandGroup(replacing: .newItem) {
                Button("Add Games…") { store.installing = true }
                    .keyboardShortcut("n")
                Button("Refresh") { Task { await store.reloadAll() } }
                    .keyboardShortcut("r")
            }
        }
    }
}
