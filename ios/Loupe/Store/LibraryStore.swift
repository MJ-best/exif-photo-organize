import Foundation
import Observation
import SwiftUI

/// Single-source-of-truth unidirectional state machine for LOUPE on iOS & iPadOS.
///
/// Architectural guarantees:
/// 1. **No torn reads**: `visibleIDs`, `counts`, and `learn` always update atomically from a single
///    `LoupeSnapshot`, and stale out-of-order responses are discarded via `appliedRevision` +
///    request tokens.
/// 2. **Windowed FFI paging**: Only visible grid IDs (±48 window) are materialized into `summaries`,
///    preventing 20,000-photo main-thread serialization stalls.
/// 3. **Optimistic UI with rollback & bounded Undo**: Rating/pick/reject mutates the visible cell
///    in 0ms, rolls back on catalog write failure, and maintains a 20-step Undo stack (`⌘Z` or
///    bottom-strip "되돌리기") serialized against in-flight writes.
/// 4. **Domain decomposition**: Import, Export, and Thermal-Budgeted Analysis live in dedicated
///    `@Observable` coordinators (`importer`, `exporter`, `analyzer`).
/// 5. **Lifecycle safety**: Backgrounding runs inside `BackgroundTaskGuard`, preserves pending
///    background analysis state for `BGProcessingTask`, drains SQLite ops & truncates WAL
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
    private(set) var isOptimizingStorage: Bool = false

    private(set) var undoStack: [UndoEntry] = []
    var canUndo: Bool { !undoStack.isEmpty }
    var lastUndoLabel: String? { undoStack.last?.label }

    var selectedIDs: Set<Int64> = []
    var isSelectionMode: Bool = false
    var viewer: ViewerSession?
    private(set) var activeDetail: LoupePhotoDetail?
    var showCompositionOverlay: Bool = false
    var showInspector: Bool = true

    var settingsSheetPresented: Bool = false
    var deleteConfirmation: DeleteConfirmation?
    var recoveryAlert: RecoveryAlert?
    private(set) var toast: BottomToast?

    // MARK: - Domain Coordinators & Services
    private(set) var engine: (any PhotoEngineProtocol)?
    let images: ImagePipeline
    let importer: ImportCoordinator
    let exporter: ExportCoordinator
    let analyzer: AnalysisCoordinator

    var thermalMonitor: ThermalBudgetMonitor { analyzer.thermalMonitor }
    var analysisProgress: AnalysisProgress? { analyzer.progress }
    var hasPendingBackgroundAnalysis: Bool { analyzer.hasPendingBackgroundAnalysis }

    private let bookmarkStore: BookmarkStore
    private let engineFactory: @Sendable (String) throws -> any PhotoEngineProtocol
    private let engineBox: EngineWeakBox
    private let refBox: StoreRefBox

    private var activeLibraryAccess: SecurityScopedAccess?
    private var appliedRevision: Int64 = 0
    private var snapshotRequestToken: UInt64 = 0
    private var inFlightBatchIDs: Set<Int64> = []
    private var idToIndex: [Int64: Int] = [:]

    private var refreshTask: Task<Void, Never>?
    private var detailTask: Task<Void, Never>?
    private var learnTask: Task<Void, Never>?
    private var lastMutationTask: Task<Void, Never>?
    private var toastTask: Task<Void, Never>?

    private let learnDebounceNanoseconds: UInt64
    private let maxUndoDepth: Int = 20

    init(
        engine: (any PhotoEngineProtocol)? = nil,
        bookmarkStore: BookmarkStore = BookmarkStore(),
        thermalMonitor: ThermalBudgetMonitor = ThermalBudgetMonitor(),
        idleTimer: IdleTimerManager = .shared,
        learnDebounceNanoseconds: UInt64 = 1_200_000_000,
        engineFactory: @escaping @Sendable (String) throws -> any PhotoEngineProtocol
    ) {
        self.engine = engine
        self.bookmarkStore = bookmarkStore
        self.learnDebounceNanoseconds = learnDebounceNanoseconds
        self.engineFactory = engineFactory

        let savedThreshold = UserDefaults.standard.integer(forKey: "loupe.threshold")
        self.threshold = savedThreshold >= 30 ? savedThreshold : 60
        let savedKeepPct = UserDefaults.standard.integer(forKey: "loupe.keep_pct")
        self.keepPct = savedKeepPct >= 5 ? savedKeepPct : 15

        let box = EngineWeakBox()
        box.set(engine)
        self.engineBox = box
        self.images = ImagePipeline { box.get() }

        let storeBox = StoreRefBox()
        self.refBox = storeBox

        self.importer = ImportCoordinator(
            engineProvider: { storeBox.store?.engine },
            onImportCompleted: {
                guard let store = storeBox.store else { return }
                await store.refreshSnapshot(force: true)
                store.analyzer.startIfNeeded()
            },
            onPresentError: { error in
                storeBox.store?.presentError(error)
            },
            idleTimer: idleTimer
        )

        self.exporter = ExportCoordinator(
            engineProvider: { storeBox.store?.engine },
            idleTimer: idleTimer
        )

        self.analyzer = AnalysisCoordinator(
            thermalMonitor: thermalMonitor,
            engineProvider: { storeBox.store?.engine },
            isSuspendedProvider: { storeBox.store?.isSuspended ?? true },
            onChunkRefresh: {
                await storeBox.store?.refreshSnapshot(force: true)
            },
            onLearnStatusUpdated: { status in
                storeBox.store?.learn = status
            },
            onToast: { text, tone in
                storeBox.store?.showToast(text, tone: tone)
            }
        )

        storeBox.store = self
    }

    // MARK: - Lifecycle (Apple `0xdead10cc`, Background Assertions & Memory Protection)

    func bootstrap() async {
        if let existing = engine {
            await configureAndLoad(engine: existing, external: false, name: "LOUPE")
            return
        }

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
    /// Wraps SQLite drain + WAL truncation inside `BackgroundTaskGuard` so iOS grants time to
    /// close file descriptors cleanly before suspension (`0xdead10cc` prevention).
    func handleScenePhase(_ phase: ScenePhase) async {
        switch phase {
        case .background:
            guard !isSuspended, let engine else { return }
            await BackgroundTaskGuard.run(name: "io.github.mj-best.loupe.suspend") {
                LoupeLog.lifecycle.info("Entering background: draining tasks, suspending SQLite, releasing ML models")
                await self.analyzer.prepareForBackground()
                self.learnTask?.cancel()
                await self.lastMutationTask?.value
                self.images.purgeMemory(includeThumbnails: false)
                do {
                    try await engine.suspend()
                    await engine.releaseMLModels()
                    self.isSuspended = true
                } catch {
                    LoupeLog.lifecycle.error("Suspend failed: \(error.localizedDescription)")
                }
            }
        case .active:
            guard isSuspended, let engine else { return }
            LoupeLog.lifecycle.info("Returning to foreground: resuming SQLite and syncing snapshot")
            do {
                try await engine.resume()
                isSuspended = false
                await refreshSnapshot(force: true)
                analyzer.startIfNeeded()
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

    func optimizeStorage() {
        guard let engine, !isSuspended, !isOptimizingStorage else { return }
        isOptimizingStorage = true
        Task { [weak self] in
            guard let self else { return }
            defer { self.isOptimizingStorage = false }
            await BackgroundTaskGuard.run(name: "io.github.mj-best.loupe.optimize") {
                do {
                    try await engine.optimizeStorage()
                    self.trashBytes = await engine.trashBytes()
                    self.showToast("카탈로그와 WAL 저장소를 정리했습니다", tone: .positive)
                } catch {
                    self.presentError(Self.mapError(error))
                }
            }
        }
    }

    private func suspendForSwitch() async {
        analyzer.resetForLibrarySwitch()
        importer.resetForLibrarySwitch()
        exporter.resetForLibrarySwitch()
        learnTask?.cancel()
        await lastMutationTask?.value
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
        undoStack.removeAll()
        viewer = nil
        activeDetail = nil
        appliedRevision = 0
    }

    private func configureAndLoad(engine: any PhotoEngineProtocol, external: Bool, name: String) async {
        self.engine = engine
        self.engineBox.set(engine)
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
        analyzer.startIfNeeded()
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
            guard token == snapshotRequestToken, activeView == view else { return }
            guard force || snap.revision >= appliedRevision else { return }

            appliedRevision = snap.revision
            visibleIDs = snap.ids
            idToIndex = Dictionary(uniqueKeysWithValues: snap.ids.enumerated().map { ($1, $0) })
            counts = snap.counts
            learn = snap.learn

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

    /// O(1) entry point for `ForEach(store.visibleIDs, id: \.self)` cells.
    func ensureLoaded(for id: Int64) {
        guard let index = idToIndex[id] else { return }
        ensureLoaded(around: index)
    }

    /// Loads compact `PhotoSummary` records only for the visible grid window (±48 cells).
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
        guard let idx = idToIndex[id] ?? visibleIDs.firstIndex(of: id) else { return }
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

    // MARK: - Optimistic Culling Mutations (0ms Feedback + Rollback + Bounded Undo)

    func rate(ids: [Int64], rating: Int) {
        let clamped = max(0, min(5, rating))
        let label = ids.count == 1
            ? (clamped == 0 ? "별점 해제" : String(repeating: "★", count: clamped))
            : "\(ids.count)장 별점 변경"
        applyDecisionMutation(
            ids: ids,
            label: label,
            tone: clamped > 0 ? .accent : .neutral,
            mutateSummary: { $0.rating = clamped },
            persist: { engine in try await engine.setRating(ids: ids, rating: clamped) }
        )
    }

    func flag(ids: [Int64], flag: Int) {
        let normalized = flag > 0 ? 1 : (flag < 0 ? -1 : 0)
        let label: String
        let tone: BottomToast.Tone
        switch normalized {
        case 1:
            label = ids.count == 1 ? "픽 — A컷" : "\(ids.count)장 픽 (A컷)"
            tone = .positive
        case -1:
            label = ids.count == 1 ? "거절 — B컷" : "\(ids.count)장 거절 (B컷)"
            tone = .negative
        default:
            label = ids.count == 1 ? "픽/거절 해제" : "\(ids.count)장 분류 해제"
            tone = .neutral
        }
        applyDecisionMutation(
            ids: ids,
            label: label,
            tone: tone,
            mutateSummary: { $0.flag = normalized },
            persist: { engine in try await engine.setFlag(ids: ids, flag: normalized) }
        )
    }

    private func applyDecisionMutation(
        ids: [Int64],
        label: String,
        tone: BottomToast.Tone,
        mutateSummary: (inout LoupePhotoSummary) -> Void,
        persist: @escaping @Sendable (any PhotoEngineProtocol) async throws -> Int64
    ) {
        guard let engine, !ids.isEmpty else { return }

        var loadedPrior: [Int64: LoupeDecisionRestore] = [:]
        for id in ids {
            if var existing = summaries[id] {
                loadedPrior[id] = LoupeDecisionRestore(id: id, rating: existing.rating, flag: existing.flag)
                mutateSummary(&existing)
                summaries[id] = existing
            }
        }
        if var active = activeDetail?.summary, ids.contains(active.id) {
            if loadedPrior[active.id] == nil {
                loadedPrior[active.id] = LoupeDecisionRestore(id: active.id, rating: active.rating, flag: active.flag)
            }
            mutateSummary(&active)
            activeDetail?.summary = active
        }

        let missingIDs = ids.filter { loadedPrior[$0] == nil }
        let immediateEntryID: UUID?
        if missingIDs.isEmpty {
            let entry = UndoEntry(label: label, priorDecisions: ids.compactMap { loadedPrior[$0] })
            pushUndo(entry)
            immediateEntryID = entry.id
        } else {
            immediateEntryID = nil
        }

        showToast(label, tone: tone, undoAvailable: true)

        let priorTask = lastMutationTask
        lastMutationTask = Task { [weak self] in
            await priorTask?.value
            guard let self else { return }
            do {
                var fullPrior = loadedPrior
                if !missingIDs.isEmpty {
                    let fetched = try await engine.photosBatch(ids: missingIDs)
                    for item in fetched {
                        fullPrior[item.id] = LoupeDecisionRestore(id: item.id, rating: item.rating, flag: item.flag)
                    }
                }
                let rev = try await persist(engine)
                self.appliedRevision = max(self.appliedRevision, rev)
                if immediateEntryID == nil {
                    let restores = ids.compactMap { fullPrior[$0] }
                    if !restores.isEmpty {
                        self.pushUndo(UndoEntry(label: label, priorDecisions: restores))
                    }
                }
                if self.viewer == nil {
                    await self.refreshSnapshot(force: true)
                }
                self.scheduleDebouncedLearn()
            } catch {
                if let entryID = immediateEntryID {
                    self.undoStack.removeAll { $0.id == entryID }
                }
                for (id, prev) in loadedPrior {
                    self.summaries[id]?.rating = prev.rating
                    self.summaries[id]?.flag = prev.flag
                }
                if let activeID = self.activeDetail?.summary.id, let prev = loadedPrior[activeID] {
                    self.activeDetail?.summary.rating = prev.rating
                    self.activeDetail?.summary.flag = prev.flag
                }
                self.toast = nil
                self.presentError(Self.mapError(error))
            }
        }
    }

    /// Reverts the most recent rating or pick/reject mutation (single or bulk) atomically in
    /// SQLite + XMP sidecars with instant 0ms UI update.
    func undoLastDecision() {
        guard let engine, let entry = undoStack.popLast() else { return }

        var rollback: [Int64: (rating: Int, flag: Int)] = [:]
        for item in entry.priorDecisions {
            if let existing = summaries[item.id] {
                rollback[item.id] = (existing.rating, existing.flag)
                summaries[item.id]?.rating = item.rating
                summaries[item.id]?.flag = item.flag
            }
            if activeDetail?.summary.id == item.id {
                if rollback[item.id] == nil, let active = activeDetail?.summary {
                    rollback[item.id] = (active.rating, active.flag)
                }
                activeDetail?.summary.rating = item.rating
                activeDetail?.summary.flag = item.flag
            }
        }

        showToast("'\(entry.label)' 되돌림", tone: .neutral, undoAvailable: false)

        let priorTask = lastMutationTask
        lastMutationTask = Task { [weak self] in
            await priorTask?.value
            guard let self else { return }
            do {
                let rev = try await engine.restoreDecisions(items: entry.priorDecisions)
                self.appliedRevision = max(self.appliedRevision, rev)
                if self.viewer == nil {
                    await self.refreshSnapshot(force: true)
                } else if let currentID = self.viewer?.currentID {
                    await self.loadActiveDetail(id: currentID)
                }
                self.scheduleDebouncedLearn()
            } catch {
                for (id, prev) in rollback {
                    self.summaries[id]?.rating = prev.rating
                    self.summaries[id]?.flag = prev.flag
                }
                if let activeID = self.activeDetail?.summary.id, let prev = rollback[activeID] {
                    self.activeDetail?.summary.rating = prev.rating
                    self.activeDetail?.summary.flag = prev.flag
                }
                self.pushUndo(entry)
                self.presentError(Self.mapError(error))
            }
        }
    }

    private func pushUndo(_ entry: UndoEntry) {
        undoStack.append(entry)
        if undoStack.count > maxUndoDepth {
            undoStack.removeFirst(undoStack.count - maxUndoDepth)
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

    // MARK: - Coordinator Convenience Forwarders

    func startAnalysisIfNeeded() {
        analyzer.startIfNeeded()
    }

    func openImport() {
        importer.open()
    }

    func openExport(for ids: [Int64]) {
        exporter.open(for: ids)
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
                self.pruneDeletedFromUndoStack(deletedSet)
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

    private func pruneDeletedFromUndoStack(_ deletedSet: Set<Int64>) {
        undoStack = undoStack.compactMap { entry in
            let remaining = entry.priorDecisions.filter { !deletedSet.contains($0.id) }
            guard !remaining.isEmpty else { return nil }
            return UndoEntry(id: entry.id, label: entry.label, priorDecisions: remaining)
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
            analyzer.startIfNeeded()
        }
    }

    func showToast(_ text: String, tone: BottomToast.Tone, undoAvailable: Bool = false) {
        toast = BottomToast(text: text, tone: tone, undoAvailable: undoAvailable)
        toastTask?.cancel()
        let durationNs: UInt64 = undoAvailable ? 2_800_000_000 : 1_400_000_000
        toastTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: durationNs)
            guard !Task.isCancelled else { return }
            self?.toast = nil
        }
    }

    func presentError(_ error: LoupeError) {
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

/// Thread-safe lock-protected reference box bridging `@MainActor` `LibraryStore` to `ImagePipeline`.
private final class EngineWeakBox: @unchecked Sendable {
    private let lock = NSLock()
    private var engine: (any PhotoEngineProtocol)?

    func set(_ value: (any PhotoEngineProtocol)?) {
        lock.lock()
        engine = value
        lock.unlock()
    }

    func get() -> (any PhotoEngineProtocol)? {
        lock.lock()
        defer { lock.unlock() }
        return engine
    }
}

/// MainActor-isolated weak reference holder allowing domain coordinators to be initialized as
/// non-optional `let` properties without retain cycles.
@MainActor
private final class StoreRefBox {
    weak var store: LibraryStore?
}
