import AppKit
import SwiftUI

/// DBM's dark workbench palette, matching the Tauri UI and the other native
/// frontends.
enum Theme {
    static let defaultConnectionColor = "#38bdf8"
    static let connectionColors = ["#38bdf8", "#22c55e", "#a78bfa", "#f59e0b", "#ef4444", "#64748b"]

    static let bg = Color(hex: "#0b1017")
    static let sidebar = Color(hex: "#0f1722")
    static let panel = Color(hex: "#111924")
    static let panelRaised = Color(hex: "#172231")
    static let panelHover = Color(hex: "#1b2a3d")
    static let editor = Color(hex: "#0b121a")
    static let border = Color(hex: "#253447")
    static let borderStrong = Color(hex: "#344963")
    static let text = Color(hex: "#dbe5f2")
    static let muted = Color(hex: "#7c8ea6")
    static let subtle = Color(hex: "#64748b")
    static let accent = Color(hex: "#38bdf8")
    static let accentStrong = Color(hex: "#0ea5e9")
    static let danger = Color(hex: "#f87171")
    static let success = Color(hex: "#4ade80")
    static let warning = Color(hex: "#fbbf24")
    static let inkOnAccent = Color(hex: "#03121d")

    /// The embedded UI family; AppKit registers it from the bundle's
    /// Resources (see ATSApplicationFontsPath) and falls back to the system
    /// font when `npm run fonts` has not been run.
    private static let uiFamily = "Satoshi Variable"

    static let monoFont = Font.system(size: 12.5, design: .monospaced)
    static let monoNSFont = NSFont.monospacedSystemFont(ofSize: 12.5, weight: .regular)
    static let uiFont = Font.custom(uiFamily, size: 12.5)
    static let smallFont = Font.custom(uiFamily, size: 11)
    static let eyebrowFont = Font.custom(uiFamily, size: 9).weight(.heavy)
    static let titleFont = Font.custom(uiFamily, size: 15).weight(.semibold)
}

extension Color {
    init(hex: String) {
        var value: UInt64 = 0
        let cleaned = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        Scanner(string: cleaned).scanHexInt64(&value)
        if cleaned.count == 6 {
            self.init(
                red: Double((value >> 16) & 0xff) / 255,
                green: Double((value >> 8) & 0xff) / 255,
                blue: Double(value & 0xff) / 255
            )
        } else {
            self = Theme.accent
        }
    }
}

/// Small helpers shared by the views.
enum Workbench {
    static func displayValue(_ value: JSONValue) -> String { value.display }

    static func columnWidth(_ dataType: String) -> CGFloat {
        let lowered = dataType.lowercased()
        if lowered.contains("json") || lowered.contains("array") { return 320 }
        if lowered.contains("text") || lowered.contains("character") || lowered.contains("timestamp") {
            return 220
        }
        return 160
    }

    static func compact(_ sql: String, limit: Int = 60) -> String {
        let compacted = sql.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        if compacted.count <= limit { return compacted }
        return String(compacted.prefix(limit)) + "…"
    }

    static func safeFileName(_ value: String) -> String {
        let invalid = CharacterSet(charactersIn: "\\/:*?\"<>|")
        return String(value.unicodeScalars.map { invalid.contains($0) ? "_" : Character($0) })
    }
}

/// DBM's button styles.
struct DBMButtonStyle: ButtonStyle {
    enum Kind {
        case primary
        case secondary
        case danger
        case link
    }

    let kind: Kind

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(kind == .link ? Theme.smallFont : Theme.uiFont)
            .fontWeight(kind == .primary ? .semibold : .regular)
            .foregroundStyle(foreground)
            .padding(.horizontal, kind == .link ? 4 : 11)
            .padding(.vertical, kind == .link ? 2 : 5)
            .background(background.opacity(configuration.isPressed ? 0.8 : 1))
            .overlay(
                RoundedRectangle(cornerRadius: 6)
                    .strokeBorder(border, lineWidth: kind == .secondary || kind == .danger ? 1 : 0)
            )
            .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    private var foreground: Color {
        switch kind {
        case .primary: return Theme.inkOnAccent
        case .secondary: return Theme.text
        case .danger: return Theme.danger
        case .link: return Theme.accent
        }
    }

    private var background: Color {
        switch kind {
        case .primary: return Theme.accentStrong
        case .secondary: return Theme.panelRaised
        case .danger: return Theme.panelRaised
        case .link: return .clear
        }
    }

    private var border: Color {
        switch kind {
        case .secondary: return Theme.borderStrong
        case .danger: return Theme.danger
        default: return .clear
        }
    }
}

struct PanelBackground: ViewModifier {
    func body(content: Content) -> some View {
        content
            .background(Theme.bg)
            .foregroundStyle(Theme.text)
    }
}

extension View {
    func dbmPanel() -> some View { modifier(PanelBackground()) }
}
