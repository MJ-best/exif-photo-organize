import Foundation

/// Production `PhotoEngineProtocol` backed by the Rust `photoorg-ios` UniFFI static library.
///
/// Blocking FFI calls are isolated onto dedicated GCD queues (`.userInitiated` for fast reads,
/// `.utility` for long scan/import/analysis/export batches) so Swift Concurrency's cooperative
/// thread pool is never starved by blocking disk or ONNX work.
final class RustPhotoEngine: PhotoEngineProtocol, @unchecked Sendable {
    let rootPath: String

    private let lib: PhotoLibrary
    private let fastQueue = DispatchQueue(
        label: "io.github.mj-best.loupe.ffi.fast",
        qos: .userInitiated,
        attributes: .concurrent
    )
    private let heavyQueue = DispatchQueue(
        label: "io.github.mj-best.loupe.ffi.heavy",
        qos: .utility,
        attributes: .concurrent
    )

    init(rootPath: String) throws {
        self.rootPath = rootPath
        do {
            self.lib = try PhotoLibrary.`open`(root: rootPath)
        } catch {
            throw Self.mapError(error)
        }
    }

    // MARK: - Lifecycle & Memory

    func suspend() async throws {
        try await runThrowing(on: fastQueue) { try self.lib.suspend() }
    }

    func resume() async throws {
        try await runThrowing(on: fastQueue) { try self.lib.resume() }
    }

    func releaseMLModels() async {
        await run(on: heavyQueue) { releaseMlModels() }
    }

    // MARK: - Snapshots & Windowed Paging

    func snapshot(view: LoupeViewFilter) async throws -> LoupeSnapshot {
        let ffiView = Self.toFfiView(view)
        let raw = try await runThrowing(on: fastQueue) { try self.lib.snapshot(view: ffiView) }
        return LoupeSnapshot(
            view: Self.fromFfiView(raw.view),
            ids: raw.ids,
            counts: LoupeCounts(
                all: Int(raw.counts.all),
                undecided: Int(raw.counts.undecided),
                candidates: Int(raw.counts.candidates),
                keepers: Int(raw.counts.keepers),
                rejected: Int(raw.counts.rejected),
                picked: Int(raw.counts.picked),
                rated: Int(raw.counts.rated),
                analysed: Int(raw.counts.analysed)
            ),
            learn: Self.fromFfiLearn(raw.learn),
            revision: raw.revision
        )
    }

    func photosBatch(ids: [Int64]) async throws -> [LoupePhotoSummary] {
        let raw = try await runThrowing(on: fastQueue) { try self.lib.photosBatch(ids: ids) }
        return raw.map(Self.fromFfiSummary)
    }

    func photoDetail(id: Int64) async throws -> LoupePhotoDetail? {
        guard let raw = try await runThrowing(on: fastQueue, { try self.lib.photoDetail(id: id) }) else {
            return nil
        }
        return LoupePhotoDetail(
            summary: Self.fromFfiSummary(raw.summary),
            reasons: raw.reasons,
            strengths: raw.strengths,
            scene: raw.scene,
            framing: raw.framing,
            compNotes: raw.compNotes.map {
                LoupeCompNote(text: $0.text, ok: $0.ok, ignored: $0.ignored)
            },
            overlay: raw.overlay.map {
                LoupeCompositionOverlay(
                    hasSubject: $0.hasSubject,
                    subjectX: $0.subjectX,
                    subjectY: $0.subjectY,
                    subjectW: $0.subjectW,
                    subjectH: $0.subjectH,
                    keyX: $0.keyX,
                    keyY: $0.keyY,
                    headTop: $0.headTop,
                    facing: $0.facing,
                    pole: $0.pole,
                    poleX: $0.poleX,
                    neckLine: $0.neckLine,
                    neckY: $0.neckY,
                    tilt: $0.tilt,
                    tiltConf: $0.tiltConf,
                    tiltVertical: $0.tiltVertical,
                    tiltAt: $0.tiltAt,
                    showThirds: $0.showThirds,
                    showHeadroom: $0.showHeadroom,
                    showLeadRoom: $0.showLeadRoom,
                    showMerger: $0.showMerger,
                    showTilt: $0.showTilt
                )
            },
            camera: raw.camera,
            lens: raw.lens,
            exposure: raw.exposure
        )
    }

    // MARK: - Zero-Reencode Media Loading

    func thumbnailData(id: Int64) async throws -> Data? {
        try await runThrowing(on: fastQueue) { try self.lib.thumbnail(id: id) }
    }

    func previewSource(id: Int64, maxEdge: Int) async throws -> LoupePreviewSource {
        let desc = try await runThrowing(on: fastQueue) {
            try self.lib.previewDescriptor(id: id, maxEdge: Int32(maxEdge))
        }
        switch desc {
        case let .embeddedSlice(path, offset, length, orientation):
            return .embeddedSlice(path: path, offset: offset, length: length, orientation: Int(orientation))
        case .renderedJpeg(let bytes):
            return .renderedJpeg(bytes)
        }
    }

    // MARK: - Culling Mutations & Settings

    func setRating(ids: [Int64], rating: Int) async throws -> Int64 {
        try await runThrowing(on: fastQueue) { try self.lib.setRating(ids: ids, rating: Int32(rating)) }
    }

    func setFlag(ids: [Int64], flag: Int) async throws -> Int64 {
        try await runThrowing(on: fastQueue) { try self.lib.setFlag(ids: ids, flag: Int32(flag)) }
    }

    func threshold() async -> Int {
        await run(on: fastQueue) { Int(self.lib.threshold()) }
    }

    func setThreshold(_ percent: Int) async {
        await run(on: fastQueue) { self.lib.setThreshold(percent: Int32(percent)) }
    }

    func keepPct() async -> Int {
        await run(on: fastQueue) { Int(self.lib.keepPct()) }
    }

    func setKeepPct(_ percent: Int) async {
        await run(on: fastQueue) { self.lib.setKeepPct(percent: Int32(percent)) }
    }

    func disabledRules() async -> Int64 {
        await run(on: fastQueue) { self.lib.disabledRules() }
    }

    func setDisabledRules(_ mask: Int64) async {
        await run(on: fastQueue) { self.lib.setDisabledRules(off: mask) }
    }

    func rules() async -> [LoupeRuleItem] {
        let raw = await run(on: fastQueue) { self.lib.rules() }
        return raw.map { LoupeRuleItem(bit: $0.bit, group: $0.group, label: $0.label, enabled: $0.enabled) }
    }

    func autoFrame(id: Int64, compose: Bool) async throws -> String {
        try await runThrowing(on: fastQueue) { try self.lib.autoFrame(id: id, compose: compose) }
    }

    func resetFraming(id: Int64) async throws {
        try await runThrowing(on: fastQueue) { try self.lib.resetFraming(id: id) }
    }

    // MARK: - Import, Analysis & Export

    func scanDirectory(path: String, label: String, includeVideos: Bool) async throws -> LoupeScanInfo {
        let raw = try await runThrowing(on: heavyQueue) {
            try self.lib.scanDirectory(path: path, label: label, includeVideos: includeVideos)
        }
        return LoupeScanInfo(
            label: raw.label,
            photos: Int(raw.photos),
            newPhotos: Int(raw.newPhotos),
            newFiles: Int(raw.newFiles),
            newBytes: raw.newBytes,
            videos: Int(raw.videos),
            rawPairs: Int(raw.rawPairs),
            firstMs: raw.firstMs,
            lastMs: raw.lastMs,
            cameras: raw.cameras,
            errors: raw.errors
        )
    }

    func importScanned(
        availableBytes: Int64,
        onProgress: @Sendable @escaping (LoupeImportStatus) -> Void
    ) async throws -> LoupeImportResult {
        let bridge = ImportListenerBridge(onProgress: onProgress)
        let raw = try await runThrowing(on: heavyQueue) {
            try self.lib.importScanned(availableBytes: availableBytes, listener: bridge)
        }
        return LoupeImportResult(
            added: Int(raw.added),
            updated: Int(raw.updated),
            duplicates: Int(raw.duplicates),
            bytes: raw.bytes,
            failures: raw.failures,
            cancelled: raw.cancelled
        )
    }

    func cancelImport() async {
        lib.cancelImport()
    }

    func pendingAnalysis() async throws -> Int {
        Int(try await runThrowing(on: fastQueue) { try self.lib.pendingAnalysis() })
    }

    func analyze(
        maxThreads: Int,
        onProgress: @Sendable @escaping (Int, Int) -> Void
    ) async throws -> Int {
        let bridge = ProgressListenerBridge(onProgress: onProgress)
        let done = try await runThrowing(on: heavyQueue) {
            try self.lib.analyze(maxThreads: Int32(maxThreads), listener: bridge)
        }
        return Int(done)
    }

    func cancelAnalysis() async {
        lib.cancelAnalysis()
    }

    func learn() async throws -> LoupeLearnStatus {
        let raw = try await runThrowing(on: heavyQueue) { try self.lib.learn() }
        return Self.fromFfiLearn(raw)
    }

    func exportPlan(ids: [Int64]) async throws -> LoupeExportPlan {
        let raw = try await runThrowing(on: fastQueue) { try self.lib.exportPlan(ids: ids) }
        return LoupeExportPlan(
            photos: Int(raw.photos),
            originalsBytes: raw.originalsBytes,
            jpegBytes: raw.jpegBytes,
            rawOnly: Int(raw.rawOnly),
            videos: Int(raw.videos),
            framedCount: Int(raw.framedCount)
        )
    }

    func exportPhotos(
        ids: [Int64],
        destDir: String,
        mode: LoupeExportMode,
        longEdge: Int,
        quality: Int,
        dateNames: Bool,
        onProgress: @Sendable @escaping (Int, Int) -> Void
    ) async throws -> LoupeExportResult {
        let bridge = ProgressListenerBridge(onProgress: onProgress)
        let ffiMode = Self.toFfiExportMode(mode)
        let raw = try await runThrowing(on: heavyQueue) {
            try self.lib.exportPhotos(
                ids: ids,
                destDir: destDir,
                mode: ffiMode,
                longEdge: Int32(longEdge),
                quality: Int32(quality),
                dateNames: dateNames,
                listener: bridge
            )
        }
        return LoupeExportResult(
            files: Int(raw.files),
            fromRaw: Int(raw.fromRaw),
            bytes: raw.bytes,
            skipped: raw.skipped,
            cancelled: raw.cancelled
        )
    }

    func cancelExport() async {
        lib.cancelExport()
    }

    // MARK: - Trash

    func deletePlan(ids: [Int64]) async throws -> LoupeDeletePlan {
        let raw = try await runThrowing(on: fastQueue) { try self.lib.deletePlan(ids: ids) }
        return LoupeDeletePlan(photos: Int(raw.photos), files: Int(raw.files), bytes: raw.bytes)
    }

    func deletePhotos(ids: [Int64]) async throws -> LoupeDeleteResult {
        let raw = try await runThrowing(on: fastQueue) { try self.lib.deletePhotos(ids: ids) }
        return LoupeDeleteResult(
            deleted: Int(raw.deleted),
            failed: Int(raw.failed),
            bytes: raw.bytes,
            firstError: raw.firstError
        )
    }

    func trashBytes() async -> Int64 {
        await run(on: fastQueue) { self.lib.trashBytes() }
    }

    func emptyTrash() async throws {
        try await runThrowing(on: heavyQueue) { try self.lib.emptyTrash() }
    }

    // MARK: - Queue Bridging & Type Mapping

    private func run<T: Sendable>(on queue: DispatchQueue, _ work: @escaping @Sendable () -> T) async -> T {
        await withCheckedContinuation { continuation in
            queue.async {
                continuation.resume(returning: work())
            }
        }
    }

    private func runThrowing<T: Sendable>(
        on queue: DispatchQueue,
        _ work: @escaping @Sendable () throws -> T
    ) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                do {
                    continuation.resume(returning: try work())
                } catch {
                    continuation.resume(throwing: Self.mapError(error))
                }
            }
        }
    }

    private static func toFfiView(_ view: LoupeViewFilter) -> LibraryView {
        switch view {
        case .all: return .all
        case .undecided: return .undecided
        case .candidates: return .candidates
        case .keepers: return .keepers
        case .rejected: return .rejected
        case .picked: return .picked
        case .rated: return .rated
        }
    }

    private static func fromFfiView(_ view: LibraryView) -> LoupeViewFilter {
        switch view {
        case .all: return .all
        case .undecided: return .undecided
        case .candidates: return .candidates
        case .keepers: return .keepers
        case .rejected: return .rejected
        case .picked: return .picked
        case .rated: return .rated
        }
    }

    private static func toFfiExportMode(_ mode: LoupeExportMode) -> ExportMode {
        switch mode {
        case .originals: return .originals
        case .jpegOnly: return .jpegOnly
        case .resized: return .resized
        }
    }

    private static func fromFfiSummary(_ raw: PhotoSummary) -> LoupePhotoSummary {
        LoupePhotoSummary(
            id: raw.id,
            name: raw.name,
            capturedMs: raw.capturedMs,
            rating: Int(raw.rating),
            flag: Int(raw.flag),
            bPct: raw.bPct.map(Int.init),
            topPct: raw.topPct.map(Int.init),
            burstRank: Int(raw.burstRank),
            burstSize: Int(raw.burstSize),
            hasFraming: raw.hasFraming,
            video: raw.video,
            raw: raw.raw,
            jpeg: raw.jpeg,
            width: Int(raw.width),
            height: Int(raw.height)
        )
    }

    private static func fromFfiLearn(_ raw: LearnStatus) -> LoupeLearnStatus {
        LoupeLearnStatus(
            summary: raw.summary,
            learned: raw.learned,
            a: Int(raw.a),
            b: Int(raw.b),
            analysed: Int(raw.analysed),
            total: Int(raw.total)
        )
    }

    private static func mapError(_ error: Error) -> LoupeError {
        if let loupe = error as? LoupeError {
            return loupe
        }
        guard let ffi = error as? PhotoError else {
            return .other(error.localizedDescription)
        }
        switch ffi {
        case .LibrarySuspended:
            return .librarySuspended
        case .NotFound(let detail):
            return .notFound(detail)
        case .PermissionDenied(let detail):
            return .permissionDenied(detail)
        case let .InsufficientStorage(requiredBytes, availableBytes):
            return .insufficientStorage(requiredBytes: requiredBytes, availableBytes: availableBytes)
        case .NotAnalyzed:
            return .notAnalyzed
        case .UnsupportedForVideo:
            return .unsupportedForVideo
        case .Cancelled:
            return .cancelled
        case .Database(let detail):
            return .database(detail)
        case .Io(let detail):
            return .io(detail)
        case .Other(let detail):
            return .other(detail)
        }
    }
}

private final class ImportListenerBridge: ImportListener, @unchecked Sendable {
    let onProgress: @Sendable (LoupeImportStatus) -> Void

    init(onProgress: @escaping @Sendable (LoupeImportStatus) -> Void) {
        self.onProgress = onProgress
    }

    func progress(status: ImportStatus) {
        onProgress(
            LoupeImportStatus(
                filesDone: Int(status.filesDone),
                filesTotal: Int(status.filesTotal),
                bytesDone: status.bytesDone,
                bytesTotal: status.bytesTotal,
                current: status.current
            )
        )
    }
}

private final class ProgressListenerBridge: ProgressListener, @unchecked Sendable {
    let onProgress: @Sendable (Int, Int) -> Void

    init(onProgress: @escaping @Sendable (Int, Int) -> Void) {
        self.onProgress = onProgress
    }

    func progress(done: Int32, total: Int32) {
        onProgress(Int(done), Int(total))
    }
}
