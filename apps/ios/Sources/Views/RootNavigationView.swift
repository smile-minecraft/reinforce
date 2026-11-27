import SwiftUI
import UniformTypeIdentifiers

enum PrimarySection: String, Hashable, CaseIterable, Identifiable {
    case reading
    case study
    case practical

    var id: String { rawValue }
}

enum ReadingRoute: Hashable {
    case note(String)
    case card(String)
}

struct RootNavigationView: View {
    let store: AppStore
    let l10n: LocalizationManager

    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @State private var selectedTab: PrimarySection = .reading
    @State private var selectedSplitSection: PrimarySection? = .reading
    @State private var readingPath: [ReadingRoute] = []
    @State private var splitPath: [ReadingRoute] = []
    @State private var showFileImporter = false

    private static var allowedImportTypes: [UTType] {
        var types: [UTType] = [.folder, .json, .plainText]
        if let jsonl = UTType(filenameExtension: "jsonl") {
            types.insert(jsonl, at: 1)
        }
        return types
    }

    var body: some View {
        Group {
            if horizontalSizeClass == .regular {
                iPadSplitShell
            } else {
                iPhoneTabShell
            }
        }
        .fileImporter(
            isPresented: $showFileImporter,
            allowedContentTypes: Self.allowedImportTypes,
            allowsMultipleSelection: false
        ) { result in
            switch result {
            case .success(let urls):
                if let url = urls.first {
                    store.importBundle(from: url, l10n: l10n)
                }
            case .failure(let error):
                store.importErrorMessage = error.localizedDescription
            }
        }
        .fullScreenCover(item: Binding(
            get: { store.activeQuiz },
            set: { if $0 == nil { store.closeQuizAndRefresh() } }
        )) { launch in
            QuizRunnerView(
                viewModel: QuizSessionViewModel(
                    session: launch.session,
                    database: store.database,
                    userId: store.userId,
                    mode: launch.mode,
                    secondsPerQuestion: launch.secondsPerQuestion,
                    totalTimeSecs: launch.totalTimeSecs
                ),
                store: store,
                l10n: l10n,
                onClose: { store.closeQuizAndRefresh() }
            )
        }
    }

    private func triggerImport() {
        if let path = store.uiTestImportPath, !path.isEmpty {
            store.importBundleAtPath(path, l10n: l10n)
        } else {
            showFileImporter = true
        }
    }

    // MARK: - iPad NavigationSplitView

    private var iPadSplitShell: some View {
        NavigationSplitView {
            List(selection: $selectedSplitSection) {
                Section(l10n.t("app.name")) {
                    Label(l10n.t("nav.reading"), systemImage: "book.closed")
                        .tag(PrimarySection.reading)
                        .accessibilityIdentifier("sidebar-reading")
                    Label(l10n.t("nav.study"), systemImage: "checklist")
                        .tag(PrimarySection.study)
                        .accessibilityIdentifier("sidebar-study")
                    Label(l10n.t("nav.practical"), systemImage: "timer")
                        .tag(PrimarySection.practical)
                        .accessibilityIdentifier("sidebar-practical")
                }

                if !store.subjects.isEmpty {
                    Section(l10n.t("nav.subjects")) {
                        Button {
                            store.selectedSubject = nil
                            selectedSplitSection = .reading
                            splitPath.removeAll()
                        } label: {
                            HStack {
                                Text(l10n.t("common.allSubjects"))
                                Spacer()
                                Text("\(store.notes.count)")
                                    .font(.caption.monospacedDigit())
                                    .foregroundStyle(ReinforceTheme.inkMuted)
                            }
                        }

                        ForEach(store.subjects, id: \.subject) { summary in
                            Button {
                                store.selectedSubject = summary.subject
                                selectedSplitSection = .reading
                                splitPath.removeAll()
                            } label: {
                                HStack {
                                    Text(summary.subject)
                                    Spacer()
                                    Text("\(summary.noteCount)")
                                        .font(.caption.monospacedDigit())
                                        .foregroundStyle(ReinforceTheme.inkMuted)
                                }
                            }
                        }
                    }
                }

                if !store.notes.isEmpty {
                    Section(l10n.t("reading.notesHeading")) {
                        ForEach(store.filteredNotes, id: \.id) { note in
                            Button {
                                selectedSplitSection = .reading
                                splitPath = [.note(note.id)]
                            } label: {
                                Label(note.title, systemImage: "doc.text")
                            }
                            .accessibilityIdentifier("sidebar-note-\(note.title)")
                        }
                    }
                }
            }
            .listStyle(.sidebar)
            .navigationTitle(l10n.t("app.name"))
            .toolbar {
                ToolbarItemGroup(placement: .primaryAction) {
                    importToolbarButton
                    languageMenu
                }
            }
            .accessibilityIdentifier("ipad-sidebar")
        } detail: {
            NavigationStack(path: $splitPath) {
                Group {
                    switch selectedSplitSection ?? .reading {
                    case .reading:
                        ReadingHomeView(
                            store: store,
                            l10n: l10n,
                            onRequestImport: triggerImport,
                            onOpenNote: { splitPath.append(.note($0.id)) },
                            onOpenCard: { splitPath.append(.card($0)) }
                        )
                    case .study:
                        StudyHubView(store: store, l10n: l10n, defaultMode: .dailyDue)
                    case .practical:
                        StudyHubView(store: store, l10n: l10n, defaultMode: .practicalExam)
                    }
                }
                .navigationDestination(for: ReadingRoute.self) { route in
                    readingDestination(route, path: $splitPath)
                }
            }
            .onChange(of: selectedSplitSection) { _, _ in
                splitPath.removeAll()
            }
        }
        .navigationSplitViewStyle(.balanced)
    }

    // MARK: - iPhone TabView

    private var iPhoneTabShell: some View {
        TabView(selection: $selectedTab) {
            NavigationStack(path: $readingPath) {
                ReadingHomeView(
                    store: store,
                    l10n: l10n,
                    onRequestImport: triggerImport,
                    onOpenNote: { readingPath.append(.note($0.id)) },
                    onOpenCard: { readingPath.append(.card($0)) }
                )
                .toolbar {
                    ToolbarItemGroup(placement: .topBarTrailing) {
                        importToolbarButton
                        languageMenu
                    }
                }
                .navigationDestination(for: ReadingRoute.self) { route in
                    readingDestination(route, path: $readingPath)
                }
            }
            .tabItem {
                Label(l10n.t("nav.reading"), systemImage: "book.closed")
            }
            .tag(PrimarySection.reading)

            NavigationStack {
                StudyHubView(store: store, l10n: l10n, defaultMode: .dailyDue)
                    .toolbar {
                        ToolbarItemGroup(placement: .topBarTrailing) {
                            importToolbarButton
                            languageMenu
                        }
                    }
            }
            .tabItem {
                Label(l10n.t("nav.study"), systemImage: "checklist")
            }
            .tag(PrimarySection.study)

            NavigationStack {
                StudyHubView(store: store, l10n: l10n, defaultMode: .practicalExam)
                    .toolbar {
                        ToolbarItemGroup(placement: .topBarTrailing) {
                            importToolbarButton
                            languageMenu
                        }
                    }
            }
            .tabItem {
                Label(l10n.t("nav.practical"), systemImage: "timer")
            }
            .tag(PrimarySection.practical)
        }
        .tint(ReinforceTheme.accent)
    }

    @ViewBuilder
    private func readingDestination(_ route: ReadingRoute, path: Binding<[ReadingRoute]>) -> some View {
        switch route {
        case .note(let noteId):
            if let note = store.notes.first(where: { $0.id == noteId }) ?? (try? store.database.note(id: noteId)) {
                NoteDetailView(
                    note: note,
                    store: store,
                    l10n: l10n,
                    onOpenCard: { path.wrappedValue.append(.card($0)) }
                )
            } else {
                Text(l10n.t("common.empty"))
            }
        case .card(let cardId):
            KnowledgeCardDetailView(
                cardId: cardId,
                store: store,
                l10n: l10n,
                onOpenCard: { path.wrappedValue.append(.card($0)) }
            )
        }
    }

    private var importToolbarButton: some View {
        Button {
            triggerImport()
        } label: {
            Label(l10n.t("import.button"), systemImage: "square.and.arrow.down")
        }
        .accessibilityIdentifier("toolbar-import-button")
    }

    private var languageMenu: some View {
        Menu {
            ForEach(AppLanguage.allCases) { option in
                Button {
                    l10n.language = option
                } label: {
                    HStack {
                        Text(languageLabel(option))
                        if l10n.language == option {
                            Image(systemName: "checkmark")
                        }
                    }
                }
            }
        } label: {
            Label(l10n.t("app.language"), systemImage: "globe")
        }
        .accessibilityIdentifier("toolbar-language-menu")
    }

    private func languageLabel(_ lang: AppLanguage) -> String {
        switch lang {
        case .system: return l10n.t("app.language.system")
        case .en: return l10n.t("app.language.en")
        case .zhTW: return l10n.t("app.language.zhTW")
        }
    }
}
