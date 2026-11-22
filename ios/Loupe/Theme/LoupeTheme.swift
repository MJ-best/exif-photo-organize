import SwiftUI

/// LOUPE's signature Cream (light) and Cocoa (dark) palettes, matching desktop and Android tokens.
struct LoupePalette: Sendable {
    let dark: Bool
    let sidebar: Color
    let content: Color
    let toolbar: Color
    let viewer: Color
    let panel: Color
    let text: Color
    let text2: Color
    let text3: Color
    let separator: Color
    let accent: Color
    let selected: Color
    let control: Color
    let star: Color
    let placeholder: Color
    let danger: Color
    let success: Color
    let warning: Color
    let hud: Color

    static let cream = LoupePalette(
        dark: false,
        sidebar: Color(hex: 0xF6EBDA),
        content: Color(hex: 0xFDF6EC),
        toolbar: Color(hex: 0xFBF1E3),
        viewer: Color(hex: 0xF2E8D9),
        panel: Color(hex: 0xFAF0E1),
        text: Color(hex: 0x3D2B21),
        text2: Color(hex: 0x7A6556),
        text3: Color(hex: 0xAE9A89),
        separator: Color(hex: 0xE9D8C2),
        accent: Color(hex: 0xEE6C3A),
        selected: Color(hex: 0xF7D8C0),
        control: Color(hex: 0xF0E2CE),
        star: Color(hex: 0xF4A92E),
        placeholder: Color(hex: 0xF1E4D1),
        danger: Color(hex: 0xE0523C),
        success: Color(hex: 0x5E9E55),
        warning: Color(hex: 0xF29A2E),
        hud: Color(hex: 0x3D2B21, alpha: 0.87)
    )

    static let cocoa = LoupePalette(
        dark: true,
        sidebar: Color(hex: 0x332820),
        content: Color(hex: 0x29201A),
        toolbar: Color(hex: 0x2F251E),
        viewer: Color(hex: 0x1C1612),
        panel: Color(hex: 0x332820),
        text: Color(hex: 0xFBF1E3),
        text2: Color(hex: 0xC8B3A0),
        text3: Color(hex: 0x8C7766),
        separator: Color(hex: 0x483A2F),
        accent: Color(hex: 0xF47A45),
        selected: Color(hex: 0x584130),
        control: Color(hex: 0x45372C),
        star: Color(hex: 0xFCB840),
        placeholder: Color(hex: 0x392D24),
        danger: Color(hex: 0xF06450),
        success: Color(hex: 0x8BC27A),
        warning: Color(hex: 0xF7A53A),
        hud: Color(hex: 0x1C140F, alpha: 0.87)
    )

    static func forColorScheme(_ scheme: ColorScheme) -> LoupePalette {
        scheme == .dark ? .cocoa : .cream
    }
}

private struct LoupePaletteKey: EnvironmentKey {
    static let defaultValue: LoupePalette = .cream
}

extension EnvironmentValues {
    var loupePalette: LoupePalette {
        get { self[LoupePaletteKey.self] }
        set { self[LoupePaletteKey.self] = newValue }
    }
}

extension Color {
    init(hex: UInt32, alpha: Double = 1.0) {
        let r = Double((hex >> 16) & 0xFF) / 255.0
        let g = Double((hex >> 8) & 0xFF) / 255.0
        let b = Double(hex & 0xFF) / 255.0
        self.init(.sRGB, red: r, green: g, blue: b, opacity: alpha)
    }
}

enum LoupeFormat {
    private static let utcCalendar: Calendar = {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = TimeZone(secondsFromGMT: 0) ?? .current
        return cal
    }()

    /// Camera clock timestamps are stored as naive UTC milliseconds.
    static func dateTime(_ ms: Int64) -> String {
        let date = Date(timeIntervalSince1970: Double(ms) / 1000.0)
        let comps = utcCalendar.dateComponents([.year, .month, .day, .hour, .minute], from: date)
        guard let y = comps.year, let m = comps.month, let d = comps.day,
              let h = comps.hour, let min = comps.minute else { return "" }
        return String(format: "%04d. %d. %d. %02d:%02d", y, m, d, h, min)
    }

    static func compactDate(_ ms: Int64) -> String {
        let date = Date(timeIntervalSince1970: Double(ms) / 1000.0)
        let comps = utcCalendar.dateComponents([.year, .month, .day], from: date)
        guard let y = comps.year, let m = comps.month, let d = comps.day else { return "" }
        return String(format: "%04d. %d. %d.", y, m, d)
    }

    static func bytes(_ value: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: max(0, value), countStyle: .file)
    }

    static func count(_ value: Int) -> String {
        NumberFormatter.localizedString(from: NSNumber(value: value), number: .decimal)
    }
}
