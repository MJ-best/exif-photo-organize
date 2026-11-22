import Foundation

// MARK: - Anti-Corruption Domain Types
// Swift UI & Store code never imports UniFFI types directly; only `RustPhotoEngine` translates
// between UniFFI bindings and these clean, Sendable domain models.

enum LoupeViewFilter: String, CaseIterable, Identifiable, Sendable {
    case all
    case undecided
    case candidates
    case keepers
    case picked
    case rejected
    case rated

    var id: String { rawValue }

    var title: String {
        switch self {
        case .all: return "모든 사진"
        case .undecided: return "아직 고르지 않음"
        case .candidates: return "B컷 후보"
        case .keepers: return "A컷 후보"
        case .picked: return "픽 (A컷)"
        case .rejected: return "거절 (B컷)"
        case .rated: return "별점 있는 사진"
        }
    }

    var shortTitle: String {
        switch self {
        case .all: return "전체"
        case .undecided: return "미결정"
        case .candidates: return "B컷 후보"
        case .keepers: return "A컷 후보"
        case .picked: return "픽"
        case .rejected: return "거절"
        case .rated: return "별점"
        }
    }

    var systemImage: String {
        switch self {
        case .all: return "photo.on.rectangle.angled"
        case .undecided: return "circle.dashed"
        case .candidates: return "exclamationmark.triangle"
        case .keepers: return "sparkles"
        case .picked: return "flag.fill"
        case .rejected: return "xmark.circle"
        case .rated: return "star.fill"
        }
    }
}

enum LoupeExportMode: String, CaseIterable, Identifiable, Sendable {
    case jpegOnly
    case resized
    case originals

    var id: String { rawValue }

    var title: String {
        switch self {
        case .jpegOnly: return "JPEG만"
        case .resized: return "작게 (공유용 · GPS 제거)"
        case .originals: return "원본 전체 (RAW · XMP 포함)"
        }
    }
}

enum LoupeError: Error, Equatable, Sendable {
    case librarySuspended
    case notFound(String)
    case permissionDenied(String)
    case insufficientStorage(requiredBytes: Int64, availableBytes: Int64)
    case notAnalyzed
    case unsupportedForVideo
    case cancelled
    case database(String)
    case io(String)
    case other(String)

    var title: String {
        switch self {
        case .librarySuspended: return "라이브러리가 잠시 멈춰 있습니다"
        case .notFound: return "파일을 찾을 수 없습니다"
        case .permissionDenied: return "폴더 접근 권한이 필요합니다"
        case .insufficientStorage: return "저장 공간이 부족합니다"
        case .notAnalyzed: return "아직 분석되지 않은 사진입니다"
        case .unsupportedForVideo: return "동영상에는 사용할 수 없습니다"
        case .cancelled: return "작업이 취소되었습니다"
        case .database: return "카탈로그를 읽지 못했습니다"
        case .io, .other: return "작업을 완료하지 못했습니다"
        }
    }

    var message: String {
        switch self {
        case .librarySuspended:
            return "백그라운드 전환 중 안전하게 연결을 닫았습니다. 다시 열어 이어서 작업할 수 있습니다."
        case .notFound(let detail):
            return detail
        case .permissionDenied(let detail):
            return "외장 드라이브나 폴더 권한이 만료되었습니다. 폴더를 다시 선택해 주세요.\n(\(detail))"
        case .insufficientStorage(let required, let available):
            return "필요한 용량은 \(LoupeFormat.bytes(required))이지만 남은 공간은 \(LoupeFormat.bytes(available))입니다. 휴지통을 비우거나 외장 SSD를 라이브러리로 사용해 보세요."
        case .notAnalyzed:
            return "셀렉 도우미 분석이 끝나면 기울기와 구도를 자동으로 맞출 수 있습니다."
        case .unsupportedForVideo:
            return "자동 수평 · 구도 크롭은 사진에만 적용할 수 있습니다."
        case .cancelled:
            return "요청에 따라 작업을 멈췄습니다."
        case .database(let detail), .io(let detail), .other(let detail):
            return detail
        }
    }
}

struct LoupeCompNote: Equatable, Sendable, Identifiable {
    var id: String { text }
    let text: String
    let ok: Bool?
    let ignored: Bool
}

struct LoupeCompositionOverlay: Equatable, Sendable {
    let hasSubject: Bool
    let subjectX: Float
    let subjectY: Float
    let subjectW: Float
    let subjectH: Float
    let keyX: Float
    let keyY: Float
    let headTop: Float?
    let facing: Float
    let pole: Float
    let poleX: Float
    let neckLine: Float
    let neckY: Float
    let tilt: Float
    let tiltConf: Float
    let tiltVertical: Bool
    let tiltAt: Float
    let showThirds: Bool
    let showHeadroom: Bool
    let showLeadRoom: Bool
    let showMerger: Bool
    let showTilt: Bool
}

struct LoupeRuleItem: Equatable, Sendable, Identifiable {
    var id: Int64 { bit }
    let bit: Int64
    let group: String
    let label: String
    let enabled: Bool
}

struct LoupePhotoSummary: Equatable, Sendable, Identifiable {
    let id: Int64
    let name: String
    let capturedMs: Int64
    var rating: Int
    /// -1 rejected (B컷), 0 undecided, 1 picked (A컷).
    var flag: Int
    let bPct: Int?
    let topPct: Int?
    let burstRank: Int
    let burstSize: Int
    var hasFraming: Bool
    let video: Bool
    let raw: Bool
    let jpeg: Bool
    let width: Int
    let height: Int

    var formatBadge: String? {
        if video { return "동영상" }
        if raw && jpeg { return "RAW+J" }
        if raw { return "RAW" }
        return nil
    }
}

struct LoupePhotoDetail: Equatable, Sendable {
    var summary: LoupePhotoSummary
    let reasons: [String]
    let strengths: [String]
    let scene: String
    let framing: String?
    let compNotes: [LoupeCompNote]
    let overlay: LoupeCompositionOverlay?
    let camera: String
    let lens: String
    let exposure: String
}

struct LoupeCounts: Equatable, Sendable {
    let all: Int
    let undecided: Int
    let candidates: Int
    let keepers: Int
    let rejected: Int
    let picked: Int
    let rated: Int
    let analysed: Int

    static let zero = LoupeCounts(all: 0, undecided: 0, candidates: 0, keepers: 0, rejected: 0, picked: 0, rated: 0, analysed: 0)

    func count(for view: LoupeViewFilter) -> Int {
        switch view {
        case .all: return all
        case .undecided: return undecided
        case .candidates: return candidates
        case .keepers: return keepers
        case .rejected: return rejected
        case .picked: return picked
        case .rated: return rated
        }
    }
}

struct LoupeLearnStatus: Equatable, Sendable {
    let summary: String
    let learned: Bool
    let a: Int
    let b: Int
    let analysed: Int
    let total: Int

    static let initial = LoupeLearnStatus(summary: "기본 기준", learned: false, a: 0, b: 0, analysed: 0, total: 0)
}

struct LoupeSnapshot: Equatable, Sendable {
    let view: LoupeViewFilter
    let ids: [Int64]
    let counts: LoupeCounts
    let learn: LoupeLearnStatus
    let revision: Int64
}

enum LoupePreviewSource: Equatable, Sendable {
    case embeddedSlice(path: String, offset: UInt64, length: UInt64, orientation: Int)
    case renderedJpeg(Data)
}

struct LoupeScanInfo: Equatable, Sendable {
    let label: String
    let photos: Int
    let newPhotos: Int
    let newFiles: Int
    let newBytes: Int64
    let videos: Int
    let rawPairs: Int
    let firstMs: Int64?
    let lastMs: Int64?
    let cameras: [String]
    let errors: [String]
}

struct LoupeImportStatus: Equatable, Sendable {
    let filesDone: Int
    let filesTotal: Int
    let bytesDone: Int64
    let bytesTotal: Int64
    let current: String

    var fraction: Double {
        guard bytesTotal > 0 else {
            return filesTotal > 0 ? Double(filesDone) / Double(filesTotal) : 0
        }
        return min(1.0, max(0.0, Double(bytesDone) / Double(bytesTotal)))
    }
}

struct LoupeImportResult: Equatable, Sendable {
    let added: Int
    let updated: Int
    let duplicates: Int
    let bytes: Int64
    let failures: [String]
    let cancelled: Bool
}

struct LoupeExportPlan: Equatable, Sendable {
    let photos: Int
    let originalsBytes: Int64
    let jpegBytes: Int64
    let rawOnly: Int
    let videos: Int
    let framedCount: Int
}

struct LoupeExportResult: Equatable, Sendable {
    let files: Int
    let fromRaw: Int
    let bytes: Int64
    let skipped: [String]
    let cancelled: Bool
}

struct LoupeDeletePlan: Equatable, Sendable {
    let photos: Int
    let files: Int
    let bytes: Int64
}

struct LoupeDeleteResult: Equatable, Sendable {
    let deleted: Int
    let failed: Int
    let bytes: Int64
    let firstError: String?
}

struct LoupeDecisionRestore: Equatable, Sendable {
    let id: Int64
    let rating: Int
    let flag: Int
}

/// Abstract engine interface enabling 100% deterministic unit testing and SwiftUI previews without
/// touching disk or SQLite.
protocol PhotoEngineProtocol: Sendable {
    var rootPath: String { get }

    func suspend() async throws
    func resume() async throws
    func releaseMLModels() async
    func optimizeStorage() async throws

    func snapshot(view: LoupeViewFilter) async throws -> LoupeSnapshot
    func photosBatch(ids: [Int64]) async throws -> [LoupePhotoSummary]
    func photoDetail(id: Int64) async throws -> LoupePhotoDetail?

    func thumbnailData(id: Int64) async throws -> Data?
    func videoPath(id: Int64) async throws -> String?
    func previewSource(id: Int64, maxEdge: Int) async throws -> LoupePreviewSource

    func setRating(ids: [Int64], rating: Int) async throws -> Int64
    func setFlag(ids: [Int64], flag: Int) async throws -> Int64
    func restoreDecisions(items: [LoupeDecisionRestore]) async throws -> Int64

    func threshold() async -> Int
    func setThreshold(_ percent: Int) async
    func keepPct() async -> Int
    func setKeepPct(_ percent: Int) async
    func disabledRules() async -> Int64
    func setDisabledRules(_ mask: Int64) async
    func rules() async -> [LoupeRuleItem]

    func autoFrame(id: Int64, compose: Bool) async throws -> String
    func resetFraming(id: Int64) async throws

    func scanDirectory(path: String, label: String, includeVideos: Bool) async throws -> LoupeScanInfo
    func importScanned(
        availableBytes: Int64,
        onProgress: @Sendable @escaping (LoupeImportStatus) -> Void
    ) async throws -> LoupeImportResult
    func cancelImport() async

    func pendingAnalysis() async throws -> Int
    func analyze(
        maxThreads: Int,
        onProgress: @Sendable @escaping (Int, Int) -> Void
    ) async throws -> Int
    func cancelAnalysis() async
    func learn() async throws -> LoupeLearnStatus

    func exportPlan(ids: [Int64]) async throws -> LoupeExportPlan
    func exportPhotos(
        ids: [Int64],
        destDir: String,
        mode: LoupeExportMode,
        longEdge: Int,
        quality: Int,
        dateNames: Bool,
        onProgress: @Sendable @escaping (Int, Int) -> Void
    ) async throws -> LoupeExportResult
    func cancelExport() async

    func deletePlan(ids: [Int64]) async throws -> LoupeDeletePlan
    func deletePhotos(ids: [Int64]) async throws -> LoupeDeleteResult
    func trashBytes() async -> Int64
    func emptyTrash() async throws
}
