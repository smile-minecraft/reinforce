import Foundation
import Observation

enum AppLanguage: String, CaseIterable, Identifiable {
    case system
    case en
    case zhTW = "zh-Hant"

    var id: String { rawValue }

    var resolvedCode: String {
        switch self {
        case .en:
            return "en"
        case .zhTW:
            return "zh-Hant"
        case .system:
            let preferred = Locale.preferredLanguages.first?.lowercased() ?? "en"
            return preferred.hasPrefix("zh") ? "zh-Hant" : "en"
        }
    }
}

@MainActor
@Observable
final class LocalizationManager {
    static let storageKey = "reinforce.appLanguage"

    var language: AppLanguage {
        didSet {
            UserDefaults.standard.set(language.rawValue, forKey: Self.storageKey)
        }
    }

    init(launchLanguage: String? = nil) {
        if let launchLanguage {
            if launchLanguage.lowercased().hasPrefix("zh") {
                self.language = .zhTW
                return
            } else if launchLanguage.lowercased().hasPrefix("en") {
                self.language = .en
                return
            }
        }
        if let saved = UserDefaults.standard.string(forKey: Self.storageKey),
           let parsed = AppLanguage(rawValue: saved) {
            self.language = parsed
        } else {
            self.language = .system
        }
    }

    var activeBundle: Bundle {
        let code = language.resolvedCode
        if let path = Bundle.main.path(forResource: code, ofType: "lproj"),
           let bundle = Bundle(path: path) {
            return bundle
        }
        return Bundle.main
    }

    func t(_ key: String) -> String {
        let localized = activeBundle.localizedString(forKey: key, value: nil, table: nil)
        if localized != key {
            return localized
        }
        if let fallbackPath = Bundle.main.path(forResource: "en", ofType: "lproj"),
           let fallbackBundle = Bundle(path: fallbackPath) {
            return fallbackBundle.localizedString(forKey: key, value: key, table: nil)
        }
        return key
    }

    func t(_ key: String, _ args: CVarArg...) -> String {
        let format = t(key)
        return String(format: format, locale: Locale(identifier: language.resolvedCode), arguments: args)
    }
}
