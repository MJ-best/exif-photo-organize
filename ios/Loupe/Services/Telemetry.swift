import Foundation
import OSLog

/// Structured Apple unified logging (`OSLog`) and Instruments signpost instrumentation.
/// All telemetry stays 100% on-device; LOUPE performs zero network calls.
enum LoupeLog {
    private static let subsystem = Bundle.main.bundleIdentifier ?? "io.github.mj-best.loupe.ios"

    static let lifecycle = Logger(subsystem: subsystem, category: "lifecycle")
    static let engine = Logger(subsystem: subsystem, category: "engine")
    static let image = Logger(subsystem: subsystem, category: "image")
    static let cull = Logger(subsystem: subsystem, category: "cull")
    static let importer = Logger(subsystem: subsystem, category: "import")
    static let exporter = Logger(subsystem: subsystem, category: "export")
    static let bookmark = Logger(subsystem: subsystem, category: "bookmark")

    static let signposter = OSSignposter(subsystem: subsystem, category: "performance")
}
