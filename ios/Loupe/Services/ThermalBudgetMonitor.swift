import Foundation
import Observation

/// Computes a safe concurrency budget for Rust ONNX photo analysis based on iOS/iPadOS thermal
/// state, Low Power Mode, and available RAM/cores. Prevents Jetsam OOM and thermal throttling.
enum ThermalBudgetPolicy {
    /// Returns `0` when analysis should pause (`.critical` thermal state), `1` under thermal or
    /// battery pressure or low-RAM devices, and `2`..`3` on healthy devices.
    static func recommendedThreads(
        thermalState: ProcessInfo.ThermalState,
        lowPowerMode: Bool,
        physicalMemoryBytes: UInt64,
        activeProcessorCount: Int
    ) -> Int {
        switch thermalState {
        case .critical:
            return 0
        case .serious:
            return 1
        case .fair:
            return 1
        case .nominal:
            if lowPowerMode { return 1 }
            let gb = physicalMemoryBytes / (1024 * 1024 * 1024)
            // M-series iPad Pro (8GB+ RAM, 8+ cores) can comfortably run 3 concurrent workers;
            // standard iPhones (6GB) cap at 2; older 4GB devices stay at 1 to prevent Jetsam.
            if gb >= 8 && activeProcessorCount >= 8 {
                return 3
            } else if gb >= 6 && activeProcessorCount >= 4 {
                return 2
            } else {
                return 1
            }
        @unknown default:
            return 1
        }
    }

    static func statusDescription(thermalState: ProcessInfo.ThermalState, lowPowerMode: Bool) -> String? {
        switch thermalState {
        case .critical:
            return "기기 온도가 높아 분석을 잠시 멈췄습니다"
        case .serious:
            return "발열을 줄이려고 천천히 분석 중"
        default:
            return lowPowerMode ? "저전력 모드라 천천히 분석 중" : nil
        }
    }
}

@MainActor
@Observable
final class ThermalBudgetMonitor {
    private(set) var thermalState: ProcessInfo.ThermalState = ProcessInfo.processInfo.thermalState
    private(set) var lowPowerMode: Bool = ProcessInfo.processInfo.isLowPowerModeEnabled
    private let observers = NotificationObserverBag()

    var recommendedThreads: Int {
        ThermalBudgetPolicy.recommendedThreads(
            thermalState: thermalState,
            lowPowerMode: lowPowerMode,
            physicalMemoryBytes: ProcessInfo.processInfo.physicalMemory,
            activeProcessorCount: ProcessInfo.processInfo.activeProcessorCount
        )
    }

    var notice: String? {
        ThermalBudgetPolicy.statusDescription(thermalState: thermalState, lowPowerMode: lowPowerMode)
    }

    var onThermalChange: ((Int) -> Void)?

    init() {
        let center = NotificationCenter.default
        observers.add(
            center.addObserver(
                forName: ProcessInfo.thermalStateDidChangeNotification,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in
                    self?.refresh()
                }
            }
        )
        observers.add(
            center.addObserver(
                forName: .NSProcessInfoPowerStateDidChange,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in
                    self?.refresh()
                }
            }
        )
    }

    private func refresh() {
        thermalState = ProcessInfo.processInfo.thermalState
        lowPowerMode = ProcessInfo.processInfo.isLowPowerModeEnabled
        let budget = recommendedThreads
        LoupeLog.cull.info("Thermal budget updated: threads=\(budget) lowPower=\(self.lowPowerMode)")
        onThermalChange?(budget)
    }
}

private final class NotificationObserverBag: @unchecked Sendable {
    private let lock = NSLock()
    private var tokens: [NSObjectProtocol] = []

    func add(_ token: NSObjectProtocol) {
        lock.lock()
        tokens.append(token)
        lock.unlock()
    }

    deinit {
        lock.lock()
        let toRemove = tokens
        tokens.removeAll()
        lock.unlock()
        for token in toRemove {
            NotificationCenter.default.removeObserver(token)
        }
    }
}
