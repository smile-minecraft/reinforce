import Foundation
import Observation

enum StudyTimestamp {
    private static let formatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    static func rfc3339(_ date: Date = Date()) -> String {
        formatter.string(from: date)
    }
}

struct ActiveQuizLaunch: Identifiable {
    let id = UUID()
    let session: QuizSessionHandle
    let mode: SessionMode
    let secondsPerQuestion: Int?
    let totalTimeSecs: Int?
}

@MainActor
@Observable
final class AppStore {
    static let localUserIdKey = "reinforce.localUserId"

    let database: LocalDatabase
    let databaseURL: URL
    let blobsURL: URL
    let userId: String
    let uiTestImportPath: String?
    let syncManager: SyncManager

    var subjects: [SubjectSummary] = []
    var notes: [Note] = []
    var knowledgeCards: [KnowledgeCard] = []
    var examSets: [ExamSetSummary] = []
    var dueQuestions: [DueQuestion] = []
    var weaknessReport: WeaknessReport?
    var scheduledCardCount: Int = 0
    var totalAttemptCount: Int = 0

    var selectedSubject: String? = nil
    var importBannerMessage: String? = nil
    var importErrorMessage: String? = nil
    var activeQuiz: ActiveQuizLaunch? = nil
    var pendingDeepLink: DeepLinkTarget? = nil

    private var blobPathCache: [String: URL] = [:]

    init(
        databaseURL: URL? = nil,
        blobsURL: URL? = nil,
        userDefaults: UserDefaults = .standard,
        launchArguments: [String] = ProcessInfo.processInfo.arguments,
        apiClient: ReinforceAPIClient = ReinforceAPIClient(),
        credentialStore: CredentialStoreProtocol = KeychainCredentialStore(),
        networkMonitor: NetworkMonitorProtocol = PathNetworkMonitor()
    ) throws {
        let reset = launchArguments.contains("-uitest-reset")
        let resolvedDB: URL
        let resolvedBlobs: URL

        if let databaseURL, let blobsURL {
            resolvedDB = databaseURL
            resolvedBlobs = blobsURL
        } else {
            let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            let appDir = support.appendingPathComponent("Reinforce", isDirectory: true)
            if reset {
                try? FileManager.default.removeItem(at: appDir)
                userDefaults.removeObject(forKey: Self.localUserIdKey)
            }
            try FileManager.default.createDirectory(at: appDir, withIntermediateDirectories: true)
            resolvedDB = appDir.appendingPathComponent("reinforce.db")
            resolvedBlobs = appDir.appendingPathComponent("blobs", isDirectory: true)
        }

        if let idx = launchArguments.firstIndex(of: "-uitest-import-path"),
           idx + 1 < launchArguments.count {
            self.uiTestImportPath = launchArguments[idx + 1]
        } else {
            self.uiTestImportPath = nil
        }

        self.databaseURL = resolvedDB
        self.blobsURL = resolvedBlobs
        self.database = try LocalDatabase.openWithBlobs(
            databasePath: resolvedDB.path,
            blobsDir: resolvedBlobs.path
        )

        if let savedUser = userDefaults.string(forKey: Self.localUserIdKey), !savedUser.isEmpty {
            self.userId = savedUser
        } else {
            let created = try database.createUser(displayName: "Local Learner")
            userDefaults.set(created, forKey: Self.localUserIdKey)
            self.userId = created
        }

        self.syncManager = SyncManager(
            database: database,
            blobsURL: resolvedBlobs,
            localUserId: userId,
            apiClient: apiClient,
            credentialStore: credentialStore,
            networkMonitor: networkMonitor,
            userDefaults: userDefaults,
            resetState: reset
        )

        self.syncManager.onDataMutated = { [weak self] mediaUpdated in
            guard let self else { return }
            if mediaUpdated {
                MediaAssetCache.shared.clear()
            }
            self.refreshAll()
        }

        if let idx = launchArguments.firstIndex(of: "-uitest-open-url"),
           idx + 1 < launchArguments.count,
           let url = URL(string: launchArguments[idx + 1]) {
            self.pendingDeepLink = DeepLinkParser.parse(url)
        }

        refreshAll()
    }

    var isEmptyLibrary: Bool {
        notes.isEmpty && knowledgeCards.isEmpty && examSets.isEmpty
    }

    var filteredNotes: [Note] {
        guard let selectedSubject else { return notes }
        return notes.filter { $0.subject == selectedSubject }
    }

    var filteredCards: [KnowledgeCard] {
        guard let selectedSubject else { return knowledgeCards }
        return knowledgeCards.filter { $0.subject == selectedSubject }
    }

    func refreshAll() {
        blobPathCache.removeAll()
        do {
            subjects = try database.subjects()
            notes = try database.notes(subject: nil, limit: 500)
            knowledgeCards = try database.knowledgeCards(subject: nil, limit: 1000)
            examSets = try database.examSets(subject: nil, limit: 500)
            dueQuestions = try database.dueQuestions(
                userId: userId,
                now: StudyTimestamp.rfc3339(),
                newCardLimit: 50,
                limit: 200
            )
            let report = try database.weaknessReport(userId: userId, limit: 50)
            weaknessReport = report

            var scheduled = 0
            var attempts = 0
            var seenQuestionIds = Set<String>()
            for summary in examSets {
                let cards = (try? database.questionCards(examSetId: summary.examSet.id, limit: 500)) ?? []
                for q in cards {
                    seenQuestionIds.insert(q.id)
                }
            }
            for q in report.questions {
                seenQuestionIds.insert(q.questionId)
                attempts += Int(q.attempts)
            }
            for qid in seenQuestionIds {
                if let sched = try? database.scheduling(userId: userId, cardId: qid), sched.reps > 0 {
                    scheduled += 1
                }
            }
            if attempts == 0 && scheduled > 0 {
                attempts = scheduled
            }
            scheduledCardCount = scheduled
            totalAttemptCount = attempts
        } catch {
            importErrorMessage = error.localizedDescription
        }
    }

    func importBundle(from url: URL, l10n: LocalizationManager) {
        importErrorMessage = nil
        importBannerMessage = nil

        let didAccess = url.startAccessingSecurityScopedResource()
        defer {
            if didAccess {
                url.stopAccessingSecurityScopedResource()
            }
        }

        do {
            let summary = try database.importBundle(bundlePath: url.path)
            MediaAssetCache.shared.clear()
            refreshAll()
            importBannerMessage = l10n.t("import.successSummary", Int(summary.applied), Int(summary.blobsWritten))
        } catch {
            importErrorMessage = error.localizedDescription
        }
    }

    func importBundleAtPath(_ path: String, l10n: LocalizationManager) {
        importBundle(from: URL(fileURLWithPath: path), l10n: l10n)
    }

    func resolveBlobURL(for resourceId: String) -> URL? {
        if let cached = blobPathCache[resourceId] {
            return cached
        }
        guard let path = try? database.resourceBlobPath(resourceId: resourceId) else {
            return nil
        }
        let url = URL(fileURLWithPath: path)
        blobPathCache[resourceId] = url
        return url
    }

    func loadNoteOutline(noteId: String) throws -> [NoteOutlineNode] {
        try database.noteOutline(noteId: noteId)
    }

    func loadKnowledgeCard(id: String) -> KnowledgeCard? {
        try? database.knowledgeCard(id: id)
    }

    func loadQuestionCard(id: String) -> QuestionCard? {
        try? database.questionCard(id: id)
    }

    func saveUserCardMeta(
        cardId: String,
        starred: Bool,
        personalTags: [String],
        personalNote: String?
    ) {
        _ = try? database.saveUserCardMeta(
            userId: userId,
            cardId: cardId,
            starred: starred,
            personalTags: personalTags,
            personalNote: personalNote
        )
        syncManager.notifyLocalStudyChange()
    }

    func handleIncomingURL(_ url: URL) {
        if let target = DeepLinkParser.parse(url) {
            pendingDeepLink = target
        }
    }

    func consumePendingDeepLink() -> DeepLinkTarget? {
        defer { pendingDeepLink = nil }
        return pendingDeepLink
    }

    func loadNeighbors(cardId: String) -> [GraphNeighbor] {
        (try? database.neighbors(endpoint: .card(id: cardId), depth: 1)) ?? []
    }

    func loadAnchor(id: String) -> Anchor? {
        try? database.anchor(anchorId: id)
    }

    func loadResource(id: String) -> Resource? {
        try? database.resource(resourceId: id)
    }

    func loadQuestionGroup(id: String) -> QuestionGroup? {
        try? database.questionGroup(id: id)
    }

    func searchLibrary(query: String) -> [SearchHit] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return [] }
        return (try? database.search(query: trimmed, limit: 50)) ?? []
    }

    func startDailyDue(
        questionLimit: UInt32 = 20,
        newCardLimit: UInt32 = 10,
        affectsScheduling: Bool = true
    ) throws -> Bool {
        let options = BuildOptions(
            questionLimit: questionLimit,
            newCardLimit: newCardLimit,
            totalTimeSecs: nil,
            secondsPerQuestion: 30,
            affectsScheduling: affectsScheduling,
            pinRadius: 0.06,
            pinAspectRatio: nil
        )
        let session = try database.createDailyDueSession(
            userId: userId,
            now: StudyTimestamp.rfc3339(),
            options: options
        )
        return launch(session: session, mode: .dailyDue, secondsPerQuestion: nil, totalTimeSecs: nil)
    }

    func startFullExam(
        examSetId: String,
        totalTimeSecs: UInt32?,
        affectsScheduling: Bool = true
    ) throws -> Bool {
        let options = BuildOptions(
            questionLimit: 200,
            newCardLimit: 20,
            totalTimeSecs: totalTimeSecs,
            secondsPerQuestion: 30,
            affectsScheduling: affectsScheduling,
            pinRadius: 0.06,
            pinAspectRatio: nil
        )
        let session = try database.createFullExamSession(examSetId: examSetId, options: options)
        return launch(
            session: session,
            mode: .fullExam,
            secondsPerQuestion: nil,
            totalTimeSecs: totalTimeSecs.map(Int.init)
        )
    }

    func startTopicDrill(
        cardIds: [String],
        questionLimit: UInt32 = 30,
        affectsScheduling: Bool = true
    ) throws -> Bool {
        let options = BuildOptions(
            questionLimit: questionLimit,
            newCardLimit: 20,
            totalTimeSecs: nil,
            secondsPerQuestion: 30,
            affectsScheduling: affectsScheduling,
            pinRadius: 0.06,
            pinAspectRatio: nil
        )
        let session = try database.createTopicDrillSession(cardIds: cardIds, options: options)
        return launch(session: session, mode: .topicDrill, secondsPerQuestion: nil, totalTimeSecs: nil)
    }

    func startPracticalExam(
        questionIds: [String] = [],
        secondsPerQuestion: UInt32 = 30,
        pinRadius: Double = 0.06,
        questionLimit: UInt32 = 30,
        affectsScheduling: Bool = true
    ) throws -> Bool {
        let clampedSeconds = min(300, max(1, secondsPerQuestion))
        let options = BuildOptions(
            questionLimit: questionLimit,
            newCardLimit: 20,
            totalTimeSecs: nil,
            secondsPerQuestion: clampedSeconds,
            affectsScheduling: affectsScheduling,
            pinRadius: pinRadius,
            pinAspectRatio: nil
        )
        let session = try database.createPracticalExamSession(questionIds: questionIds, options: options)
        return launch(
            session: session,
            mode: .practicalExam,
            secondsPerQuestion: Int(clampedSeconds),
            totalTimeSecs: nil
        )
    }

    func startWeaknessDrill(
        questionLimit: UInt32 = 20,
        affectsScheduling: Bool = true
    ) throws -> Bool {
        let options = BuildOptions(
            questionLimit: questionLimit,
            newCardLimit: 20,
            totalTimeSecs: nil,
            secondsPerQuestion: 30,
            affectsScheduling: affectsScheduling,
            pinRadius: 0.06,
            pinAspectRatio: nil
        )
        let session = try database.createWeaknessDrillSession(userId: userId, options: options)
        return launch(session: session, mode: .weaknessDrill, secondsPerQuestion: nil, totalTimeSecs: nil)
    }

    private func launch(
        session: QuizSessionHandle,
        mode: SessionMode,
        secondsPerQuestion: Int?,
        totalTimeSecs: Int?
    ) -> Bool {
        guard let questions = try? session.questions(), !questions.isEmpty else {
            return false
        }
        try? session.start(now: StudyTimestamp.rfc3339())
        activeQuiz = ActiveQuizLaunch(
            session: session,
            mode: mode,
            secondsPerQuestion: secondsPerQuestion,
            totalTimeSecs: totalTimeSecs
        )
        return true
    }

    func closeQuizAndRefresh() {
        activeQuiz = nil
        refreshAll()
        syncManager.notifyLocalStudyChange()
    }
}
