import XCTest
@testable import Loupe

final class ThermalAndPipelineTests: XCTestCase {
    func testThermalBudgetPausesOnCriticalAndCapsOnLowRAM() {
        XCTAssertEqual(
            ThermalBudgetPolicy.recommendedThreads(
                thermalState: .critical,
                lowPowerMode: false,
                physicalMemoryBytes: 16 * 1024 * 1024 * 1024,
                activeProcessorCount: 10
            ),
            0
        )
        XCTAssertEqual(
            ThermalBudgetPolicy.recommendedThreads(
                thermalState: .serious,
                lowPowerMode: false,
                physicalMemoryBytes: 8 * 1024 * 1024 * 1024,
                activeProcessorCount: 8
            ),
            1
        )
        XCTAssertEqual(
            ThermalBudgetPolicy.recommendedThreads(
                thermalState: .nominal,
                lowPowerMode: true,
                physicalMemoryBytes: 8 * 1024 * 1024 * 1024,
                activeProcessorCount: 8
            ),
            1
        )
        // Older 4GB iPhone stays at 1 thread to prevent Jetsam OOM.
        XCTAssertEqual(
            ThermalBudgetPolicy.recommendedThreads(
                thermalState: .nominal,
                lowPowerMode: false,
                physicalMemoryBytes: 4 * 1024 * 1024 * 1024,
                activeProcessorCount: 6
            ),
            1
        )
        // Standard 6GB iPhone uses 2 threads.
        XCTAssertEqual(
            ThermalBudgetPolicy.recommendedThreads(
                thermalState: .nominal,
                lowPowerMode: false,
                physicalMemoryBytes: 6 * 1024 * 1024 * 1024,
                activeProcessorCount: 6
            ),
            2
        )
        // M-series iPad Pro (8GB+, 8+ cores) scales up to 3 threads.
        XCTAssertEqual(
            ThermalBudgetPolicy.recommendedThreads(
                thermalState: .nominal,
                lowPowerMode: false,
                physicalMemoryBytes: 8 * 1024 * 1024 * 1024,
                activeProcessorCount: 8
            ),
            3
        )
    }

    func testImagePipelineCachesAndPurgesOnMemoryPressure() async {
        let mock = MockPhotoEngine(sampleCount: 3)
        let pipeline = ImagePipeline { mock }

        let thumb = await pipeline.thumbnail(id: 1, targetPixelSize: 256)
        XCTAssertNotNil(thumb)
        XCTAssertNotNil(pipeline.cachedThumbnail(id: 1))

        let preview = await pipeline.preview(id: 1, maxEdge: 1024)
        XCTAssertNotNil(preview)
        XCTAssertNotNil(pipeline.cachedPreview(id: 1, maxEdge: 1024))

        pipeline.purgeMemory(includeThumbnails: true)
        XCTAssertNil(pipeline.cachedThumbnail(id: 1))
        XCTAssertNil(pipeline.cachedPreview(id: 1, maxEdge: 1024))
    }

    func testPreciseInvalidationPreservesNeighborPreviewCache() async {
        let mock = MockPhotoEngine(sampleCount: 4)
        let pipeline = ImagePipeline { mock }

        _ = await pipeline.thumbnail(id: 1, targetPixelSize: 256)
        _ = await pipeline.thumbnail(id: 2, targetPixelSize: 256)
        _ = await pipeline.preview(id: 1, maxEdge: 1280)
        _ = await pipeline.preview(id: 2, maxEdge: 1280)

        pipeline.invalidate(ids: [1])

        XCTAssertNil(pipeline.cachedThumbnail(id: 1))
        XCTAssertNil(pipeline.cachedPreview(id: 1, maxEdge: 1280))
        // Neighbor photo #2 stays warm in both thumbnail and preview caches.
        XCTAssertNotNil(pipeline.cachedThumbnail(id: 2))
        XCTAssertNotNil(pipeline.cachedPreview(id: 2, maxEdge: 1280))
    }

    func testConcurrentThumbnailRequestsCoalesceIntoSingleEngineFetch() async {
        let mock = MockPhotoEngine(sampleCount: 2)
        let pipeline = ImagePipeline { mock }

        let loadedCount = await withTaskGroup(of: Bool.self) { group in
            for _ in 0..<6 {
                group.addTask {
                    await pipeline.thumbnail(id: 1, targetPixelSize: 256) != nil
                }
            }
            var ok = 0
            for await hit in group where hit {
                ok += 1
            }
            return ok
        }

        XCTAssertEqual(loadedCount, 6)
        let fetches = await mock.thumbnailFetchCount
        XCTAssertEqual(fetches, 1)
    }

    func testReadFileSliceExtractsExactBytesViaMmap() throws {
        let tmp = FileManager.default.temporaryDirectory
            .appendingPathComponent("loupe-mmap-\(UUID().uuidString).bin")
        let payload = Data([0x00, 0x01, 0x02, 0xFF, 0xD8, 0xAA, 0xBB, 0xFF, 0xD9, 0x99])
        try payload.write(to: tmp)
        defer { try? FileManager.default.removeItem(at: tmp) }

        let slice = ImagePipeline.readFileSlice(path: tmp.path, offset: 3, length: 6)
        XCTAssertEqual(slice, Data([0xFF, 0xD8, 0xAA, 0xBB, 0xFF, 0xD9]))
    }

    @MainActor
    func testIdleTimerManagerReferenceCountsOverlappingAcquisitions() {
        var currentDisabled = false
        let manager = IdleTimerManager { disabled in
            currentDisabled = disabled
        }

        let t1 = manager.acquire(reason: "import")
        XCTAssertTrue(currentDisabled)
        XCTAssertEqual(manager.activeCount, 1)

        let t2 = manager.acquire(reason: "export")
        XCTAssertTrue(currentDisabled)
        XCTAssertEqual(manager.activeCount, 2)

        t1.release()
        t1.release() // Idempotent double-release must not underflow.
        XCTAssertTrue(currentDisabled)
        XCTAssertEqual(manager.activeCount, 1)

        t2.release()
        XCTAssertFalse(currentDisabled)
        XCTAssertEqual(manager.activeCount, 0)
    }
}
