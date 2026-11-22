import SwiftUI
import XCTest
@testable import Loupe

@MainActor
final class LibraryStoreTests: XCTestCase {
    private func makeStore(sampleCount: Int = 12) -> (LibraryStore, MockPhotoEngine) {
        let mock = MockPhotoEngine(sampleCount: sampleCount)
        let store = LibraryStore(
            engine: mock,
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
        // Optimistic update happens synchronously on MainActor.
        XCTAssertEqual(store.summaries[targetID]?.rating, 5)

        try? await Task.sleep(nanoseconds: 80_000_000)
        // Rolled back after failure and surfaced a recovery alert.
        XCTAssertEqual(store.summaries[targetID]?.rating, originalRating)
        XCTAssertNotNil(store.recoveryAlert)
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

    func testInsufficientStorageProducesTargetedRecoveryAlert() async {
        let (store, _) = makeStore(sampleCount: 4)
        await store.bootstrap()

        let dummyFolder = FileManager.default.temporaryDirectory
        store.scanCardFolder(url: dummyFolder, includeVideos: true)
        try? await Task.sleep(nanoseconds: 60_000_000)

        // Verify structured RecoveryAlert mapping for insufficient storage.
        let alert = RecoveryAlert.from(.insufficientStorage(requiredBytes: 1_000_000_000, availableBytes: 100_000_000))
        XCTAssertEqual(alert.primaryAction, .openSettingsToEmptyTrash)
        XCTAssertNotNil(alert.primaryActionTitle)

        let permAlert = RecoveryAlert.from(.permissionDenied("bookmark expired"))
        XCTAssertEqual(permAlert.primaryAction, .pickLibraryFolder)
    }

    func testDeleteUpdatesViewerSessionGracefully() async {
        let (store, _) = makeStore(sampleCount: 5)
        await store.bootstrap()
        let first = store.visibleIDs[0]
        let second = store.visibleIDs[1]

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

    func testEmptyLibraryBootstrapSucceeds() async {
        let (store, _) = makeStore(sampleCount: 0)
        await store.bootstrap()

        XCTAssertTrue(store.isReady)
        XCTAssertTrue(store.visibleIDs.isEmpty)
        XCTAssertEqual(store.counts, .zero)
    }
}
