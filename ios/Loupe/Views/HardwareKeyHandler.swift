import SwiftUI
import UIKit

/// Native UIKit `UIKeyCommand` responder for iPad Magic Keyboard & Bluetooth keyboards.
/// Supports desktop LOUPE shortcuts including `F1`–`F5` star ratings, `F6`/`0` clear, `P`/`X`/`U`,
/// `Z` zoom, `R` auto-straighten, `G` composition overlay, double-Space zoom, arrows, Delete, Esc,
/// and automatically populates iPadOS's hold-⌘ shortcut discovery HUD (`discoverabilityTitle`).
struct HardwareKeyHandler: UIViewControllerRepresentable {
    struct Callbacks {
        let onRate: (Int) -> Void
        let onPick: () -> Void
        let onReject: () -> Void
        let onClearFlag: () -> Void
        let onToggleZoom: () -> Void
        let onToggleOverlay: () -> Void
        let onAutoStraighten: () -> Void
        let onPrevious: () -> Void
        let onNext: () -> Void
        let onSpace: () -> Void
        let onUndo: () -> Void
        let onDelete: () -> Void
        let onClose: () -> Void
    }

    let callbacks: Callbacks

    func makeUIViewController(context: Context) -> KeyCommandViewController {
        let vc = KeyCommandViewController()
        vc.callbacks = callbacks
        return vc
    }

    func updateUIViewController(_ uiViewController: KeyCommandViewController, context: Context) {
        uiViewController.callbacks = callbacks
        if uiViewController.view.window != nil && !uiViewController.isFirstResponder {
            uiViewController.becomeFirstResponder()
        }
    }

    final class KeyCommandViewController: UIViewController {
        var callbacks: Callbacks?

        override var canBecomeFirstResponder: Bool { true }

        override func viewDidAppear(_ animated: Bool) {
            super.viewDidAppear(animated)
            becomeFirstResponder()
        }

        override var keyCommands: [UIKeyCommand]? {
            [
                makeCommand(UIKeyCommand.f1, title: "별점 1개", action: #selector(rate1)),
                makeCommand(UIKeyCommand.f2, title: "별점 2개", action: #selector(rate2)),
                makeCommand(UIKeyCommand.f3, title: "별점 3개", action: #selector(rate3)),
                makeCommand(UIKeyCommand.f4, title: "별점 4개", action: #selector(rate4)),
                makeCommand(UIKeyCommand.f5, title: "별점 5개", action: #selector(rate5)),
                makeCommand(UIKeyCommand.f6, title: "별점 지우기", action: #selector(rate0)),
                makeCommand("1", title: nil, action: #selector(rate1)),
                makeCommand("2", title: nil, action: #selector(rate2)),
                makeCommand("3", title: nil, action: #selector(rate3)),
                makeCommand("4", title: nil, action: #selector(rate4)),
                makeCommand("5", title: nil, action: #selector(rate5)),
                makeCommand("0", title: nil, action: #selector(rate0)),
                makeCommand("p", title: "픽 (A컷)", action: #selector(pick)),
                makeCommand("x", title: "거절 (B컷)", action: #selector(reject)),
                makeCommand("u", title: "픽/거절 해제", action: #selector(clearFlag)),
                makeCommand("z", title: "100% 확대 전환", action: #selector(toggleZoom)),
                makeCommand("z", modifiers: .command, title: "실행 취소", action: #selector(undoLast)),
                makeCommand("g", title: "구도 표시", action: #selector(toggleOverlay)),
                makeCommand("r", title: "자동 수평", action: #selector(autoStraighten)),
                makeCommand(UIKeyCommand.inputLeftArrow, title: "이전 사진", action: #selector(prevPhoto)),
                makeCommand(UIKeyCommand.inputRightArrow, title: "다음 사진", action: #selector(nextPhoto)),
                makeCommand(" ", title: "다음 사진 (두 번: 확대)", action: #selector(spacePressed)),
                makeCommand(UIKeyCommand.inputDelete, title: "휴지통으로 옮기기", action: #selector(deletePhoto)),
                makeCommand("\u{8}", title: nil, action: #selector(deletePhoto)),
                makeCommand(UIKeyCommand.inputEscape, title: "닫기", action: #selector(closeViewer)),
            ]
        }

        private func makeCommand(
            _ input: String,
            modifiers: UIKeyModifierFlags = [],
            title: String?,
            action: Selector
        ) -> UIKeyCommand {
            let cmd = UIKeyCommand(input: input, modifierFlags: modifiers, action: action)
            cmd.discoverabilityTitle = title
            cmd.wantsPriorityOverSystemBehavior = true
            return cmd
        }

        @objc private func rate1() { callbacks?.onRate(1) }
        @objc private func rate2() { callbacks?.onRate(2) }
        @objc private func rate3() { callbacks?.onRate(3) }
        @objc private func rate4() { callbacks?.onRate(4) }
        @objc private func rate5() { callbacks?.onRate(5) }
        @objc private func rate0() { callbacks?.onRate(0) }
        @objc private func pick() { callbacks?.onPick() }
        @objc private func reject() { callbacks?.onReject() }
        @objc private func clearFlag() { callbacks?.onClearFlag() }
        @objc private func toggleZoom() { callbacks?.onToggleZoom() }
        @objc private func undoLast() { callbacks?.onUndo() }
        @objc private func toggleOverlay() { callbacks?.onToggleOverlay() }
        @objc private func autoStraighten() { callbacks?.onAutoStraighten() }
        @objc private func prevPhoto() { callbacks?.onPrevious() }
        @objc private func nextPhoto() { callbacks?.onNext() }
        @objc private func spacePressed() { callbacks?.onSpace() }
        @objc private func deletePhoto() { callbacks?.onDelete() }
        @objc private func closeViewer() { callbacks?.onClose() }
    }
}
