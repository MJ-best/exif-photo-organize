import SwiftUI
import XCTest
@testable import Loupe

@MainActor
final class LibraryStoreTests: XCTestCase {
    private func makeIsolatedDefaults() -> UserDefaults {
        let suite = "loupe.tests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        return defaults
    }

    private func makeStore(
        sampleCount: Int = 12,
        userDefaults: UserDefaults? = nil,
        suppressAutoOnboarding: Bool = true,
        idleTimer: IdleTimerManager? = nil
    ) -> (LibraryStore, MockPhotoEngine) {
        let mock = MockPhotoEngine(sampleCount: sampleCount)
        let store = LibraryStore(
            engine: mock,
            userDefaults: userDefaults ?? makeIsolatedDefaults(),
            suppressAutoOnboarding: suppressAutoOnboarding,
            idleTimer: idleTimer ?? IdleTimerManager(applyDisabled: { _ in }),
            learnDebounceNanoseconds: 20_000_000,
            engineFactory: { _ in mock }
        )
        return (store, mock)
    }

    func testBootstrapLoadsAtomicSnapshotAndFirstWindow() async {
        let (store, _) = makeStore(sampleCount: 15)
        await store.bootstrap()

        XCTAssertTrue(store.isReady)
        XCTAssertEqual(store.visibleIDs.count, 15)
        XCTAssertEqual(store.counts.all, 15)
        XCTAssertEqual(store.summaries.count, 15)
        XCTAssertFalse(store.rules.isEmpty)
    }

    func testOptimisticRatingUpdatesImmediatelyAndRollsBackOnFailure() async {
        let (store, mock) = makeStore(sampleCount: 6)
        await store.bootstrap()

        let targetID = try! XCTUnwrap(store.visibleIDs.first)
        let originalRating = store.summaries[targetID]?.rating ?? 0

        await mock.setInjectedError(.io("디스크 쓰기 실패"))
        store.rate(ids: [targetID], rating: 5)
        XCTAssertEqual(store.summaries[targetID]?.rating, 5)

        try? await Task.sleep(nanoseconds: 80_000_000)
        XCTAssertEqual(store.summaries[targetID]?.rating, originalRating)
        XCTAssertFalse(store.canUndo)
        XCTAssertNotNil(store.recoveryAlert)
    }

    func testUndoRestoresSingleAndBulkDecisionsAtomically() async {
        let (store, mock) = makeStore(sampleCount: 6)
        await store.bootstrap()

        let ids = Array(store.visibleIDs.prefix(3))
        let original = ids.map { id in
            (id: id, rating: store.summaries[id]?.rating ?? 0, flag: store.summaries[id]?.flag ?? 0)
        }

        // Single rating followed by bulk reject.
        store.rate(ids: [ids[0]], rating: 5)
        XCTAssertTrue(store.canUndo)
        XCTAssertEqual(store.summaries[ids[0]]?.rating, 5)

        store.flag(ids: ids, flag: -1)
        XCTAssertEqual(store.toast?.undoAvailable, true)
        for id in ids {
            XCTAssertEqual(store.summaries[id]?.flag, -1)
        }

        // Undo bulk reject -> restores all 3 flags while keeping ids[0]'s 5-star rating.
        store.undoLastDecision()
        try? await Task.sleep(nanoseconds: 100_000_000)
        for orig in original {
            XCTAssertEqual(store.summaries[orig.id]?.flag, orig.flag)
        }
        XCTAssertEqual(store.summaries[ids[0]]?.rating, 5)

        // Undo single rating -> restores ids[0]'s original rating.
        store.undoLastDecision()
        try? await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertEqual(store.summaries[ids[0]]?.rating, original[0].rating)
        XCTAssertFalse(store.canUndo)

        let restores = await mock.restoreCallCount
        XCTAssertEqual(restores, 2)
    }

    func testBackgroundSuspendDropsDBAndReleasesMLModels() async {
        let (store, mock) = makeStore(sampleCount: 8)
        await store.bootstrap()

        await store.handleScenePhase(.background)
        let suspends = await mock.suspendCallCount
        let releases = await mock.releaseModelsCallCount
        XCTAssertTrue(store.isSuspended)
        XCTAssertEqual(suspends, 1)
        XCTAssertEqual(releases, 1)

        await store.handleScenePhase(.active)
        let resumes = await mock.resumeCallCount
        XCTAssertFalse(store.isSuspended)
        XCTAssertEqual(resumes, 1)
    }

    func testBackgroundSuspendPreservesPendingBackgroundAnalysisFlag() async {
        let (store, mock) = makeStore(sampleCount: 6)
        await mock.setPendingAnalysis(10)
        await mock.setAnalyzeStepDelay(50_000_000)
        await store.bootstrap()

        try? await Task.sleep(nanoseconds: 40_000_000)
        XCTAssertNotNil(store.analyzer.progress)

        // Transitioning to background cancels foreground analysis (clearing UI progress)
        // while preserving `hasPendingBackgroundAnalysis` for `BGProcessingTask` scheduling.
        await store.handleScenePhase(.background)
        XCTAssertTrue(store.isSuspended)
        XCTAssertNil(store.analyzer.progress)
        XCTAssertTrue(store.hasPendingBackgroundAnalysis)
    }

    func testImportAndExportCoordinatorsManageLifecycleAndWakeLock() async {
        var idleDisabledHistory: [Bool] = []
        let wakeManager = IdleTimerManager { disabled in
            idleDisabledHistory.append(disabled)
        }
        let (store, _) = makeStore(sampleCount: 4, idleTimer: wakeManager)
        await store.bootstrap()

        let dummyFolder = FileManager.default.temporaryDirectory
        store.importer.scanCardFolder(url: dummyFolder, includeVideos: true)
        try? await Task.sleep(nanoseconds: 60_000_000)
        if case .ready = store.importer.phase {
            store.importer.startImport()
            try? await Task.sleep(nanoseconds: 80_000_000)
        } else {
            XCTFail("Expected ImportCoordinator to reach .ready")
        }
        if case .done(let result) = store.importer.phase {
            XCTAssertEqual(result.added, 12)
        } else {
            XCTFail("Expected ImportCoordinator to reach .done")
        }

        // Export flow
        store.exporter.open(for: [store.visibleIDs[0], store.visibleIDs[1]])
        try? await Task.sleep(nanoseconds: 60_000_000)
        if case .ready = store.exporter.phase {
            store.exporter.run(mode: .jpegOnly, longEdge: 2048, quality: 88, dateNames: true)
            try? await Task.sleep(nanoseconds: 80_000_000)
        } else {
            XCTFail("Expected ExportCoordinator to reach .ready")
        }
        if case .done(let res, _) = store.exporter.phase {
            XCTAssertEqual(res.files, 2)
        } else {
            XCTFail("Expected ExportCoordinator to reach .done")
        }
        store.exporter.cleanupTempDirectory()

        // Wake lock was acquired and cleanly restored to activeCount == 0.
        XCTAssertEqual(wakeManager.activeCount, 0)
        XCTAssertEqual(idleDisabledHistory.first, true)
        XCTAssertEqual(idleDisabledHistory.last, false)
    }

    func testInsufficientStorageProducesTargetedRecoveryAlert() async {
        let alert = RecoveryAlert.from(.insufficientStorage(requiredBytes: 1_000_000_000, availableBytes: 100_000_000))
        XCTAssertEqual(alert.primaryAction, .openSettingsToEmptyTrash)
        XCTAssertNotNil(alert.primaryActionTitle)

        let permAlert = RecoveryAlert.from(.permissionDenied("bookmark expired"))
        XCTAssertEqual(permAlert.primaryAction, .pickLibraryFolder)
    }

    func testDeleteUpdatesViewerSessionAndPrunesUndoStack() async {
        let (store, _) = makeStore(sampleCount: 5)
        await store.bootstrap()
        let first = store.visibleIDs[0]
        let second = store.visibleIDs[1]

        store.rate(ids: [first], rating: 4)
        XCTAssertTrue(store.canUndo)

        store.openViewer(at: first)
        XCTAssertEqual(store.viewer?.currentID, first)

        store.askDelete(ids: [first])
        try? await Task.sleep(nanoseconds: 60_000_000)
        XCTAssertNotNil(store.deleteConfirmation)

        store.confirmDelete()
        try? await Task.sleep(nanoseconds: 100_000_000)

        XCTAssertEqual(store.viewer?.currentID, second)
        XCTAssertEqual(store.counts.all, 4)
        XCTAssertGreaterThan(store.trashBytes, 0)
        XCTAssertFalse(store.canUndo)
    }

    func testAutoFrameAndResetFramingBumpReloadToken() async {
        let (store, _) = makeStore(sampleCount: 3)
        await store.bootstrap()
        let first = store.visibleIDs[0]
        store.openViewer(at: first)
        try? await Task.sleep(nanoseconds: 40_000_000)

        let tokenBefore = store.imageReloadToken
        store.autoFrame(id: first, compose: true)
        try? await Task.sleep(nanoseconds: 80_000_000)

        XCTAssertGreaterThan(store.imageReloadToken, tokenBefore)
        XCTAssertEqual(store.activeDetail?.summary.hasFraming, true)
        XCTAssertNotNil(store.activeDetail?.framing)

        let tokenAfterAuto = store.imageReloadToken
        store.resetFraming(id: first)
        try? await Task.sleep(nanoseconds: 80_000_000)

        XCTAssertGreaterThan(store.imageReloadToken, tokenAfterAuto)
        XCTAssertEqual(store.activeDetail?.summary.hasFraming, false)
        XCTAssertNil(store.activeDetail?.framing)
    }

    func testOptimizeStorageCallsEngineAndShowsToast() async {
        let (store, mock) = makeStore(sampleCount: 4)
        await store.bootstrap()

        store.optimizeStorage()
        try? await Task.sleep(nanoseconds: 80_000_000)

        let calls = await mock.optimizeCallCount
        XCTAssertEqual(calls, 1)
        XCTAssertFalse(store.isOptimizingStorage)
        XCTAssertNotNil(store.toast)
    }

    func testEmptyLibraryBootstrapSucceeds() async {
        let (store, _) = makeStore(sampleCount: 0)
        await store.bootstrap()

        XCTAssertTrue(store.isReady)
        XCTAssertTrue(store.visibleIDs.isEmpty)
        XCTAssertEqual(store.counts, .zero)
    }

    func testFirstRunOnboardingPresentedOnlyForEmptyUnseenLibrary() async {
        let defaults = makeIsolatedDefaults()
        let (store, _) = makeStore(
            sampleCount: 0,
            userDefaults: defaults,
            suppressAutoOnboarding: false
        )
        await store.bootstrap()

        XCTAssertTrue(store.onboardingPresented)

        store.completeOnboarding()
        XCTAssertFalse(store.onboardingPresented)
        XCTAssertTrue(defaults.bool(forKey: LibraryStore.onboardingCompletedKey))

        let (secondStore, _) = makeStore(
            sampleCount: 0,
            userDefaults: defaults,
            suppressAutoOnboarding: false
        )
        await secondStore.bootstrap()
        XCTAssertFalse(secondStore.onboardingPresented)
    }

    func testOnboardingSuppressedWhenLibraryAlreadyHasPhotos() async {
        let defaults = makeIsolatedDefaults()
        let (store, _) = makeStore(
            sampleCount: 5,
            userDefaults: defaults,
            suppressAutoOnboarding: false
        )
        await store.bootstrap()

        XCTAssertFalse(store.onboardingPresented)
    }

    func testViewerGestureTipDismissesAndPersists() async {
        let defaults = makeIsolatedDefaults()
        let (store, _) = makeStore(sampleCount: 3, userDefaults: defaults)
        await store.bootstrap()

        XCTAssertFalse(store.hasDismissedViewerGestureTip)
        store.dismissViewerGestureTip()
        XCTAssertTrue(store.hasDismissedViewerGestureTip)
        XCTAssertTrue(defaults.bool(forKey: LibraryStore.viewerGestureTipSeenKey))

        let (secondStore, _) = makeStore(sampleCount: 3, userDefaults: defaults)
        XCTAssertTrue(secondStore.hasDismissedViewerGestureTip)
    }

    func testSampleModeEntersIsolatedLibraryAndExitsCleanly() async {
        let defaults = makeIsolatedDefaults()
        let store = LibraryStore(
            engine: MockPhotoEngine(sampleCount: 0),
            userDefaults: defaults,
            suppressAutoOnboarding: false,
            idleTimer: IdleTimerManager(applyDisabled: { _ in }),
            learnDebounceNanoseconds: 20_000_000,
            engineFactory: { path in
                MockPhotoEngine(sampleCount: path.contains("LOUPE-Sample") ? 5 : 0)
            }
        )
        await store.bootstrap()
        XCTAssertTrue(store.onboardingPresented)
        XCTAssertFalse(store.isSampleMode)

        let dummySampleDir = FileManager.default.temporaryDirectory
        await store.enterSampleMode(sampleDirectory: dummySampleDir)

        XCTAssertTrue(store.isSampleMode)
        XCTAssertFalse(store.onboardingPresented)
        XCTAssertEqual(store.counts.all, 5)
        XCTAssertEqual(store.libraryDisplayName, "견본 사진 체험")

        await store.exitSampleMode()

        XCTAssertFalse(store.isSampleMode)
        XCTAssertEqual(store.counts.all, 0)
        XCTAssertEqual(store.libraryDisplayName, "이 기기 · LOUPE")
    }

    func testOpenImportDuringSampleModeExitsSampleModeFirst() async {
        let defaults = makeIsolatedDefaults()
        let store = LibraryStore(
            engine: MockPhotoEngine(sampleCount: 0),
            userDefaults: defaults,
            suppressAutoOnboarding: true,
            idleTimer: IdleTimerManager(applyDisabled: { _ in }),
            learnDebounceNanoseconds: 20_000_000,
            engineFactory: { path in
                MockPhotoEngine(sampleCount: path.contains("LOUPE-Sample") ? 5 : 0)
            }
        )
        await store.bootstrap()

        await store.enterSampleMode(sampleDirectory: FileManager.default.temporaryDirectory)
        XCTAssertTrue(store.isSampleMode)

        store.openImport()
        try? await Task.sleep(nanoseconds: 120_000_000)

        XCTAssertFalse(store.isSampleMode)
        XCTAssertTrue(store.importer.sheetPresented)
    }
}
