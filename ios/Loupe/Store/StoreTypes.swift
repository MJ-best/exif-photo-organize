import Foundation

/// Explicit state machine types for the unidirectional `LibraryStore`.

struct ViewerSession: Equatable, Identifiable, Sendable {
    let id = UUID()
    let ids: [Int64]
    var index: Int

    var currentID: Int64? {
        guard index >= 0 && index < ids.count else { return nil }
        return ids[index]
    }
}

enum ImportPhase: Equatable, Sendable {
    case idle
    case scanning(label: String)
    case ready(info: LoupeScanInfo, includeVideos: Bool, freeBytes: Int64)
    case copying(status: LoupeImportStatus, label: String)
    case done(LoupeImportResult)
    case failed(LoupeError)
}

enum ExportPhase: Equatable, Sendable {
    case idle
    case planning
    case ready(ids: [Int64], plan: LoupeExportPlan)
    case running(done: Int, total: Int)
    case done(result: LoupeExportResult, outputDirectory: URL)
    case failed(LoupeError)
}

struct DeleteConfirmation: Equatable, Identifiable, Sendable {
    let id = UUID()
    let ids: [Int64]
    let plan: LoupeDeletePlan
}

enum RecoveryActionKind: Equatable, Sendable {
    case reopenLibrary
    case pickLibraryFolder
    case openSettingsToEmptyTrash
    case startAnalysis
}

struct RecoveryAlert: Equatable, Identifiable, Sendable {
    let id = UUID()
    let error: LoupeError
    let primaryActionTitle: String?
    let primaryAction: RecoveryActionKind?

    static func from(_ error: LoupeError) -> RecoveryAlert {
        switch error {
        case .librarySuspended:
            return RecoveryAlert(error: error, primaryActionTitle: "다시 열기", primaryAction: .reopenLibrary)
        case .permissionDenied:
            return RecoveryAlert(error: error, primaryActionTitle: "폴더 다시 선택", primaryAction: .pickLibraryFolder)
        case .insufficientStorage:
            return RecoveryAlert(error: error, primaryActionTitle: "설정에서 휴지통 비우기", primaryAction: .openSettingsToEmptyTrash)
        case .notAnalyzed:
            return RecoveryAlert(error: error, primaryActionTitle: "지금 분석 시작", primaryAction: .startAnalysis)
        default:
            return RecoveryAlert(error: error, primaryActionTitle: nil, primaryAction: nil)
        }
    }
}

struct BottomToast: Equatable, Identifiable, Sendable {
    enum Tone: Equatable, Sendable {
        case neutral
        case positive
        case negative
        case accent
    }

    let id = UUID()
    let text: String
    let tone: Tone
    let undoAvailable: Bool

    init(text: String, tone: Tone, undoAvailable: Bool = false) {
        self.text = text
        self.tone = tone
        self.undoAvailable = undoAvailable
    }
}

struct UndoEntry: Equatable, Identifiable, Sendable {
    let id: UUID
    let label: String
    var priorDecisions: [LoupeDecisionRestore]

    init(id: UUID = UUID(), label: String, priorDecisions: [LoupeDecisionRestore]) {
        self.id = id
        self.label = label
        self.priorDecisions = priorDecisions
    }
}

struct AnalysisProgress: Equatable, Sendable {
    let done: Int
    let total: Int
    let pausedByThermal: Bool
}
