import GameController
import SwiftUI

/// What a game controller or the keyboard asks the console UI to do.
enum ConsoleCommand {
    case left, right, up, down
    /// A / Cross, or Return.
    case confirm
    /// B / Circle, or Escape.
    case back
    /// The shoulder buttons, or Tab: switch between Games and Bottles.
    case previousTab, nextTab
    /// The Menu / Options button: the focused item's actions.
    case menu
}

/// Game controllers driving the UI, the way a console's home screen works:
/// D-pad or left stick to move, A (Cross) to open, B (Circle) to go back,
/// the shoulder buttons to switch tabs. Input arrives only while crossfree
/// is the frontmost app, so a running game keeps its controller.
final class Gamepads: ObservableObject {
    /// The connected controller's button symbols for on-screen hints, so a
    /// DualSense shows Cross and Circle and an Xbox pad shows A and B.
    struct Glyphs: Equatable {
        var confirm: String
        var back: String
        var previous: String
        var next: String
        var menu: String
    }

    @Published private(set) var glyphs: Glyphs?
    /// The connected controller's name, such as "DualSense Wireless Controller".
    @Published private(set) var controllerName: String?
    /// Receives every command (on the main queue).
    var onCommand: ((ConsoleCommand) -> Void)?

    /// Where the left stick points, so a push counts once.
    private var stick: ConsoleCommand?
    private var observers: [NSObjectProtocol] = []

    init() {
        let center = NotificationCenter.default
        observers.append(
            center.addObserver(forName: .GCControllerDidConnect, object: nil, queue: .main) { [weak self] note in
                if let controller = note.object as? GCController { self?.attach(controller) }
            })
        observers.append(
            center.addObserver(forName: .GCControllerDidDisconnect, object: nil, queue: .main) { [weak self] _ in
                self?.updateGlyphs()
            })
        GCController.controllers().forEach(attach)
    }

    deinit {
        observers.forEach(NotificationCenter.default.removeObserver)
    }

    private func attach(_ controller: GCController) {
        guard let pad = controller.extendedGamepad else { return }
        controller.handlerQueue = .main
        bind(pad.dpad.left, .left)
        bind(pad.dpad.right, .right)
        bind(pad.dpad.up, .up)
        bind(pad.dpad.down, .down)
        bind(pad.buttonA, .confirm)
        bind(pad.buttonB, .back)
        bind(pad.leftShoulder, .previousTab)
        bind(pad.rightShoulder, .nextTab)
        bind(pad.buttonMenu, .menu)
        pad.leftThumbstick.valueChangedHandler = { [weak self] _, x, y in
            guard let self else { return }
            let direction: ConsoleCommand? =
                abs(x) < 0.55 && abs(y) < 0.55
                ? nil
                : abs(x) > abs(y)
                    ? (x < 0 ? .left : .right)
                    : (y < 0 ? .down : .up)
            if direction != stick, let direction { onCommand?(direction) }
            stick = direction
        }
        updateGlyphs()
    }

    private func bind(_ button: GCControllerButtonInput, _ command: ConsoleCommand) {
        button.pressedChangedHandler = { [weak self] _, _, pressed in
            if pressed { self?.onCommand?(command) }
        }
    }

    private func updateGlyphs() {
        let controller = GCController.controllers().first { $0.extendedGamepad != nil }
        controllerName = controller.map { $0.vendorName ?? "Game controller" }
        guard let pad = controller?.extendedGamepad else {
            glyphs = nil
            return
        }
        glyphs = Glyphs(
            confirm: pad.buttonA.sfSymbolsName ?? "a.circle",
            back: pad.buttonB.sfSymbolsName ?? "b.circle",
            previous: pad.leftShoulder.sfSymbolsName ?? "l1.rectangle.roundedbottom",
            next: pad.rightShoulder.sfSymbolsName ?? "r1.rectangle.roundedbottom",
            menu: pad.buttonMenu.sfSymbolsName ?? "line.3.horizontal.circle"
        )
    }
}
