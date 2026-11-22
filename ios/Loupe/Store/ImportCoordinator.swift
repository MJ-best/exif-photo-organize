import Foundation
import Observation

/// Dedicated domain coordinator for SD card / USB-C drive scanning and checksum-verified import.
/// Manages security-scoped folder access, screen wake lock (`IdleTimerManager`), and iOS background
/// task assertions (`BackgroundTaskGuard`) independently of `LibraryStore`.
@MainActor
@Observable
final class ImportCoordinator {
    var sheetPresented: Bool = false
    private(set) var phase: ImportPhase = .idle

    private let engineProvider: () -> (any PhotoEngineProtocol)?
    private let onImportCompleted: () async -> Void
    private let onPresentError: (LoupeError) -> Void
    private let idleTimer: IdleTimerManager

    private var activeImportAccess: SecurityScopedAccess?
    private var lastImportURL: URL?
    private var importTask: Task<Void, Never>?

    init(
        engineProvider: @escaping () -> (any PhotoEngineProtocol)?,
        onImportCompleted: @escaping () async -> Void,
        onPresentError: @escaping (LoupeError) -> Void,
        idleTimer: IdleTimerManager? = nil
    ) {
        self.engineProvider = engineProvider
        self.onImportCompleted = onImportCompleted
        self.onPresentError = onPresentError
        self.idleTimer = idleTimer ?? .shared
    }

    func open() {
        if case .done = phase { phase = .idle }
        if case .failed = phase { phase = .idle }
        sheetPresented = true
    }

    func scanCardFolder(url: URL, includeVideos: Bool = true) {
        guard let engine = engineProvider() else { return }
        lastImportURL = url
        let access = SecurityScopedAccess(url: url)
        activeImportAccess = access
        let label = url.lastPathComponent
        phase = .scanning(label: label)

        importTask?.cancel()
        importTask = Task { [weak self] in
            guard let self else { return }
            do {
                let info = try await engine.scanDirectory(path: url.path, label: label, includeVideos: includeVideos)
                guard !Task.isCancelled else { return }
                let destURL = URL(fileURLWithPath: engine.rootPath)
                let freeBytes = BookmarkStore.availableCapacityBytes(at: destURL)
                self.phase = .ready(info: info, includeVideos: includeVideos, freeBytes: freeBytes)
            } catch {
                guard !Task.isCancelled else { return }
                let mapped = Self.mapError(error)
                self.phase = .failed(mapped)
            }
        }
    }

    func setIncludeVideos(_ includeVideos: Bool) {
        if let url = lastImportURL {
            scanCardFolder(url: url, includeVideos: includeVideos)
        }
    }

    func startImport() {
        guard let engine = engineProvider(), case let .ready(info, _, freeBytes) = phase else { return }
        phase = .copying(
            status: LoupeImportStatus(filesDone: 0, filesTotal: info.newFiles, bytesDone: 0, bytesTotal: info.newBytes, current: ""),
            label: info.label
        )
        let wakeToken = idleTimer.acquire(reason: "sd-import")

        importTask = Task { [weak self] in
            guard let self else {
                wakeToken.release()
                return
            }
            defer {
                wakeToken.release()
                self.activeImportAccess = nil
            }
            await BackgroundTaskGuard.run(
                name: "io.github.mj-best.loupe.import",
                onExpire: { [weak self] in
                    self?.cancel()
                }
            ) {
                do {
                    let result = try await engine.importScanned(availableBytes: freeBytes) { [weak self] status in
                        Task { @MainActor [weak self] in
                            guard let self, case .copying(_, let lbl) = self.phase else { return }
                            self.phase = .copying(status: status, label: lbl)
                        }
                    }
                    guard !Task.isCancelled else { return }
                    self.phase = .done(result)
                    LoupeLog.importer.info("Import finished: added=\(result.added) dup=\(result.duplicates) cancelled=\(result.cancelled)")
                    await self.onImportCompleted()
                } catch {
                    guard !Task.isCancelled else { return }
                    let mapped = Self.mapError(error)
                    self.phase = .failed(mapped)
                    if case .insufficientStorage = mapped {
                        self.onPresentError(mapped)
                    }
                }
            }
        }
    }

    func cancel() {
        Task { [weak self] in
            await self?.engineProvider()?.cancelImport()
        }
    }

    func resetForLibrarySwitch() {
        importTask?.cancel()
        importTask = nil
        activeImportAccess = nil
        lastImportURL = nil
        phase = .idle
    }

    private static func mapError(_ error: Error) -> LoupeError {
        if let loupe = error as? LoupeError { return loupe }
        return .other(error.localizedDescription)
    }
}
