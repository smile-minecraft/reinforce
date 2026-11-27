import SwiftUI
import UIKit

enum ReinforceTheme {
    private static func dynamic(light: UInt32, dark: UInt32) -> Color {
        Color(uiColor: UIColor { traits in
            let hex = traits.userInterfaceStyle == .dark ? dark : light
            let red = CGFloat((hex >> 16) & 0xFF) / 255.0
            let green = CGFloat((hex >> 8) & 0xFF) / 255.0
            let blue = CGFloat(hex & 0xFF) / 255.0
            return UIColor(red: red, green: green, blue: blue, alpha: 1.0)
        })
    }

    static let canvas = dynamic(light: 0xF8F9FA, dark: 0x0D1117)
    static let surface = dynamic(light: 0xFFFFFF, dark: 0x151B23)
    static let subtle = dynamic(light: 0xF1F3F6, dark: 0x1C2430)
    static let border = dynamic(light: 0xDDE2EA, dark: 0x2A3444)
    static let ink = dynamic(light: 0x141923, dark: 0xE6EDF3)
    static let inkMuted = dynamic(light: 0x525C6E, dark: 0x919EAE)

    static let accent = dynamic(light: 0x1D4ED8, dark: 0x60A5FA)
    static let accentSoft = dynamic(light: 0xEFF6FF, dark: 0x172554)

    static let ok = dynamic(light: 0x15803D, dark: 0x4ADE80)
    static let okSoft = dynamic(light: 0xECFDF5, dark: 0x052E16)

    static let bad = dynamic(light: 0xB91C1C, dark: 0xF87171)
    static let badSoft = dynamic(light: 0xFEF2F2, dark: 0x450A0A)

    static let warn = dynamic(light: 0xB45309, dark: 0xFBBF24)
    static let warnSoft = dynamic(light: 0xFFFBEB, dark: 0x451A03)

    static let cardCornerRadius: CGFloat = 14
    static let minTouchTarget: CGFloat = 44
}

struct ReinforceCardModifier: ViewModifier {
    var padding: CGFloat = 16

    func body(content: Content) -> some View {
        content
            .padding(padding)
            .background(ReinforceTheme.surface)
            .clipShape(RoundedRectangle(cornerRadius: ReinforceTheme.cardCornerRadius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: ReinforceTheme.cardCornerRadius, style: .continuous)
                    .stroke(ReinforceTheme.border, lineWidth: 1)
            )
    }
}

extension View {
    func reinforceCard(padding: CGFloat = 16) -> some View {
        modifier(ReinforceCardModifier(padding: padding))
    }
}
