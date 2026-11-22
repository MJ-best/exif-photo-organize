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
}
