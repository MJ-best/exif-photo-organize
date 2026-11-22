import Foundation
import Observation

/// Dedicated domain coordinator for thermal-budgeted on-device AI culling analysis.
///
/// Key invariants:
/// - Tracks `hasPendingBackgroundAnalysis` independently of `progress` so cancelling foreground
///   analysis during `.background` transition never prevents `BGProcessingTask` scheduling.
/// - Refreshes the visible snapshot incrementally every 64 analyzed photos so B-cut/A-cut
///   candidates appear live during large SD card imports.
/// - Automatically pauses on `.critical` thermal state and resumes once the device cools.
@MainActor
@Observable
final class AnalysisCoordinator {
    private(set) var progress: AnalysisProgress?
    private(set) var hasPendingBackgroundAnalysis: Bool = false

    let thermalMonitor: ThermalBudgetMonitor

    private let engineProvider: () -> (any PhotoEngineProtocol)?
    private let isSuspendedProvider: () -> Bool
    private let onChunkRefresh: () async -> Void
    private let onLearnStatusUpdated: (LoupeLearnStatus) -> Void
    private let onToast: (String, BottomToast.Tone) -> Void

    private var analysisTask: Task<Void, Never>?
    private var analysisRunToken: UInt64 = 0
    private var lastLiveRefreshDone: Int = 0

    init(
        thermalMonitor: ThermalBudgetMonitor? = nil,
        engineProvider: @escaping () -> (any PhotoEngineProtocol)?,
        isSuspendedProvider: @escaping () -> Bool,
        onChunkRefresh: @escaping () async -> Void,
        onLearnStatusUpdated: @escaping (LoupeLearnStatus) -> Void,
        onToast: @escaping (String, BottomToast.Tone) -> Void
    ) {
        self.thermalMonitor = thermalMonitor ?? ThermalBudgetMonitor()
        self.engineProvider = engineProvider
        self.isSuspendedProvider = isSuspendedProvider
        self.onChunkRefresh = onChunkRefresh
        self.onLearnStatusUpdated = onLearnStatusUpdated
        self.onToast = onToast

        self.thermalMonitor.onThermalChange = { [weak self] budget in
            Task { @MainActor [weak self] in
                await self?.handleThermalBudgetChange(budget)
            }
        }
    }

    func startIfNeeded() {
        guard let engine = engineProvider(), !isSuspendedProvider() else { return }
        guard analysisTask == nil || analysisTask?.isCancelled == true else { return }

        let budget = thermalMonitor.recommendedThreads
        if budget == 0 {
            hasPendingBackgroundAnalysis = true
            progress = AnalysisProgress(done: 0, total: 0, pausedByThermal: true)
            return
        }

        analysisRunToken &+= 1
        let runToken = analysisRunToken
        lastLiveRefreshDone = 0

        analysisTask = Task { [weak self] in
            guard let self else { return }
            defer {
                if self.analysisRunToken == runToken {
                    self.analysisTask = nil
                }
            }
            do {
                let pending = try await engine.pendingAnalysis()
                guard !Task.isCancelled, self.analysisRunToken == runToken else { return }
                if pending == 0 {
                    self.hasPendingBackgroundAnalysis = false
                    if let status = try? await engine.learn() {
                        guard self.analysisRunToken == runToken else { return }
                        self.onLearnStatusUpdated(status)
                        await self.onChunkRefresh()
                    }
                    self.progress = nil
                    return
                }
                self.hasPendingBackgroundAnalysis = true
                self.progress = AnalysisProgress(done: 0, total: pending, pausedByThermal: false)
                let completed = try await engine.analyze(maxThreads: budget) { [weak self] done, total in
                    Task { @MainActor [weak self] in
                        guard let self, self.analysisRunToken == runToken, self.analysisTask != nil else { return }
                        self.progress = AnalysisProgress(done: done, total: total, pausedByThermal: false)
                        if done > 0 && (done - self.lastLiveRefreshDone) >= 64 {
                            self.lastLiveRefreshDone = done
                            await self.onChunkRefresh()
                        }
                    }
                }
                guard self.analysisRunToken == runToken else { return }
                self.progress = nil
                if !Task.isCancelled && completed >= pending {
                    self.hasPendingBackgroundAnalysis = false
                }
                if !self.isSuspendedProvider() {
                    await self.onChunkRefresh()
                }
            } catch {
                guard self.analysisRunToken == runToken else { return }
                self.progress = nil
                let mapped = Self.mapError(error)
                if mapped != .cancelled && mapped != .librarySuspended {
                    self.onToast("셀렉 분석 중단: \(mapped.message)", .negative)
                }
            }
        }
    }

    /// Cancels active foreground analysis before SQLite suspension while preserving
    /// `hasPendingBackgroundAnalysis` so `LoupeApp` reliably schedules `BGProcessingTask`.
    func prepareForBackground() async {
        let shouldSchedule = (progress != nil || analysisTask != nil || hasPendingBackgroundAnalysis)
        hasPendingBackgroundAnalysis = shouldSchedule
        analysisRunToken &+= 1
        analysisTask?.cancel()
        analysisTask = nil
        progress = nil
        if let engine = engineProvider() {
            await engine.cancelAnalysis()
        }
    }

    func markBackgroundAnalysisCompleted() {
        hasPendingBackgroundAnalysis = false
    }

    func resetForLibrarySwitch() {
        analysisRunToken &+= 1
        analysisTask?.cancel()
        analysisTask = nil
        progress = nil
        hasPendingBackgroundAnalysis = false
        lastLiveRefreshDone = 0
    }

    private func handleThermalBudgetChange(_ budget: Int) async {
        guard let engine = engineProvider() else { return }
        if budget == 0 {
            guard analysisTask != nil else { return }
            hasPendingBackgroundAnalysis = true
            analysisRunToken &+= 1
            await engine.cancelAnalysis()
            analysisTask?.cancel()
            analysisTask = nil
            progress = AnalysisProgress(
                done: progress?.done ?? 0,
                total: progress?.total ?? 0,
                pausedByThermal: true
            )
            onToast("기기 온도가 높아 분석을 잠시 멈췄습니다", .neutral)
        } else if progress?.pausedByThermal == true {
            startIfNeeded()
        }
    }

    private static func mapError(_ error: Error) -> LoupeError {
        if let loupe = error as? LoupeError { return loupe }
        return .other(error.localizedDescription)
    }
}
