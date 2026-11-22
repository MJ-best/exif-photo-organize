import Foundation
import Observation

/// Dedicated domain coordinator for JPEG / resized (GPS-stripped) / RAW+XMP export staging.
/// Manages temporary staging directory cleanup, screen wake lock, and background task protection.
@MainActor
@Observable
final class ExportCoordinator {
    var sheetPresented: Bool = false
    private(set) var phase: ExportPhase = .idle

    private let engineProvider: () -> (any PhotoEngineProtocol)?
    private let idleTimer: IdleTimerManager

    private var exportTask: Task<Void, Never>?
    private var stagedDirectory: URL?

    init(
        engineProvider: @escaping () -> (any PhotoEngineProtocol)?,
        idleTimer: IdleTimerManager = .shared
    ) {
        self.engineProvider = engineProvider
        self.idleTimer = idleTimer
    }

    func open(for ids: [Int64]) {
        guard let engine = engineProvider(), !ids.isEmpty else { return }
        cleanupTempDirectory()
        sheetPresented = true
        phase = .planning
        exportTask?.cancel()
        exportTask = Task { [weak self] in
            guard let self else { return }
            do {
                let plan = try await engine.exportPlan(ids: ids)
                guard !Task.isCancelled else { return }
                self.phase = .ready(ids: ids, plan: plan)
            } catch {
                guard !Task.isCancelled else { return }
                self.phase = .failed(Self.mapError(error))
            }
        }
    }

    func run(mode: LoupeExportMode, longEdge: Int, quality: Int, dateNames: Bool) {
        guard let engine = engineProvider(), case let .ready(ids, _) = phase else { return }
        cleanupTempDirectory()

        let outDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("loupe-export-\(UUID().uuidString)", isDirectory: true)
        stagedDirectory = outDir
        phase = .running(done: 0, total: ids.count)

        let wakeToken = idleTimer.acquire(reason: "photo-export")
        exportTask = Task { [weak self] in
            guard let self else {
                wakeToken.release()
                return
            }
            defer { wakeToken.release() }
            await BackgroundTaskGuard.run(
                name: "io.github.mj-best.loupe.export",
                onExpire: { [weak self] in
                    self?.cancel()
                }
            ) {
                do {
                    let result = try await engine.exportPhotos(
                        ids: ids,
                        destDir: outDir.path,
                        mode: mode,
                        longEdge: longEdge,
                        quality: quality,
                        dateNames: dateNames
                    ) { [weak self] done, total in
                        Task { @MainActor [weak self] in
                            guard let self, case .running = self.phase else { return }
                            self.phase = .running(done: done, total: total)
                        }
                    }
                    guard !Task.isCancelled else {
                        self.cleanupTempDirectory()
                        return
                    }
                    if result.cancelled {
                        self.cleanupTempDirectory()
                    }
                    self.phase = .done(result: result, outputDirectory: outDir)
                    LoupeLog.exporter.info("Export finished: files=\(result.files) bytes=\(result.bytes) cancelled=\(result.cancelled)")
                } catch {
                    self.cleanupTempDirectory()
                    guard !Task.isCancelled else { return }
                    self.phase = .failed(Self.mapError(error))
                }
            }
        }
    }

    func cancel() {
        Task { [weak self] in
            await self?.engineProvider()?.cancelExport()
        }
    }

    func cleanupTempDirectory() {
        if let dir = stagedDirectory {
            try? FileManager.default.removeItem(at: dir)
            stagedDirectory = nil
        }
        if case let .done(_, outDir) = phase {
            try? FileManager.default.removeItem(at: outDir)
        }
    }

    func resetForLibrarySwitch() {
        exportTask?.cancel()
        exportTask = nil
        cleanupTempDirectory()
        phase = .idle
    }

    private static func mapError(_ error: Error) -> LoupeError {
        if let loupe = error as? LoupeError { return loupe }
        return .other(error.localizedDescription)
    }
}
