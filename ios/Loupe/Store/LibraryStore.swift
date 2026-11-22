import Foundation
import Observation
import SwiftUI

/// Single-source-of-truth unidirectional state machine for LOUPE on iOS & iPadOS.
///
/// Architectural guarantees:
/// 1. **No torn reads**: `visibleIDs`, `counts`, and `learn` always update atomically from a single
///    `LoupeSnapshot`, and stale out-of-order responses are discarded via `appliedRevision` +
///    request tokens.
/// 2. **Windowed FFI paging**: Only visible grid IDs (±40 window) are materialized into `summaries`,
///    preventing 20,000-photo main-thread serialization stalls.
/// 3. **Optimistic UI with rollback**: Rating/pick/reject mutates the visible cell in 0ms and rolls
///    back cleanly if the Rust catalog write fails.
/// 4. **Lifecycle safety**: Backgrounding cancels analysis, checkpoints & drops SQLite connections
///    (`0xdead10cc` prevention), and releases ~100MB+ ONNX runtime graphs.
@MainActor
@Observable
final class LibraryStore {
    // MARK: - Observable State
    private(set) var isReady: Bool = false
    private(set) var isSuspended: Bool = false
    private(set) var isExternalLibrary: Bool = false
    private(set) var libraryDisplayName: String = "나의 기기 · LOUPE"

    private(set) var activeView: LoupeViewFilter = .all
    private(set) var visibleIDs: [Int64] = []
    private(set) var summaries: [Int64: LoupePhotoSummary] = [:]
    private(set) var counts: LoupeCounts = .zero
    private(set) var learn: LoupeLearnStatus = .initial
    private(set) var rules: [LoupeRuleItem] = []

    private(set) var threshold: Int = 60
    private(set) var keepPct: Int = 15
    private(set) var trashBytes: Int64 = 0
    private(set) var imageReloadToken: Int = 0

    var selectedIDs: Set<Int64> = []
    var isSelectionMode: Bool = false
    var viewer: ViewerSession?
    private(set) var activeDetail: LoupePhotoDetail?
    var showCompositionOverlay: Bool = false
    var showInspector: Bool = true

    var importSheetPresented: Bool = false
    private(set) var importPhase: ImportPhase = .idle

    var exportSheetPresented: Bool = false
    private(set) var exportPhase: ExportPhase = .idle

    var settingsSheetPresented: Bool = false
    var deleteConfirmation: DeleteConfirmation?
    var recoveryAlert: RecoveryAlert?
    private(set) var toast: BottomToast?
    private(set) var analysisProgress: AnalysisProgress?

    // MARK: - Dependencies & Concurrency Guards
    private(set) var engine: (any PhotoEngineProtocol)?
    let images: ImagePipeline
    let thermalMonitor: ThermalBudgetMonitor
    private let bookmarkStore: BookmarkStore
    private let engineFactory: @Sendable (String) throws -> any PhotoEngineProtocol

    private var activeLibraryAccess: SecurityScopedAccess?
    private var activeImportAccess: SecurityScopedAccess?
    private var lastImportURL: URL?

    private var appliedRevision: Int64 = 0
    private var snapshotRequestToken: UInt64 = 0
    private var inFlightBatchIDs: Set<Int64> = []
    private var idToIndex: [Int64: Int] = [:]

    private var refreshTask: Task<Void, Never>?
    private var detailTask: Task<Void, Never>?
    private var learnTask: Task<Void, Never>?
    private var analysisTask: Task<Void, Never>?
    private var importTask: Task<Void, Never>?
    private var exportTask: Task<Void, Never>?
    private var toastTask: Task<Void, Never>?

    private let learnDebounceNanoseconds: UInt64

    init(
        engine: (any PhotoEngineProtocol)? = nil,
        bookmarkStore: BookmarkStore = BookmarkStore(),
        thermalMonitor: ThermalBudgetMonitor = ThermalBudgetMonitor(),
        learnDebounceNanoseconds: UInt64 = 1_200_000_000,
        engineFactory: @escaping @Sendable (String) throws -> any PhotoEngineProtocol
    ) {
        self.engine = engine
        self.bookmarkStore = bookmarkStore
        self.thermalMonitor = thermalMonitor
        self.learnDebounceNanoseconds = learnDebounceNanoseconds
        self.engineFactory = engineFactory

        // Load persisted user thresholds.
        let savedThreshold = UserDefaults.standard.integer(forKey: "loupe.threshold")
        self.threshold = savedThreshold >= 30 ? savedThreshold : 60
        let savedKeepPct = UserDefaults.standard.integer(forKey: "loupe.keep_pct")
        self.keepPct = savedKeepPct >= 5 ? savedKeepPct : 15

        // Build ImagePipeline reading the current engine dynamically.
        let box = EngineWeakBox()
        self.images = ImagePipeline { box.engine }
        self.engineBox = box
        box.engine = engine

        self.thermalMonitor.onThermalChange = { [weak self] budget in
            Task { @MainActor [weak self] in
                await self?.handleThermalBudgetChange(budget)
            }
        }
    }

    private let engineBox: EngineWeakBox

    // MARK: - Lifecycle (Apple `0xdead10cc` & Memory Warning Protection)

    func bootstrap() async {
        if let existing = engine {
            await configureAndLoad(engine: existing, external: false, name: "LOUPE")
            return
        }

        // Prefer previously bookmarked external SSD library if connected, else default Documents/LOUPE.
        if let external = bookmarkStore.resolveExternalLibrary() {
            do {
                let opened = try engineFactory(external.url.path)
                activeLibraryAccess = external
                await configureAndLoad(engine: opened, external: true, name: external.url.lastPathComponent)
                return
            } catch {
                LoupeLog.lifecycle.warning("External library unavailable, falling back to on-device: \(error.localizedDescription)")
            }
        }

        do {
            let localURL = try bookmarkStore.defaultLibraryURL()
            let opened = try engineFactory(localURL.path)
            activeLibraryAccess = nil
            await configureAndLoad(engine: opened, external: false, name: "나의 기기 · LOUPE")
        } catch {
            presentError(Self.mapError(error))
        }
    }

    /// Switches the active library to a user-selected external SSD or Files.app folder.
    func switchToExternalLibrary(url: URL) async {
        do {
            try bookmarkStore.saveExternalLibrary(url)
            let access = SecurityScopedAccess(url: url)
            await suspendForSwitch()
            let opened = try engineFactory(url.path)
            activeLibraryAccess = access
            await configureAndLoad(engine: opened, external: true, name: url.lastPathComponent)
            showToast("'\(url.lastPathComponent)' 라이브러리를 열었습니다", tone: .positive)
        } catch {
            presentError(Self.mapError(error))
        }
    }

    func switchToDefaultLibrary() async {
        do {
            bookmarkStore.clearExternalLibrary()
            await suspendForSwitch()
            let localURL = try bookmarkStore.defaultLibraryURL()
            let opened = try engineFactory(localURL.path)
            activeLibraryAccess = nil
            await configureAndLoad(engine: opened, external: false, name: "나의 기기 · LOUPE")
            showToast("기기 내장 라이브러리로 전환했습니다", tone: .neutral)
        } catch {
            presentError(Self.mapError(error))
        }
    }

    /// Called when SwiftUI `scenePhase` enters `.background` or `.active`.
    func handleScenePhase(_ phase: ScenePhase) async {
        switch phase {
        case .background:
            guard !isSuspended, let engine else { return }
            LoupeLog.lifecycle.info("Entering background: cancelling tasks, suspending SQLite, releasing ML models")
            analysisTask?.cancel()
            learnTask?.cancel()
            await engine.cancelAnalysis()
            images.purgeMemory(includeThumbnails: false)
            do {
                try await engine.suspend()
                await engine.releaseMLModels()
                isSuspended = true
            } catch {
                LoupeLog.lifecycle.error("Suspend failed: \(error.localizedDescription)")
            }
        case .active:
            guard isSuspended, let engine else { return }
            LoupeLog.lifecycle.info("Returning to foreground: resuming SQLite and syncing snapshot")
            do {
                try await engine.resume()
                isSuspended = false
                await refreshSnapshot(force: true)
                startAnalysisIfNeeded()
            } catch {
                presentError(Self.mapError(error))
            }
        case .inactive:
            break
        @unknown default:
            break
        }
    }

    func handleMemoryWarning() async {
        LoupeLog.lifecycle.warning("Memory warning received: purging image caches and ONNX runtime graphs")
        images.purgeMemory(includeThumbnails: true)
        if let engine {
            await engine.releaseMLModels()
        }
    }

    private func suspendForSwitch() async {
        analysisTask?.cancel()
        learnTask?.cancel()
        importTask?.cancel()
        if let old = engine {
            await old.cancelAnalysis()
            try? await old.suspend()
            await old.releaseMLModels()
        }
        images.purgeMemory(includeThumbnails: true)
        summaries.removeAll()
        visibleIDs.removeAll()
        idToIndex.removeAll()
        selectedIDs.removeAll()
        viewer = nil
        activeDetail = nil
        appliedRevision = 0
    }

    private func configureAndLoad(engine: any PhotoEngineProtocol, external: Bool, name: String) async {
        self.engine = engine
        self.engineBox.engine = engine
        self.isExternalLibrary = external
        self.libraryDisplayName = name
        self.isSuspended = false

        let savedOff = (UserDefaults.standard.object(forKey: "loupe.disabled_rules") as? NSNumber)?.int64Value ?? 0
        await engine.setThreshold(threshold)
        await engine.setKeepPct(keepPct)
        await engine.setDisabledRules(savedOff)
        self.rules = await engine.rules()
        self.trashBytes = await engine.trashBytes()

        await refreshSnapshot(force: true)
        self.isReady = true
        startAnalysisIfNeeded()
    }

    // MARK: - View Navigation & Atomic Snapshot

    func selectView(_ view: LoupeViewFilter) {
        guard view != activeView else { return }
        activeView = view
        selectedIDs.removeAll()
        isSelectionMode = false
        refreshTask?.cancel()
        refreshTask = Task { [weak self] in
            await self?.refreshSnapshot(force: true)
        }
    }

    func refreshSnapshot(force: Bool = false) async {
        guard let engine, !isSuspended else { return }
        let view = activeView
        snapshotRequestToken &+= 1
        let token = snapshotRequestToken

        do {
            let snap = try await engine.snapshot(view: view)
            // Discard stale out-of-order responses.
            guard token == snapshotRequestToken, activeView == view else { return }
            guard force || snap.revision >= appliedRevision else { return }

            appliedRevision = snap.revision
            visibleIDs = snap.ids
            idToIndex = Dictionary(uniqueKeysWithValues: snap.ids.enumerated().map { ($1, $0) })
            counts = snap.counts
            learn = snap.learn

            // Prune summary cache entries no longer in the library and eagerly load first window.
            let validSet = Set(snap.ids)
            if summaries.count > 2000 {
                summaries = summaries.filter { validSet.contains($0.key) }
            }
            if !snap.ids.isEmpty {
                await loadWindow(around: 0, forceRefresh: true)
            }
            if let currentViewerID = viewer?.currentID {
                await loadActiveDetail(id: currentViewerID)
            }
        } catch {
            presentError(Self.mapError(error))
        }
    }

    /// O(1) entry point for `ForEach(store.visibleIDs, id: \.self)` cells without allocating
    /// `Array(visibleIDs.enumerated())` on every SwiftUI body evaluation.
    func ensureLoaded(for id: Int64) {
        guard let index = idToIndex[id] else { return }
        ensureLoaded(around: index)
    }

    /// Loads compact `PhotoSummary` records only for the visible grid window (±40 cells).
    func ensureLoaded(around index: Int) {
        Task { [weak self] in
            await self?.loadWindow(around: index, forceRefresh: false)
        }
    }

    private func loadWindow(around index: Int, forceRefresh: Bool) async {
        guard let engine, !isSuspended, !visibleIDs.isEmpty else { return }
        let start = max(0, index - 24)
        let end = min(visibleIDs.count - 1, index + 48)
        guard start <= end else { return }

        let targetIDs = visibleIDs[start...end].filter { id in
            if forceRefresh { return !inFlightBatchIDs.contains(id) }
            return summaries[id] == nil && !inFlightBatchIDs.contains(id)
        }
        guard !targetIDs.isEmpty else { return }

        inFlightBatchIDs.formUnion(targetIDs)
        defer { inFlightBatchIDs.subtract(targetIDs) }

        do {
            let fetched = try await engine.photosBatch(ids: targetIDs)
            for item in fetched {
                summaries[item.id] = item
            }
        } catch {
            LoupeLog.engine.debug("photosBatch failed: \(error.localizedDescription)")
        }
    }

    // MARK: - Viewer & Prefetching

    func openViewer(at id: Int64) {
        guard let idx = visibleIDs.firstIndex(of: id) else { return }
        viewer = ViewerSession(ids: visibleIDs, index: idx)
        loadActiveDetailAsync(id: id)
    }

    func viewerMoved(to index: Int, previewEdge: Int) {
        guard var session = viewer, index >= 0, index < session.ids.count else { return }
        guard session.index != index else { return }
        session.index = index
        viewer = session
        let id = session.ids[index]
        loadActiveDetailAsync(id: id)
        images.prefetchNeighbors(ids: session.ids, around: index, maxEdge: previewEdge)
    }

    func closeViewer() {
        viewer = nil
        activeDetail = nil
        images.cancelPrefetch()
        Task { [weak self] in
            await self?.refreshSnapshot(force: true)
        }
    }

    private func loadActiveDetailAsync(id: Int64) {
        detailTask?.cancel()
        detailTask = Task { [weak self] in
            await self?.loadActiveDetail(id: id)
        }
    }

    private func loadActiveDetail(id: Int64) async {
        guard let engine, !isSuspended else { return }
        do {
            let detail = try await engine.photoDetail(id: id)
            guard !Task.isCancelled else { return }
            if viewer?.currentID == id {
                activeDetail = detail
            }
            if let summary = detail?.summary {
                summaries[id] = summary
            }
        } catch {
            LoupeLog.engine.debug("photoDetail(\(id)) failed: \(error.localizedDescription)")
        }
    }

    // MARK: - Optimistic Culling Mutations (0ms Feedback + Rollback)

    func rate(ids: [Int64], rating: Int) {
        guard let engine, !ids.isEmpty else { return }
        let clamped = max(0, min(5, rating))

        // Optimistic update for instant 0ms response.
        var rollback: [Int64: Int] = [:]
        for id in ids {
            if let existing = summaries[id] {
                rollback[id] = existing.rating
                summaries[id]?.rating = clamped
            }
        }
        if let activeID = activeDetail?.summary.id, ids.contains(activeID) {
            activeDetail?.summary.rating = clamped
        }

        let toastText = clamped == 0 ? "별점 해제" : String(repeating: "★", count: clamped)
        showToast(toastText, tone: clamped > 0 ? .accent : .neutral)

        Task { [weak self] in
            guard let self else { return }
            do {
                let rev = try await engine.setRating(ids: ids, rating: clamped)
                self.appliedRevision = max(self.appliedRevision, rev)
                if self.viewer == nil {
                    await self.refreshSnapshot(force: true)
                }
                self.scheduleDebouncedLearn()
            } catch {
                // Roll back optimistic mutation on failure.
                for (id, prev) in rollback {
                    self.summaries[id]?.rating = prev
                }
                if let activeID = self.activeDetail?.summary.id, let prev = rollback[activeID] {
                    self.activeDetail?.summary.rating = prev
                }
                self.presentError(Self.mapError(error))
            }
        }
    }

    func flag(ids: [Int64], flag: Int) {
        guard let engine, !ids.isEmpty else { return }
        let normalized = flag > 0 ? 1 : (flag < 0 ? -1 : 0)

        var rollback: [Int64: Int] = [:]
        for id in ids {
            if let existing = summaries[id] {
                rollback[id] = existing.flag
                summaries[id]?.flag = normalized
            }
        }
        if let activeID = activeDetail?.summary.id, ids.contains(activeID) {
            activeDetail?.summary.flag = normalized
        }

        switch normalized {
        case 1: showToast("픽 — A컷", tone: .positive)
        case -1: showToast("거절 — B컷", tone: .negative)
        default: showToast("픽/거절 해제", tone: .neutral)
        }

        Task { [weak self] in
            guard let self else { return }
            do {
                let rev = try await engine.setFlag(ids: ids, flag: normalized)
                self.appliedRevision = max(self.appliedRevision, rev)
                if self.viewer == nil {
                    await self.refreshSnapshot(force: true)
                }
                self.scheduleDebouncedLearn()
            } catch {
                for (id, prev) in rollback {
                    self.summaries[id]?.flag = prev
                }
                if let activeID = self.activeDetail?.summary.id, let prev = rollback[activeID] {
                    self.activeDetail?.summary.flag = prev
                }
                self.presentError(Self.mapError(error))
            }
        }
    }

    private func scheduleDebouncedLearn() {
        guard let engine else { return }
        learnTask?.cancel()
        learnTask = Task { [weak self] in
            guard let self else { return }
            try? await Task.sleep(nanoseconds: self.learnDebounceNanoseconds)
            guard !Task.isCancelled, !self.isSuspended else { return }
            do {
                let status = try await engine.learn()
                guard !Task.isCancelled else { return }
                self.learn = status
                if self.viewer == nil {
                    await self.refreshSnapshot(force: true)
                } else if let currentID = self.viewer?.currentID {
                    await self.loadActiveDetail(id: currentID)
                }
            } catch {
                LoupeLog.cull.debug("Debounced learn failed: \(error.localizedDescription)")
            }
        }
    }

    // MARK: - Selection

    func toggleSelection(id: Int64) {
        if selectedIDs.contains(id) {
            selectedIDs.remove(id)
        } else {
            selectedIDs.insert(id)
        }
        if selectedIDs.isEmpty {
            isSelectionMode = false
        }
    }

    func selectAllVisible() {
        selectedIDs = Set(visibleIDs)
        isSelectionMode = !selectedIDs.isEmpty
    }

    func clearSelection() {
        selectedIDs.removeAll()
        isSelectionMode = false
    }

    // MARK: - Culling Settings & Non-Destructive Framing

    func updateThreshold(_ value: Int) {
        let clamped = max(30, min(90, value))
        threshold = clamped
        UserDefaults.standard.set(clamped, forKey: "loupe.threshold")
        Task { [weak self] in
            await self?.engine?.setThreshold(clamped)
            await self?.refreshSnapshot(force: true)
        }
    }

    func updateKeepPct(_ value: Int) {
        let clamped = max(5, min(50, value))
        keepPct = clamped
        UserDefaults.standard.set(clamped, forKey: "loupe.keep_pct")
        Task { [weak self] in
            await self?.engine?.setKeepPct(clamped)
            await self?.refreshSnapshot(force: true)
        }
    }

    func toggleRule(bit: Int64) {
        guard let engine else { return }
        Task { [weak self] in
            guard let self else { return }
            let next = (await engine.disabledRules()) ^ bit
            UserDefaults.standard.set(NSNumber(value: next), forKey: "loupe.disabled_rules")
            await engine.setDisabledRules(next)
            self.rules = await engine.rules()
            _ = try? await engine.learn()
            await self.refreshSnapshot(force: true)
        }
    }

    func autoFrame(id: Int64, compose: Bool) {
        guard let engine else { return }
        Task { [weak self] in
            guard let self else { return }
            do {
                let message = try await engine.autoFrame(id: id, compose: compose)
                self.images.invalidate(ids: [id])
                self.imageReloadToken &+= 1
                self.showToast(message, tone: .accent)
                await self.loadActiveDetail(id: id)
                if self.viewer == nil {
                    await self.refreshSnapshot(force: true)
                }
            } catch {
                self.presentError(Self.mapError(error))
            }
        }
    }

    func resetFraming(id: Int64) {
        guard let engine else { return }
        Task { [weak self] in
            guard let self else { return }
            do {
                try await engine.resetFraming(id: id)
                self.images.invalidate(ids: [id])
                self.imageReloadToken &+= 1
                self.showToast("크롭 · 수평을 원래대로 돌렸습니다", tone: .neutral)
                await self.loadActiveDetail(id: id)
                if self.viewer == nil {
                    await self.refreshSnapshot(force: true)
                }
            } catch {
                self.presentError(Self.mapError(error))
            }
        }
    }

    // MARK: - Thermal-Budgeted Analysis

    func startAnalysisIfNeeded() {
        guard let engine, !isSuspended else { return }
        guard analysisTask == nil || analysisTask?.isCancelled == true else { return }

        let budget = thermalMonitor.recommendedThreads
        if budget == 0 {
            analysisProgress = AnalysisProgress(done: 0, total: 0, pausedByThermal: true)
            return
        }

        analysisTask = Task { [weak self] in
            guard let self else { return }
            defer { self.analysisTask = nil }
            do {
                let pending = try await engine.pendingAnalysis()
                if pending == 0 {
                    if let status = try? await engine.learn() {
                        self.learn = status
                        await self.refreshSnapshot(force: true)
                    }
                    self.analysisProgress = nil
                    return
                }
                self.analysisProgress = AnalysisProgress(done: 0, total: pending, pausedByThermal: false)
                let _ = try await engine.analyze(maxThreads: budget) { [weak self] done, total in
                    Task { @MainActor [weak self] in
                        self?.analysisProgress = AnalysisProgress(done: done, total: total, pausedByThermal: false)
                    }
                }
                self.analysisProgress = nil
                await self.refreshSnapshot(force: true)
            } catch {
                self.analysisProgress = nil
                let mapped = Self.mapError(error)
                if mapped != .cancelled && mapped != .librarySuspended {
                    self.showToast("셀렉 분석 중단: \(mapped.message)", tone: .negative)
                }
            }
        }
    }

    private func handleThermalBudgetChange(_ budget: Int) async {
        guard let engine, analysisTask != nil else { return }
        if budget == 0 {
            await engine.cancelAnalysis()
            analysisTask?.cancel()
            analysisProgress = AnalysisProgress(done: analysisProgress?.done ?? 0, total: analysisProgress?.total ?? 0, pausedByThermal: true)
            showToast("기기 온도가 높아 분석을 잠시 멈췄습니다", tone: .neutral)
        } else if analysisProgress?.pausedByThermal == true {
            startAnalysisIfNeeded()
        }
    }

    // MARK: - Import (SD Card / External SSD / Folder)

    func openImport() {
        if case .done = importPhase { importPhase = .idle }
        if case .failed = importPhase { importPhase = .idle }
        importSheetPresented = true
    }

    func scanCardFolder(url: URL, includeVideos: Bool = true) {
        guard let engine else { return }
        lastImportURL = url
        let access = SecurityScopedAccess(url: url)
        activeImportAccess = access
        let label = url.lastPathComponent
        importPhase = .scanning(label: label)

        importTask?.cancel()
        importTask = Task { [weak self] in
            guard let self else { return }
            do {
                let info = try await engine.scanDirectory(path: url.path, label: label, includeVideos: includeVideos)
                let destURL = URL(fileURLWithPath: engine.rootPath)
                let freeBytes = BookmarkStore.availableCapacityBytes(at: destURL)
                self.importPhase = .ready(info: info, includeVideos: includeVideos, freeBytes: freeBytes)
            } catch {
                let mapped = Self.mapError(error)
                self.importPhase = .failed(mapped)
            }
        }
    }

    func setImportIncludeVideos(_ includeVideos: Bool) {
        if let url = lastImportURL {
            scanCardFolder(url: url, includeVideos: includeVideos)
        }
    }

    func startImport() {
        guard let engine, case let .ready(info, _, freeBytes) = importPhase else { return }
        importPhase = .copying(
            status: LoupeImportStatus(filesDone: 0, filesTotal: info.newFiles, bytesDone: 0, bytesTotal: info.newBytes, current: ""),
            label: info.label
        )
        importTask = Task { [weak self] in
            guard let self else { return }
            do {
                let result = try await engine.importScanned(availableBytes: freeBytes) { [weak self] status in
                    Task { @MainActor [weak self] in
                        guard let self, case .copying(_, let lbl) = self.importPhase else { return }
                        self.importPhase = .copying(status: status, label: lbl)
                    }
                }
                self.activeImportAccess = nil
                self.importPhase = .done(result)
                await self.refreshSnapshot(force: true)
                self.startAnalysisIfNeeded()
            } catch {
                self.activeImportAccess = nil
                let mapped = Self.mapError(error)
                self.importPhase = .failed(mapped)
                if case .insufficientStorage = mapped {
                    self.presentError(mapped)
                }
            }
        }
    }

    func cancelImport() {
        Task { [weak self] in
            await self?.engine?.cancelImport()
        }
    }

    // MARK: - Export (AirDrop / Files / Lightroom)

    func openExport(for ids: [Int64]) {
        guard let engine, !ids.isEmpty else { return }
        exportSheetPresented = true
        exportPhase = .planning
        exportTask?.cancel()
        exportTask = Task { [weak self] in
            guard let self else { return }
            do {
                let plan = try await engine.exportPlan(ids: ids)
                self.exportPhase = .ready(ids: ids, plan: plan)
            } catch {
                self.exportPhase = .failed(Self.mapError(error))
            }
        }
    }

    func runExport(mode: LoupeExportMode, longEdge: Int, quality: Int, dateNames: Bool) {
        guard let engine, case let .ready(ids, _) = exportPhase else { return }
        let outDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("loupe-export-\(UUID().uuidString)", isDirectory: true)
        exportPhase = .running(done: 0, total: ids.count)

        exportTask = Task { [weak self] in
            guard let self else { return }
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
                        self?.exportPhase = .running(done: done, total: total)
                    }
                }
                self.exportPhase = .done(result: result, outputDirectory: outDir)
            } catch {
                self.exportPhase = .failed(Self.mapError(error))
            }
        }
    }

    func cancelExport() {
        Task { [weak self] in
            await self?.engine?.cancelExport()
        }
    }

    // MARK: - Non-Destructive Library Trash

    func askDelete(ids: [Int64]) {
        guard let engine, !ids.isEmpty else { return }
        Task { [weak self] in
            guard let self else { return }
            do {
                let plan = try await engine.deletePlan(ids: ids)
                self.deleteConfirmation = DeleteConfirmation(ids: ids, plan: plan)
            } catch {
                self.presentError(Self.mapError(error))
            }
        }
    }

    func confirmDelete() {
        guard let engine, let confirmation = deleteConfirmation else { return }
        deleteConfirmation = nil
        Task { [weak self] in
            guard let self else { return }
            do {
                let result = try await engine.deletePhotos(ids: confirmation.ids)
                self.images.invalidate(ids: confirmation.ids)
                let deletedSet = Set(confirmation.ids)
                self.selectedIDs.subtract(deletedSet)
                for id in confirmation.ids {
                    self.summaries.removeValue(forKey: id)
                }
                if let session = self.viewer {
                    let remaining = session.ids.filter { !deletedSet.contains($0) }
                    if remaining.isEmpty {
                        self.viewer = nil
                        self.activeDetail = nil
                    } else {
                        let nextIdx = min(session.index, remaining.count - 1)
                        self.viewer = ViewerSession(ids: remaining, index: nextIdx)
                        self.loadActiveDetailAsync(id: remaining[nextIdx])
                    }
                }
                self.trashBytes = await engine.trashBytes()
                if result.deleted > 0 {
                    self.showToast("\(result.deleted)장을 휴지통으로 옮겼습니다 (학습 기록은 유지)", tone: .neutral)
                }
                await self.refreshSnapshot(force: true)
            } catch {
                self.presentError(Self.mapError(error))
            }
        }
    }

    func refreshTrashBytes() {
        Task { [weak self] in
            guard let self, let engine = self.engine else { return }
            self.trashBytes = await engine.trashBytes()
        }
    }

    func emptyTrash() {
        guard let engine else { return }
        Task { [weak self] in
            guard let self else { return }
            do {
                try await engine.emptyTrash()
                self.trashBytes = await engine.trashBytes()
                self.showToast("휴지통을 비웠습니다", tone: .positive)
            } catch {
                self.presentError(Self.mapError(error))
            }
        }
    }

    // MARK: - Recovery & Bottom Toast

    func performRecoveryAction(_ action: RecoveryActionKind) {
        recoveryAlert = nil
        switch action {
        case .reopenLibrary:
            Task { [weak self] in
                await self?.handleScenePhase(.active)
            }
        case .pickLibraryFolder, .openSettingsToEmptyTrash:
            settingsSheetPresented = true
        case .startAnalysis:
            startAnalysisIfNeeded()
        }
    }

    func showToast(_ text: String, tone: BottomToast.Tone) {
        toast = BottomToast(text: text, tone: tone)
        toastTask?.cancel()
        toastTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 1_400_000_000)
            guard !Task.isCancelled else { return }
            self?.toast = nil
        }
    }

    private func presentError(_ error: LoupeError) {
        LoupeLog.engine.error("Error surfaced to UI: \(error.title) — \(error.message)")
        recoveryAlert = RecoveryAlert.from(error)
    }

    private static func mapError(_ error: Error) -> LoupeError {
        if let loupe = error as? LoupeError {
            return loupe
        }
        return .other(error.localizedDescription)
    }
}

private final class EngineWeakBox: @unchecked Sendable {
    var engine: (any PhotoEngineProtocol)?
}
