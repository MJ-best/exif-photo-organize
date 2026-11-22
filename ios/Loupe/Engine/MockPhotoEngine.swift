import Foundation
import UIKit

/// Deterministic in-memory engine used by SwiftUI Previews and XCTest suites.
actor MockPhotoEngine: PhotoEngineProtocol {
    nonisolated let rootPath: String

    private var items: [Int64: LoupePhotoDetail] = [:]
    private var order: [Int64] = []
    private var revisionCounter: Int64 = 1
    private var thresholdValue: Int = 60
    private var keepPctValue: Int = 15
    private var disabledMask: Int64 = 0
    private var suspendedState: Bool = false
    private var trashSizeBytes: Int64 = 0
    private var pendingCount: Int = 0
    private var sampleJpegData: Data

    var suspendCallCount: Int = 0
    var resumeCallCount: Int = 0
    var releaseModelsCallCount: Int = 0
    var learnCallCount: Int = 0
    var lastAnalyzeThreadBudget: Int = 0
    var injectedError: LoupeError?

    init(rootPath: String = "/tmp/loupe-mock", sampleCount: Int = 18) {
        self.rootPath = rootPath
        self.sampleJpegData = Self.makeSolidJpeg(width: 480, height: 320)
        let baseMs: Int64 = 1_763_683_200_000
        guard sampleCount > 0 else { return }
        for i in 1...sampleCount {
            let id = Int64(i)
            let isCandidate = i % 3 == 0
            let isKeeper = i % 4 == 1
            let bPct = isCandidate ? 78 : (isKeeper ? 12 : 38)
            let summary = LoupePhotoSummary(
                id: id,
                name: String(format: "IMG_%04d.CR3", i),
                capturedMs: baseMs + Int64(i * 45_000),
                rating: i % 5 == 0 ? 4 : 0,
                flag: i == 2 ? 1 : (i == 6 ? -1 : 0),
                bPct: bPct,
                topPct: isKeeper ? 8 : nil,
                burstRank: i <= 3 ? i : 0,
                burstSize: i <= 3 ? 3 : 0,
                hasFraming: false,
                video: false,
                raw: true,
                jpeg: i % 2 == 0,
                width: 6000,
                height: 4000
            )
            let detail = LoupePhotoDetail(
                summary: summary,
                reasons: isCandidate ? ["눈 감음 의심", "수평 1.8° 기울어짐"] : [],
                strengths: isKeeper ? ["눈에 초점", "3분할 구도", "배경이 깔끔하게 흐림"] : [],
                scene: i % 2 == 0 ? "인물" : "풍경",
                framing: nil,
                compNotes: [
                    LoupeCompNote(text: "3분할: 눈 위치가 위 1/3 선에 가까움", ok: true, ignored: false),
                    LoupeCompNote(text: isCandidate ? "수평 1.8° 기울어짐" : "수평이 잘 맞음", ok: !isCandidate, ignored: false),
                ],
                overlay: LoupeCompositionOverlay(
                    hasSubject: true,
                    subjectX: 0.28,
                    subjectY: 0.16,
                    subjectW: 0.44,
                    subjectH: 0.78,
                    keyX: 0.42,
                    keyY: 0.34,
                    headTop: 0.14,
                    facing: 0.4,
                    pole: 0.0,
                    poleX: 0.5,
                    neckLine: 0.0,
                    neckY: 0.5,
                    tilt: isCandidate ? 1.8 : 0.1,
                    tiltConf: isCandidate ? 0.75 : 0.2,
                    tiltVertical: false,
                    tiltAt: 0.52,
                    showThirds: true,
                    showHeadroom: true,
                    showLeadRoom: true,
                    showMerger: true,
                    showTilt: true
                ),
                camera: "Canon EOS R6 Mark II",
                lens: "RF 50mm F1.2L USM",
                exposure: "50mm · ƒ1.4 · 1/500 · ISO 200"
            )
            items[id] = detail
            order.append(id)
        }
    }

    func setInjectedError(_ err: LoupeError?) {
        injectedError = err
    }

    func setPendingAnalysis(_ count: Int) {
        pendingCount = count
    }

    private func guardReady() throws {
        if let err = injectedError { throw err }
        if suspendedState { throw LoupeError.librarySuspended }
    }

    func suspend() async throws {
        suspendCallCount += 1
        suspendedState = true
    }

    func resume() async throws {
        resumeCallCount += 1
        suspendedState = false
        revisionCounter += 1
    }

    func releaseMLModels() async {
        releaseModelsCallCount += 1
    }

    func snapshot(view: LoupeViewFilter) async throws -> LoupeSnapshot {
        try guardReady()
        let ids = order.filter { id in
            guard let p = items[id]?.summary else { return false }
            return matches(view: view, photo: p)
        }
        return LoupeSnapshot(
            view: view,
            ids: ids,
            counts: computeCounts(),
            learn: currentLearnStatus(),
            revision: revisionCounter
        )
    }

    func photosBatch(ids: [Int64]) async throws -> [LoupePhotoSummary] {
        try guardReady()
        return ids.compactMap { items[$0]?.summary }
    }

    func photoDetail(id: Int64) async throws -> LoupePhotoDetail? {
        try guardReady()
        return items[id]
    }

    func thumbnailData(id: Int64) async throws -> Data? {
        try guardReady()
        guard items[id] != nil else { return nil }
        return sampleJpegData
    }

    func previewSource(id: Int64, maxEdge: Int) async throws -> LoupePreviewSource {
        try guardReady()
        guard items[id] != nil else { throw LoupeError.notFound("사진 #\(id)") }
        return .renderedJpeg(sampleJpegData)
    }

    func setRating(ids: [Int64], rating: Int) async throws -> Int64 {
        try guardReady()
        let clamped = max(0, min(5, rating))
        for id in ids {
            items[id]?.summary.rating = clamped
        }
        revisionCounter += 1
        return revisionCounter
    }

    func setFlag(ids: [Int64], flag: Int) async throws -> Int64 {
        try guardReady()
        let normalized = flag > 0 ? 1 : (flag < 0 ? -1 : 0)
        for id in ids {
            items[id]?.summary.flag = normalized
        }
        revisionCounter += 1
        return revisionCounter
    }

    func threshold() async -> Int { thresholdValue }

    func setThreshold(_ percent: Int) async {
        thresholdValue = max(30, min(90, percent))
        revisionCounter += 1
    }

    func keepPct() async -> Int { keepPctValue }

    func setKeepPct(_ percent: Int) async {
        keepPctValue = max(5, min(50, percent))
        revisionCounter += 1
    }

    func disabledRules() async -> Int64 { disabledMask }

    func setDisabledRules(_ mask: Int64) async {
        disabledMask = mask
        revisionCounter += 1
    }

    func rules() async -> [LoupeRuleItem] {
        [
            LoupeRuleItem(bit: 1, group: "초점 · 노출", label: "초점 흐림", enabled: (disabledMask & 1) == 0),
            LoupeRuleItem(bit: 2, group: "초점 · 노출", label: "흔들림", enabled: (disabledMask & 2) == 0),
            LoupeRuleItem(bit: 4, group: "초점 · 노출", label: "눈 감음", enabled: (disabledMask & 4) == 0),
            LoupeRuleItem(bit: 512, group: "구도 · 프레이밍", label: "3분할", enabled: (disabledMask & 512) == 0),
            LoupeRuleItem(bit: 1024, group: "구도 · 프레이밍", label: "헤드룸 (머리 위 여백)", enabled: (disabledMask & 1024) == 0),
            LoupeRuleItem(bit: 8192, group: "구도 · 프레이밍", label: "수평 · 수직 기울어짐", enabled: (disabledMask & 8192) == 0),
        ]
    }

    func autoFrame(id: Int64, compose: Bool) async throws -> String {
        try guardReady()
        guard var detail = items[id] else { throw LoupeError.notFound("사진 #\(id)") }
        detail.summary.hasFraming = true
        let label = compose ? "1.8° 돌리고 88% 크기로 자름" : "1.8° 돌림"
        items[id] = LoupePhotoDetail(
            summary: detail.summary,
            reasons: detail.reasons,
            strengths: detail.strengths,
            scene: detail.scene,
            framing: label,
            compNotes: detail.compNotes,
            overlay: detail.overlay,
            camera: detail.camera,
            lens: detail.lens,
            exposure: detail.exposure
        )
        revisionCounter += 1
        return compose ? "1.8° 돌려 수평을 맞추고 구도에 맞춰 잘랐습니다" : "1.8° 돌려 수평을 맞췄습니다"
    }

    func resetFraming(id: Int64) async throws {
        try guardReady()
        guard var detail = items[id] else { throw LoupeError.notFound("사진 #\(id)") }
        detail.summary.hasFraming = false
        items[id] = LoupePhotoDetail(
            summary: detail.summary,
            reasons: detail.reasons,
            strengths: detail.strengths,
            scene: detail.scene,
            framing: nil,
            compNotes: detail.compNotes,
            overlay: detail.overlay,
            camera: detail.camera,
            lens: detail.lens,
            exposure: detail.exposure
        )
        revisionCounter += 1
    }

    func scanDirectory(path: String, label: String, includeVideos: Bool) async throws -> LoupeScanInfo {
        try guardReady()
        return LoupeScanInfo(
            label: label.isEmpty ? "SD_CARD" : label,
            photos: 24,
            newPhotos: 12,
            newFiles: 24,
            newBytes: 360 * 1024 * 1024,
            videos: includeVideos ? 2 : 0,
            rawPairs: 12,
            firstMs: 1_763_683_200_000,
            lastMs: 1_763_690_400_000,
            cameras: ["Canon EOS R6 Mark II · 12장"],
            errors: []
        )
    }

    func importScanned(
        availableBytes: Int64,
        onProgress: @Sendable @escaping (LoupeImportStatus) -> Void
    ) async throws -> LoupeImportResult {
        try guardReady()
        let required: Int64 = 360 * 1024 * 1024
        if availableBytes >= 0 && availableBytes < required {
            throw LoupeError.insufficientStorage(requiredBytes: required, availableBytes: availableBytes)
        }
        onProgress(LoupeImportStatus(filesDone: 12, filesTotal: 24, bytesDone: required / 2, bytesTotal: required, current: "IMG_1001.CR3"))
        onProgress(LoupeImportStatus(filesDone: 24, filesTotal: 24, bytesDone: required, bytesTotal: required, current: "IMG_1012.JPG"))
        revisionCounter += 1
        return LoupeImportResult(added: 12, updated: 0, duplicates: 12, bytes: required, failures: [], cancelled: false)
    }

    func cancelImport() async {}

    func pendingAnalysis() async throws -> Int {
        try guardReady()
        return pendingCount
    }

    func analyze(
        maxThreads: Int,
        onProgress: @Sendable @escaping (Int, Int) -> Void
    ) async throws -> Int {
        try guardReady()
        lastAnalyzeThreadBudget = maxThreads
        let total = pendingCount
        onProgress(total, total)
        pendingCount = 0
        revisionCounter += 1
        return total
    }

    func cancelAnalysis() async {}

    func learn() async throws -> LoupeLearnStatus {
        try guardReady()
        learnCallCount += 1
        revisionCounter += 1
        return currentLearnStatus()
    }

    func exportPlan(ids: [Int64]) async throws -> LoupeExportPlan {
        try guardReady()
        let framed = ids.filter { items[$0]?.summary.hasFraming == true }.count
        return LoupeExportPlan(
            photos: ids.count,
            originalsBytes: Int64(ids.count) * 28 * 1024 * 1024,
            jpegBytes: Int64(ids.count) * 6 * 1024 * 1024,
            rawOnly: 0,
            videos: 0,
            framedCount: framed
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
        try guardReady()
        onProgress(ids.count, ids.count)
        return LoupeExportResult(files: ids.count, fromRaw: 0, bytes: Int64(ids.count) * 2_000_000, skipped: [], cancelled: false)
    }

    func cancelExport() async {}

    func deletePlan(ids: [Int64]) async throws -> LoupeDeletePlan {
        try guardReady()
        return LoupeDeletePlan(photos: ids.count, files: ids.count * 2, bytes: Int64(ids.count) * 32 * 1024 * 1024)
    }

    func deletePhotos(ids: [Int64]) async throws -> LoupeDeleteResult {
        try guardReady()
        var deleted = 0
        for id in ids where items.removeValue(forKey: id) != nil {
            deleted += 1
        }
        order.removeAll { items[$0] == nil }
        let bytes = Int64(deleted) * 32 * 1024 * 1024
        trashSizeBytes += bytes
        revisionCounter += 1
        return LoupeDeleteResult(deleted: deleted, failed: 0, bytes: bytes, firstError: nil)
    }

    func trashBytes() async -> Int64 { trashSizeBytes }

    func emptyTrash() async throws {
        try guardReady()
        trashSizeBytes = 0
    }

    private func matches(view: LoupeViewFilter, photo: LoupePhotoSummary) -> Bool {
        switch view {
        case .all: return true
        case .undecided: return photo.flag == 0 && photo.rating == 0
        case .candidates: return photo.flag == 0 && photo.rating == 0 && (photo.bPct ?? 0) >= thresholdValue
        case .keepers: return photo.flag == 0 && photo.rating == 0 && (photo.topPct ?? 100) <= keepPctValue
        case .rejected: return photo.flag < 0
        case .picked: return photo.flag > 0
        case .rated: return photo.rating > 0
        }
    }

    private func computeCounts() -> LoupeCounts {
        let all = order.compactMap { items[$0]?.summary }
        return LoupeCounts(
            all: all.count,
            undecided: all.filter { matches(view: .undecided, photo: $0) }.count,
            candidates: all.filter { matches(view: .candidates, photo: $0) }.count,
            keepers: all.filter { matches(view: .keepers, photo: $0) }.count,
            rejected: all.filter { matches(view: .rejected, photo: $0) }.count,
            picked: all.filter { matches(view: .picked, photo: $0) }.count,
            rated: all.filter { matches(view: .rated, photo: $0) }.count,
            analysed: all.filter { $0.bPct != nil }.count
        )
    }

    private func currentLearnStatus() -> LoupeLearnStatus {
        let all = order.compactMap { items[$0]?.summary }
        let a = all.filter { $0.flag > 0 || $0.rating >= 3 }.count
        let b = all.filter { $0.flag < 0 || $0.rating == 1 }.count
        let learned = a >= 6 && b >= 6
        let summary = learned
            ? "내 기준으로 학습됨 · A \(a) · B \(b)"
            : "기본 기준 · 학습까지 A \(min(a, 6)) / B \(min(b, 6)) (각 6장)"
        return LoupeLearnStatus(summary: summary, learned: learned, a: a, b: b, analysed: all.count, total: all.count)
    }

    private static func makeSolidJpeg(width: Int, height: Int) -> Data {
        let renderer = UIGraphicsImageRenderer(size: CGSize(width: width, height: height))
        let image = renderer.image { ctx in
            UIColor(red: 0.93, green: 0.85, blue: 0.74, alpha: 1.0).setFill()
            ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
            UIColor(red: 0.93, green: 0.42, blue: 0.23, alpha: 1.0).setFill()
            ctx.fill(CGRect(x: width / 3, y: height / 3, width: width / 3, height: height / 3))
        }
        return image.jpegData(compressionQuality: 0.8) ?? Data()
    }
}
