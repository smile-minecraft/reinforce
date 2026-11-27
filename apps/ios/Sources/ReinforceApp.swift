import SwiftUI

@main
struct ReinforceApp: App {
    @Environment(\.scenePhase) private var scenePhase
    @State private var store: AppStore?
    @State private var initError: String?
    @State private var l10n: LocalizationManager

    init() {
        let args = ProcessInfo.processInfo.arguments
        let launchLang: String? = {
            if let idx = args.firstIndex(of: "-uitest-language"), idx + 1 < args.count {
                return args[idx + 1]
            }
            return nil
        }()
        self._l10n = State(initialValue: LocalizationManager(launchLanguage: launchLang))
        do {
            self._store = State(initialValue: try AppStore())
            self._initError = State(initialValue: nil)
        } catch {
            self._store = State(initialValue: nil)
            self._initError = State(initialValue: error.localizedDescription)
        }
    }

    var body: some Scene {
        WindowGroup {
            if let store {
                RootNavigationView(store: store, l10n: l10n)
                    .onOpenURL { url in
                        store.handleIncomingURL(url)
                    }
                    .onChange(of: scenePhase, initial: true) { _, newPhase in
                        store.syncManager.handleScenePhaseChange(newPhase)
                    }
            } else {
                VStack(spacing: 12) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.largeTitle)
                        .foregroundStyle(ReinforceTheme.bad)
                    Text(l10n.t("common.errorGeneric"))
                        .font(.headline)
                    if let initError {
                        Text(initError)
                            .font(.caption)
                            .foregroundStyle(ReinforceTheme.inkMuted)
                    }
                }
                .padding()
            }
        }
    }
}
