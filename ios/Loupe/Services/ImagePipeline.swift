import AVFoundation
import Foundation
import ImageIO
import UIKit

/// High-performance, memory-bounded image pipeline:
/// - Hardware-accelerated JPEG decoding via `CGImageSourceCreateThumbnailAtIndex`
/// - Zero-copy mmap slice reading (`Data(contentsOf:options:.mappedIfSafe)`) for RAW/JPEG previews
/// - Hardware video thumbnail extraction via `AVAssetImageGenerator`
/// - In-flight request coalescing + bounded decode concurrency (max 4) for stutter-free scrolling
/// - Off-main-thread pre-decoding (`byPreparingForDisplay()`) for 120Hz ProMotion
/// - Precise per-photo preview cache invalidation (preserves prefetched neighbors)
/// - Cost-bounded `NSCache` with automatic purge on `didReceiveMemoryWarningNotification`
final class ImagePipeline: @unchecked Sendable {
    private let thumbCache: NSCache<NSNumber, UIImage>
    private let previewCache: NSCache<NSString, UIImage>
    private let keyLock = NSLock()
    private var previewKeysByPhotoID: [Int64: Set<NSString>] = [:]
    private let engineProvider: @Sendable () -> (any PhotoEngineProtocol)?
    private var memoryObserver: NSObjectProtocol?
    private let prefetchActor = PrefetchCoordinator()
    private let thumbCoalescer = ThumbnailCoalescer()
    private let decodeLimiter: DecodeLimiter

    init(
        thumbBudgetBytes: Int = 64 * 1024 * 1024,
        previewBudgetBytes: Int = 96 * 1024 * 1024,
        maxConcurrentDecodes: Int = 4,
        engineProvider: @escaping @Sendable () -> (any PhotoEngineProtocol)?
    ) {
        self.engineProvider = engineProvider
        self.decodeLimiter = DecodeLimiter(limit: max(1, maxConcurrentDecodes))

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

    /// Invalidates ONLY the specified photo IDs from both thumbnail and preview caches so
    /// neighboring prefetched previews stay warm after auto-frame / crop / delete operations.
    func invalidate(ids: [Int64]) {
        for id in ids {
            thumbCache.removeObject(forKey: NSNumber(value: id))
            let previewKeys: Set<NSString>?
            keyLock.lock()
            previewKeys = previewKeysByPhotoID.removeValue(forKey: id)
            keyLock.unlock()
            if let previewKeys {
                for key in previewKeys {
                    previewCache.removeObject(forKey: key)
                }
            }
        }
    }

    func purgeMemory(includeThumbnails: Bool) {
        LoupeLog.image.info("Purging image caches (includeThumbnails=\(includeThumbnails))")
        previewCache.removeAllObjects()
        keyLock.lock()
        previewKeysByPhotoID.removeAll()
        keyLock.unlock()
        if includeThumbnails {
            thumbCache.removeAllObjects()
        }
        Task {
            await prefetchActor.cancelAll()
            if includeThumbnails {
                await thumbCoalescer.cancelAll()
            }
        }
    }

    /// Loads and pre-decodes a grid/filmstrip thumbnail with in-flight deduplication and bounded
    /// concurrency. Falls back to `AVAssetImageGenerator` for video assets.
    func thumbnail(id: Int64, targetPixelSize: Int = 480) async -> UIImage? {
        if let cached = cachedThumbnail(id: id) {
            return cached
        }
        if Task.isCancelled { return nil }

        return await thumbCoalescer.load(id: id) { [weak self] in
            guard let self else { return nil }
            return await self.loadThumbnailUncached(id: id, targetPixelSize: targetPixelSize)
        }
    }

    private func loadThumbnailUncached(id: Int64, targetPixelSize: Int) async -> UIImage? {
        if let cached = cachedThumbnail(id: id) {
            return cached
        }
        if Task.isCancelled { return nil }
        guard let engine = engineProvider() else { return nil }

        let limiter = self.decodeLimiter
        await limiter.acquire()
        defer { Task { await limiter.release() } }
        if Task.isCancelled { return nil }

        let signpost = LoupeLog.signposter.beginInterval("ThumbnailLoad", id: LoupeLog.signposter.makeSignpostID())
        defer { LoupeLog.signposter.endInterval("ThumbnailLoad", signpost) }

        do {
            let decoded: UIImage?
            if let data = try await engine.thumbnailData(id: id) {
                if Task.isCancelled { return nil }
                decoded = Self.decodeHardware(data: data, maxPixelSize: targetPixelSize, exifOrientation: 1)
            } else if let path = try await engine.videoPath(id: id) {
                if Task.isCancelled { return nil }
                decoded = await Self.generateVideoThumbnail(path: path, maxPixelSize: targetPixelSize)
            } else {
                decoded = nil
            }
            guard let image = decoded, !Task.isCancelled else { return nil }
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

    /// Loads and pre-decodes a full-screen viewer preview using zero-copy mmap + hardware ImageIO
    /// decoding whenever the Rust engine supplies an `EmbeddedSlice`.
    func preview(id: Int64, maxEdge: Int) async -> UIImage? {
        let key = Self.previewKey(id: id, maxEdge: maxEdge)
        if let cached = previewCache.object(forKey: key) {
            return cached
        }
        if Task.isCancelled { return nil }
        guard let engine = engineProvider() else { return nil }

        let limiter = self.decodeLimiter
        await limiter.acquire()
        defer { Task { await limiter.release() } }
        if Task.isCancelled { return nil }

        let signpost = LoupeLog.signposter.beginInterval("PreviewLoad", id: LoupeLog.signposter.makeSignpostID())
        defer { LoupeLog.signposter.endInterval("PreviewLoad", signpost) }

        do {
            let source = try await engine.previewSource(id: id, maxEdge: maxEdge)
            if Task.isCancelled { return nil }

            let decoded: UIImage?
            switch source {
            case let .embeddedSlice(path, offset, length, orientation):
                // Zero-copy mmap slice of the embedded JPEG inside the RAW/JPEG file (no full RAW
                // heap copy, no Rust software re-encode, no FFI byte array transfer).
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
            recordPreviewKey(key, for: id)
            return prepared
        } catch {
            if !Task.isCancelled {
                LoupeLog.image.debug("Preview \(id) failed: \(error.localizedDescription)")
            }
            return nil
        }
    }

    private func recordPreviewKey(_ key: NSString, for id: Int64) {
        keyLock.lock()
        previewKeysByPhotoID[id, default: []].insert(key)
        keyLock.unlock()
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

    /// Zero-copy memory-mapped byte slice with safe `FileHandle` fallback for non-mappable files.
    static func readFileSlice(path: String, offset: UInt64, length: UInt64) -> Data? {
        guard length > 0, length <= 96 * 1024 * 1024 else { return nil }
        let url = URL(fileURLWithPath: path)
        if let start = Int(exactly: offset),
           let len = Int(exactly: length),
           let end = start.addingReportingOverflow(len).overflow ? nil : start + len,
           let mapped = try? Data(contentsOf: url, options: .mappedIfSafe),
           end <= mapped.count {
            return mapped[start..<end]
        }
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

    /// Hardware-accelerated video frame extraction for video assets in the grid & filmstrip.
    private static func generateVideoThumbnail(path: String, maxPixelSize: Int) async -> UIImage? {
        let url = URL(fileURLWithPath: path)
        guard FileManager.default.fileExists(atPath: path) else { return nil }
        let asset = AVURLAsset(url: url)
        let generator = AVAssetImageGenerator(asset: asset)
        generator.appliesPreferredTrackTransform = true
        let edge = CGFloat(max(64, maxPixelSize))
        generator.maximumSize = CGSize(width: edge, height: edge)
        do {
            let (cgImage, _) = try await generator.image(at: .zero)
            return UIImage(cgImage: cgImage)
        } catch {
            LoupeLog.image.debug("Video thumbnail failed for \(url.lastPathComponent): \(error.localizedDescription)")
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

/// Bounded concurrency gate keeping simultaneous ImageIO/AVFoundation decodes capped during rapid
/// grid flings so memory spikes never trigger iOS Jetsam.
private actor DecodeLimiter {
    private let limit: Int
    private var active: Int = 0
    private var waiters: [CheckedContinuation<Void, Never>] = []

    init(limit: Int) {
        self.limit = limit
    }

    func acquire() async {
        if active < limit {
            active += 1
            return
        }
        await withCheckedContinuation { continuation in
            waiters.append(continuation)
        }
    }

    func release() {
        if !waiters.isEmpty {
            let next = waiters.removeFirst()
            next.resume()
        } else {
            active = max(0, active - 1)
        }
    }
}

/// Deduplicates concurrent thumbnail requests for the same photo ID (e.g., Grid + Filmstrip or
/// rapid scroll bounce) and only cancels the underlying decode when the last waiter cancels.
private actor ThumbnailCoalescer {
    private struct Entry {
        let task: Task<UIImage?, Never>
        var waiters: Set<UUID>
    }

    private var inFlight: [Int64: Entry] = [:]

    func load(id: Int64, work: @escaping @Sendable () async -> UIImage?) async -> UIImage? {
        let waiterID = UUID()
        let task: Task<UIImage?, Never>
        if var existing = inFlight[id] {
            existing.waiters.insert(waiterID)
            inFlight[id] = existing
            task = existing.task
        } else {
            let created = Task { await work() }
            inFlight[id] = Entry(task: created, waiters: [waiterID])
            task = created
        }

        return await withTaskCancellationHandler {
            let result = await task.value
            removeWaiter(id: id, waiterID: waiterID, cancelIfLast: false)
            return result
        } onCancel: {
            Task { await self.removeWaiter(id: id, waiterID: waiterID, cancelIfLast: true) }
        }
    }

    private func removeWaiter(id: Int64, waiterID: UUID, cancelIfLast: Bool) {
        guard var entry = inFlight[id], entry.waiters.remove(waiterID) != nil else { return }
        if entry.waiters.isEmpty {
            if cancelIfLast { entry.task.cancel() }
            inFlight.removeValue(forKey: id)
        } else {
            inFlight[id] = entry
        }
    }

    func cancelAll() {
        for (_, entry) in inFlight {
            entry.task.cancel()
        }
        inFlight.removeAll()
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
