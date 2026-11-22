import Foundation
import UIKit

/// Safe, idempotent wrapper around `UIApplication.beginBackgroundTask` so critical SQLite WAL
/// checkpointing (`suspend()`), SD card imports, and photo exports finish cleanly when the user
/// locks the screen or switches apps.
final class BackgroundTaskGuard: @unchecked Sendable {
    private let lock = NSLock()
    private var identifier: UIBackgroundTaskIdentifier = .invalid
    private let name: String

    @MainActor
    init(name: String, onExpire: (@MainActor @Sendable () -> Void)? = nil) {
        self.name = name
        let taskName = name
        let assigned = UIApplication.shared.beginBackgroundTask(withName: taskName) { [weak self] in
            LoupeLog.lifecycle.warning("Background task expiring: \(taskName)")
            Task { @MainActor in
                onExpire?()
                self?.end()
            }
        }
        lock.lock()
        self.identifier = assigned
        lock.unlock()
    }

    @MainActor
    func end() {
        let toEnd: UIBackgroundTaskIdentifier
        lock.lock()
        toEnd = identifier
        identifier = .invalid
        lock.unlock()
        if toEnd != .invalid {
            UIApplication.shared.endBackgroundTask(toEnd)
        }
    }

    deinit {
        lock.lock()
        let toEnd = identifier
        identifier = .invalid
        lock.unlock()
        if toEnd != .invalid {
            DispatchQueue.main.async {
                UIApplication.shared.endBackgroundTask(toEnd)
            }
        }
    }

    @MainActor
    static func run<T>(
        name: String,
        onExpire: (@MainActor @Sendable () -> Void)? = nil,
        _ operation: @MainActor () async throws -> T
    ) async rethrows -> T {
        let handle = BackgroundTaskGuard(name: name, onExpire: onExpire)
        defer { handle.end() }
        return try await operation()
    }
}

/// Reference-counted screen wake lock (`UIApplication.shared.isIdleTimerDisabled`) so long SD card
/// imports and large exports are not interrupted by auto-lock, without overlapping tasks
/// prematurely re-enabling auto-lock.
@MainActor
final class IdleTimerManager {
    static let shared = IdleTimerManager()

    private(set) var activeCount: Int = 0
    private let applyDisabled: @MainActor (Bool) -> Void

    init(applyDisabled: (@MainActor (Bool) -> Void)? = nil) {
        self.applyDisabled = applyDisabled ?? { UIApplication.shared.isIdleTimerDisabled = $0 }
    }

    func acquire(reason: String) -> Token {
        activeCount += 1
        if activeCount == 1 {
            applyDisabled(true)
            LoupeLog.lifecycle.info("Idle timer disabled (\(reason))")
        }
        return Token(manager: self, reason: reason)
    }

    fileprivate func release(reason: String) {
        guard activeCount > 0 else { return }
        activeCount -= 1
        if activeCount == 0 {
            applyDisabled(false)
            LoupeLog.lifecycle.info("Idle timer restored (\(reason))")
        }
    }

    final class Token {
        private weak var manager: IdleTimerManager?
        private let reason: String
        private var released = false

        fileprivate init(manager: IdleTimerManager, reason: String) {
            self.manager = manager
            self.reason = reason
        }

        @MainActor
        func release() {
            guard !released else { return }
            released = true
            manager?.release(reason: reason)
        }

        deinit {
            if !released, let mgr = manager {
                let r = reason
                Task { @MainActor in
                    mgr.release(reason: r)
                }
            }
        }
    }
}
