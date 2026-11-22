import Foundation
import ImageIO
import UIKit

/// High-performance, memory-bounded image pipeline:
/// - Hardware-accelerated JPEG decoding via `CGImageSourceCreateThumbnailAtIndex`
/// - Zero software re-encoding for unframed RAW/JPEG previews (`EmbeddedSlice` -> offset read)
/// - Off-main-thread pre-decoding (`preparingForDisplay()`) to keep 120Hz ProMotion hitch-free
/// - Cost-bounded `NSCache` with automatic purge on `didReceiveMemoryWarningNotification`
/// - Cooperative `Task` cancellation for fast scrolling + ±2 neighbor prefetching in Viewer
final class ImagePipeline: @unchecked Sendable {
    private let thumbCache: NSCache<NSNumber, UIImage>
    private let previewCache: NSCache<NSString, UIImage>
    private let engineProvider: @Sendable () -> (any PhotoEngineProtocol)?
    private var memoryObserver: NSObjectProtocol?
    private let prefetchActor = PrefetchCoordinator()

    init(
        thumbBudgetBytes: Int = 64 * 1024 * 1024,
        previewBudgetBytes: Int = 96 * 1024 * 1024,
        engineProvider: @escaping @Sendable () -> (any PhotoEngineProtocol)?
    ) {
        self.engineProvider = engineProvider
        self.thumbCache = NSCache<NSNumber, UIImage>()
        self.thumbCache.totalCostLimit = thumbBudgetBytes
        self.thumbCache.countLimit = 600

        self.previewCache = NSCache<NSString, UIImage>()
        self.previewCache.totalCostLimit = previewBudgetBytes
        self.previewCache.countLimit = 6

        self.memoryObserver = NotificationCenter.default.addObserver(
            forName: UIApplication.didReceiveMemoryWarningNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.purgeMemory(includeThumbnails: true)
        }
    }

    deinit {
        if let observer = memoryObserver {
            NotificationCenter.default.removeObserver(observer)
        }
    }

    func cachedThumbnail(id: Int64) -> UIImage? {
        thumbCache.object(forKey: NSNumber(value: id))
    }

    func cachedPreview(id: Int64, maxEdge: Int) -> UIImage? {
        previewCache.object(forKey: Self.previewKey(id: id, maxEdge: maxEdge))
    }

    func invalidate(ids: [Int64]) {
        for id in ids {
            thumbCache.removeObject(forKey: NSNumber(value: id))
        }
        // Preview keys include maxEdge; clearing the small preview cache avoids stale crops.
        previewCache.removeAllObjects()
    }

    func purgeMemory(includeThumbnails: Bool) {
        LoupeLog.image.info("Purging image caches (includeThumbnails=\(includeThumbnails))")
        previewCache.removeAllObjects()
        if includeThumbnails {
            thumbCache.removeAllObjects()
        }
        Task { await prefetchActor.cancelAll() }
    }

    /// Loads and pre-decodes a grid thumbnail. Returns `nil` if cancelled or missing.
    func thumbnail(id: Int64, targetPixelSize: Int = 480) async -> UIImage? {
        if let cached = cachedThumbnail(id: id) {
            return cached
        }
        if Task.isCancelled { return nil }
        guard let engine = engineProvider() else { return nil }

        let signpost = LoupeLog.signposter.beginInterval("ThumbnailLoad", id: LoupeLog.signposter.makeSignpostID())
        defer { LoupeLog.signposter.endInterval("ThumbnailLoad", signpost) }

        do {
            guard let data = try await engine.thumbnailData(id: id), !Task.isCancelled else {
                return nil
            }
            guard let image = Self.decodeHardware(data: data, maxPixelSize: targetPixelSize, exifOrientation: 1) else {
                return nil
            }
            if Task.isCancelled { return nil }
            let prepared = await image.byPreparingForDisplay() ?? image
            thumbCache.setObject(prepared, forKey: NSNumber(value: id), cost: Self.pixelCost(prepared))
            return prepared
        } catch {
            if !Task.isCancelled {
                LoupeLog.image.debug("Thumbnail \(id) failed: \(error.localizedDescription)")
            }
            return nil
        }
    }

    /// Loads and pre-decodes a full-screen viewer preview using zero-reencode hardware decoding
    /// whenever the Rust engine supplies an `EmbeddedSlice`.
    func preview(id: Int64, maxEdge: Int) async -> UIImage? {
        let key = Self.previewKey(id: id, maxEdge: maxEdge)
        if let cached = previewCache.object(forKey: key) {
            return cached
        }
        if Task.isCancelled { return nil }
        guard let engine = engineProvider() else { return nil }

        let signpost = LoupeLog.signposter.beginInterval("PreviewLoad", id: LoupeLog.signposter.makeSignpostID())
        defer { LoupeLog.signposter.endInterval("PreviewLoad", signpost) }

        do {
            let source = try await engine.previewSource(id: id, maxEdge: maxEdge)
            if Task.isCancelled { return nil }

            let decoded: UIImage?
            switch source {
            case let .embeddedSlice(path, offset, length, orientation):
                // Read ONLY the embedded JPEG byte slice directly from disk (no full RAW copy,
                // no Rust software decode/re-encode, no FFI byte array transfer).
                guard let slice = Self.readFileSlice(path: path, offset: offset, length: length) else {
                    return nil
                }
                decoded = Self.decodeHardware(data: slice, maxPixelSize: maxEdge, exifOrientation: orientation)
            case .renderedJpeg(let bytes):
                decoded = Self.decodeHardware(data: bytes, maxPixelSize: maxEdge, exifOrientation: 1)
            }

            guard let image = decoded, !Task.isCancelled else { return nil }
            let prepared = await image.byPreparingForDisplay() ?? image
            previewCache.setObject(prepared, forKey: key, cost: Self.pixelCost(prepared))
            return prepared
        } catch {
            if !Task.isCancelled {
                LoupeLog.image.debug("Preview \(id) failed: \(error.localizedDescription)")
            }
            return nil
        }
    }

    /// Prefetches adjacent viewer photos (+1, -1, +2) at utility priority so swiping while zoomed
    /// in feels instantaneous. Cancels outdated prefetch tasks automatically.
    func prefetchNeighbors(ids: [Int64], around index: Int, maxEdge: Int) {
        let candidates = [index + 1, index - 1, index + 2]
            .filter { $0 >= 0 && $0 < ids.count }
            .map { ids[$0] }
        Task {
            await prefetchActor.schedule(ids: candidates, maxEdge: maxEdge, pipeline: self)
        }
    }

    func cancelPrefetch() {
        Task { await prefetchActor.cancelAll() }
    }

    // MARK: - Hardware Decoding Helpers

    private static func previewKey(id: Int64, maxEdge: Int) -> NSString {
        "\(id)_\(maxEdge)" as NSString
    }

    private static func pixelCost(_ image: UIImage) -> Int {
        guard let cg = image.cgImage else {
            return Int(image.size.width * image.size.height * image.scale * image.scale * 4)
        }
        return cg.bytesPerRow * cg.height
    }

    private static func readFileSlice(path: String, offset: UInt64, length: UInt64) -> Data? {
        guard length > 0, length <= 96 * 1024 * 1024 else { return nil }
        let url = URL(fileURLWithPath: path)
        do {
            let handle = try FileHandle(forReadingFrom: url)
            defer { try? handle.close() }
            try handle.seek(toOffset: offset)
            return try handle.read(upToCount: Int(length))
        } catch {
            LoupeLog.image.debug("readFileSlice failed for \(url.lastPathComponent): \(error.localizedDescription)")
            return nil
        }
    }

    /// Single-pass hardware JPEG downsample via Apple ImageIO (`CGImageSourceCreateThumbnailAtIndex`).
    static func decodeHardware(data: Data, maxPixelSize: Int, exifOrientation: Int) -> UIImage? {
        let cfData = data as CFData
        let sourceOptions: [CFString: Any] = [
            kCGImageSourceShouldCache: false,
        ]
        guard let source = CGImageSourceCreateWithData(cfData, sourceOptions as CFDictionary) else {
            return nil
        }
        let thumbOptions: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: false,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: max(64, maxPixelSize),
        ]
        guard let cgImage = CGImageSourceCreateThumbnailAtIndex(source, 0, thumbOptions as CFDictionary) else {
            return nil
        }
        return UIImage(cgImage: cgImage, scale: 1.0, orientation: uiOrientation(fromExif: exifOrientation))
    }

    private static func uiOrientation(fromExif exif: Int) -> UIImage.Orientation {
        switch exif {
        case 2: return .upMirrored
        case 3: return .down
        case 4: return .downMirrored
        case 5: return .leftMirrored
        case 6: return .right
        case 7: return .rightMirrored
        case 8: return .left
        default: return .up
        }
    }
}

private actor PrefetchCoordinator {
    private var tasks: [Int64: Task<Void, Never>] = [:]

    func schedule(ids: [Int64], maxEdge: Int, pipeline: ImagePipeline) {
        let keep = Set(ids)
        for (id, task) in tasks where !keep.contains(id) {
            task.cancel()
            tasks.removeValue(forKey: id)
        }
        for id in ids where tasks[id] == nil {
            if pipeline.cachedPreview(id: id, maxEdge: maxEdge) != nil { continue }
            tasks[id] = Task(priority: .utility) {
                _ = await pipeline.preview(id: id, maxEdge: maxEdge)
            }
        }
    }

    func cancelAll() {
        for (_, task) in tasks {
            task.cancel()
        }
        tasks.removeAll()
    }
}
