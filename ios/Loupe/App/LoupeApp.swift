import BackgroundTasks
import SwiftUI
import UIKit

@main
struct LoupeApp: App {
    static let backgroundAnalysisTaskID = "io.github.mj-best.loupe.analyze"

    @State private var store: LibraryStore
    @Environment(\.scenePhase) private var scenePhase

    init() {
        let store = LibraryStore { path in
            try RustPhotoEngine(rootPath: path)
        }
        _store = State(initialValue: store)
        Self.registerBackgroundTasks(store: store)
    }

    var body: some Scene {
        WindowGroup {
            LoupeRootContainer(store: store)
                .task {
                    await store.bootstrap()
                }
                .onChange(of: scenePhase) { _, newPhase in
                    Task { @MainActor in
                        await store.handleScenePhase(newPhase)
                        if newPhase == .background {
                            Self.scheduleBackgroundAnalysisIfPending(store: store)
                        }
                    }
                }
                .onReceive(NotificationCenter.default.publisher(for: UIApplication.didReceiveMemoryWarningNotification)) { _ in
                    Task { @MainActor in
                        await store.handleMemoryWarning()
                    }
                }
        }
    }

    /// Registers a charging-only `BGProcessingTask` so large overnight SD imports finish culling
    /// analysis cleanly without draining battery or risking background Jetsam.
    private static func registerBackgroundTasks(store: LibraryStore) {
        BGTaskScheduler.shared.register(
            forTaskWithIdentifier: backgroundAnalysisTaskID,
            using: nil
        ) { task in
            guard let processingTask = task as? BGProcessingTask else {
                task.setTaskCompleted(success: false)
                return
            }
            let completionGate = TaskCompletionGate()

            let work = Task {
                guard let engine = await MainActor.run(body: { store.engine }) else {
                    if completionGate.tryComplete() {
                        processingTask.setTaskCompleted(success: false)
                    }
                    return
                }
                do {
                    try await engine.resume()
                    let pending = try await engine.pendingAnalysis()
                    if pending > 0 {
                        let done = try await engine.analyze(maxThreads: 1) { _, _ in }
                        if done >= pending {
                            await MainActor.run { store.analyzer.markBackgroundAnalysisCompleted() }
                        }
                    } else {
                        await MainActor.run { store.analyzer.markBackgroundAnalysisCompleted() }
                    }
                    try await engine.suspend()
                    await engine.releaseMLModels()
                    if completionGate.tryComplete() {
                        processingTask.setTaskCompleted(success: true)
                    }
                } catch {
                    try? await engine.suspend()
                    await engine.releaseMLModels()
                    if completionGate.tryComplete() {
                        processingTask.setTaskCompleted(success: false)
                    }
                }
            }

            processingTask.expirationHandler = {
                work.cancel()
                Task {
                    if let engine = await MainActor.run(body: { store.engine }) {
                        await engine.cancelAnalysis()
                        try? await engine.suspend()
                        await engine.releaseMLModels()
                    }
                    if completionGate.tryComplete() {
                        processingTask.setTaskCompleted(success: false)
                    }
                }
            }
        }
    }

    @MainActor
    private static func scheduleBackgroundAnalysisIfPending(store: LibraryStore) {
        guard store.analyzer.hasPendingBackgroundAnalysis else { return }
        let request = BGProcessingTaskRequest(identifier: backgroundAnalysisTaskID)
        request.requiresExternalPower = true
        request.requiresNetworkConnectivity = false
        do {
            try BGTaskScheduler.shared.submit(request)
            LoupeLog.lifecycle.info("Scheduled charging-only BGProcessingTask for pending analysis")
        } catch {
            LoupeLog.lifecycle.debug("BGProcessingTask submit skipped: \(error.localizedDescription)")
        }
    }
}

/// Ensures `BGTask.setTaskCompleted(success:)` is invoked at most once even if expiration races
/// with normal task completion.
private final class TaskCompletionGate: @unchecked Sendable {
    private let lock = NSLock()
    private var completed = false

    func tryComplete() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard !completed else { return false }
        completed = true
        return true
    }
}

private struct LoupeRootContainer: View {
    @Bindable var store: LibraryStore
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        RootView(store: store)
            .environment(\.loupePalette, LoupePalette.forColorScheme(colorScheme))
    }
}
