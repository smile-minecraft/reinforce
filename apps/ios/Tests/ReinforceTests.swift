import SwiftUI
import XCTest
@testable import Reinforce

final class ReinforceTests: XCTestCase {
    private var tempDir: URL!

    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("reinforce-ios-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        if let tempDir {
            try? FileManager.default.removeItem(at: tempDir)
        }
    }

    private func fixtureURL(_ name: String) throws -> URL {
        let bundle = Bundle(for: Self.self)
        guard let url = bundle.url(forResource: name, withExtension: nil) else {
            throw XCTSkip("Missing test fixture folder: \(name)")
        }
        return url
    }

    func testDemoBundleImportAndCatalog() throws {
        let dbPath = tempDir.appendingPathComponent("reinforce.db").path
        let blobsPath = tempDir.appendingPathComponent("blobs").path
        let db = try LocalDatabase.openWithBlobs(databasePath: dbPath, blobsDir: blobsPath)

        let demoDir = try fixtureURL("demo")
        let summary = try db.importBundle(bundlePath: demoDir.path)
        XCTAssertGreaterThan(summary.applied, 0)
        XCTAssertEqual(summary.blobsWritten, 2)

        let subjects = try db.subjects()
        XCTAssertEqual(subjects.map(\.subject), ["physics"])

        let notes = try db.notes(subject: "physics", limit: 20)
        XCTAssertEqual(notes.count, 1)
        XCTAssertEqual(notes[0].title, "相位變化入門")

        let outline = try db.noteOutline(noteId: notes[0].id)
        XCTAssertFalse(outline.isEmpty)

        let cards = try db.knowledgeCards(subject: "physics", limit: 50)
        XCTAssertEqual(cards.count, 3)

        let pinCard = try XCTUnwrap(cards.first(where: { $0.title == "相位圖上的特徵點" }))
        let blocks = ContentBlock.decodeArray(from: pinCard.blocksJson)
        guard case .interactivePins(let resourceId, let overlayId, let pins) = blocks.first else {
            return XCTFail("Expected interactive_pins block")
        }
        XCTAssertEqual(pins.count, 2)
        XCTAssertNotNil(overlayId)

        let baseBlobPath = try db.resourceBlobPath(resourceId: resourceId)
        let loaded = LoadedMediaAsset.load(from: URL(fileURLWithPath: baseBlobPath))
        XCTAssertNotNil(loaded)
        if let loaded {
            XCTAssertEqual(loaded.aspectRatio, 800.0 / 500.0, accuracy: 0.01)
        }

        let neighbors = try db.neighbors(endpoint: .card(id: cards[0].id), depth: 1)
        XCTAssertFalse(neighbors.isEmpty)
    }

    func testAllFiveQuizModesAndPersistenceAcrossReopen() throws {
        let dbPath = tempDir.appendingPathComponent("persist.db").path
        let blobsPath = tempDir.appendingPathComponent("blobs").path
        let userId: String

        do {
            let db = try LocalDatabase.openWithBlobs(databasePath: dbPath, blobsDir: blobsPath)
            _ = try db.importBundle(bundlePath: try fixtureURL("demo").path)
            _ = try db.importBundle(bundlePath: try fixtureURL("practical_demo").path)
            userId = try db.createUser(displayName: "Offline Tester")

            let defaults = BuildOptions(
                questionLimit: 20,
                newCardLimit: 20,
                totalTimeSecs: nil,
                secondsPerQuestion: 30,
                affectsScheduling: true,
                pinRadius: 0.08,
                pinAspectRatio: nil
            )

            // 1. Daily Due
            let daily = try db.createDailyDueSession(userId: userId, now: StudyTimestamp.rfc3339(), options: defaults)
            XCTAssertFalse(try daily.questions().isEmpty)

            // 2. Full Exam (deferred grading)
            let examSets = try db.examSets(subject: nil, limit: 10)
            let examSetId = try XCTUnwrap(examSets.first?.examSet.id)
            let fullExam = try db.createFullExamSession(examSetId: examSetId, options: defaults)
            try fullExam.start(now: StudyTimestamp.rfc3339())
            let examQuestions = try fullExam.questions()
            XCTAssertEqual(examQuestions.count, 4)

            // Answer Q1 right, Q2 wrong (so weakness report has data), Q3 right, Q4 voided
            let g1 = try fullExam.submitAnswer(
                answer: QuizAnswer(submission: .keys(values: ["A"]), confidence: .certain, errorTags: []),
                now: StudyTimestamp.rfc3339()
            )
            XCTAssertNil(g1, "Full exam defers grading until finish")
            _ = try fullExam.submitAnswer(
                answer: QuizAnswer(submission: .keys(values: ["B"]), confidence: .guessed, errorTags: ["concept_gap"]),
                now: StudyTimestamp.rfc3339()
            )
            _ = try fullExam.submitAnswer(
                answer: QuizAnswer(submission: .text(value: "boiling"), confidence: .certain, errorTags: []),
                now: StudyTimestamp.rfc3339()
            )
            _ = try fullExam.submitAnswer(
                answer: QuizAnswer(submission: .text(value: "-"), confidence: .certain, errorTags: []),
                now: StudyTimestamp.rfc3339()
            )

            let written = try db.commitSession(session: fullExam, userId: userId, now: StudyTimestamp.rfc3339())
            XCTAssertEqual(written, 4)
            let score = try fullExam.score()
            XCTAssertEqual(score.correct, 2)
            XCTAssertEqual(score.scoreable, 3)
            XCTAssertEqual(score.voided, 1)

            // 3. Topic Drill
            let cards = try db.knowledgeCards(subject: "physics", limit: 20)
            let tableCard = try XCTUnwrap(cards.first(where: { $0.title == "水的三態與相變" }))
            let topic = try db.createTopicDrillSession(cardIds: [tableCard.id], options: defaults)
            XCTAssertFalse(try topic.questions().isEmpty)

            // 4. Practical Exam (pin_identify + pin_locate)
            let practical = try db.createPracticalExamSession(questionIds: [], options: defaults)
            let practicalQuestions = try practical.questions()
            XCTAssertEqual(practicalQuestions.count, 2)
            try practical.start(now: StudyTimestamp.rfc3339())
            let pinIdOutcome = try practical.submitAnswer(
                answer: QuizAnswer(submission: .text(value: "triple point"), confidence: .certain, errorTags: []),
                now: StudyTimestamp.rfc3339()
            )
            XCTAssertEqual(pinIdOutcome?.isCorrect, true)
            let pinLocOutcome = try practical.submitAnswer(
                answer: QuizAnswer(
                    submission: .pinClicks(clicks: [PinClick(no: 2, point: NormalizedPoint(x: 0.75, y: 0.25))]),
                    confidence: .certain,
                    errorTags: []
                ),
                now: StudyTimestamp.rfc3339()
            )
            XCTAssertEqual(pinLocOutcome?.isCorrect, true)

            // 5. Weakness Drill
            let weaknessSession = try db.createWeaknessDrillSession(userId: userId, options: defaults)
            XCTAssertFalse(try weaknessSession.questions().isEmpty)
        }

        // Reopen database from disk to verify persistence across app restart
        let reopened = try LocalDatabase.openWithBlobs(databasePath: dbPath, blobsDir: blobsPath)
        let notes = try reopened.notes(subject: nil, limit: 20)
        XCTAssertEqual(notes.count, 1)

        let report = try reopened.weaknessReport(userId: userId, limit: 20)
        XCTAssertFalse(report.questions.isEmpty)

        let examSets = try reopened.examSets(subject: nil, limit: 10)
        let questions = try reopened.questionCards(examSetId: examSets[0].examSet.id, limit: 20)
        let sched = try reopened.scheduling(userId: userId, cardId: questions[0].id)
        XCTAssertNotNil(sched)
        XCTAssertGreaterThanOrEqual(sched?.reps ?? 0, 1)
    }

    @MainActor
    func testPracticalExamCountdownPausesInBackground() throws {
        let dbPath = tempDir.appendingPathComponent("practical-pause.db").path
        let blobsPath = tempDir.appendingPathComponent("blobs").path
        let db = try LocalDatabase.openWithBlobs(databasePath: dbPath, blobsDir: blobsPath)
        _ = try db.importBundle(bundlePath: try fixtureURL("practical_demo").path)
        let userId = try db.createUser(displayName: "Timer Tester")

        var currentDate = Date(timeIntervalSince1970: 1_700_000_000)
        let options = BuildOptions(
            questionLimit: 10,
            newCardLimit: 10,
            totalTimeSecs: nil,
            secondsPerQuestion: 30,
            affectsScheduling: false,
            pinRadius: 0.06,
            pinAspectRatio: nil
        )
        let session = try db.createPracticalExamSession(questionIds: [], options: options)
        try session.start(now: StudyTimestamp.rfc3339(currentDate))

        let vm = QuizSessionViewModel(
            session: session,
            database: db,
            userId: userId,
            mode: .practicalExam,
            secondsPerQuestion: 30,
            totalTimeSecs: nil,
            nowProvider: { currentDate },
            autoStartTimer: false
        )

        // 5 seconds pass while active
        currentDate = currentDate.addingTimeInterval(5)
        vm.tickOnce(at: currentDate)
        XCTAssertEqual(vm.questionRemainingSecs, 25, accuracy: 0.2)
        if case .inProgress(let idx) = vm.state {
            XCTAssertEqual(idx, 0)
        } else {
            return XCTFail("Expected inProgress state before background")
        }

        // App enters background -> must pause countdown
        vm.handleScenePhaseChange(.background)
        XCTAssertTrue(vm.pausedByBackground)
        if case .paused(let idx) = vm.state {
            XCTAssertEqual(idx, 0)
        } else {
            return XCTFail("Expected paused state in background")
        }

        // 120 seconds pass in background (4x the 30s limit!)
        currentDate = currentDate.addingTimeInterval(120)
        vm.tickOnce(at: currentDate)
        XCTAssertEqual(vm.questionRemainingSecs, 25, accuracy: 0.2)

        // User returns and resumes -> timer must NOT have timed out
        vm.resumeSession()
        vm.stopTimer()
        currentDate = currentDate.addingTimeInterval(2)
        vm.tickOnce(at: currentDate)

        if case .inProgress(let idx) = vm.state {
            XCTAssertEqual(idx, 0, "Question must remain on index 0 after background pause")
        } else {
            return XCTFail("Expected inProgress state after resuming from background, got \(vm.state)")
        }
        XCTAssertNil(vm.feedback, "Question must not have timed out while backgrounded")
    }

    func testMarkdownParserTablesAndCallouts() {
        let sample = """
        # Heading

        | 相變 | 吸放熱 | 常見例子 |
        | --- | --- | --- |
        | 熔化 | 吸熱 | 冰變成水 |
        | 汽化 | 吸熱 | 水變成蒸氣 |

        > [!tip] Remember
        > Latent heat changes phase without raising temperature.
        """
        let elements = MarkdownParser.parse(sample)
        XCTAssertEqual(elements.count, 3)

        guard case .table(_, let headers, let rows) = elements[1] else {
            return XCTFail("Expected parsed table at index 1")
        }
        XCTAssertEqual(headers, ["相變", "吸放熱", "常見例子"])
        XCTAssertEqual(rows.count, 2)
        XCTAssertEqual(rows[0], ["熔化", "吸熱", "冰變成水"])

        guard case .callout(_, let kind, let title, let isOpen, let body) = elements[2] else {
            return XCTFail("Expected parsed callout at index 2")
        }
        XCTAssertEqual(kind, "tip")
        XCTAssertEqual(title, "Remember")
        XCTAssertTrue(isOpen)
        XCTAssertTrue(body.contains("Latent heat"))
    }

    func testLatexToMathMLConversion() {
        let mathML = LatexToMathML.renderMathML(latex: #"\frac{dP}{dT}=\frac{L}{T\Delta v}"#, display: true)
        XCTAssertTrue(mathML.contains("<math"))
        XCTAssertTrue(mathML.contains("<mfrac>"))
        XCTAssertTrue(mathML.contains("\u{0394}"))

        let chem = LatexToMathML.renderMathML(latex: #"\ce{H2O -> H+ + OH-}"#, display: false)
        XCTAssertTrue(chem.contains("<msub>"))
        XCTAssertTrue(chem.contains("\u{2192}"))
    }

    func testLocalizationParityEnAndZhTW() throws {
        let appBundle = Bundle(for: AppStore.self)
        let enPath = try XCTUnwrap(appBundle.path(forResource: "Localizable", ofType: "strings", inDirectory: "en.lproj"))
        let zhPath = try XCTUnwrap(appBundle.path(forResource: "Localizable", ofType: "strings", inDirectory: "zh-Hant.lproj"))

        let enDict = try XCTUnwrap(NSDictionary(contentsOfFile: enPath) as? [String: String])
        let zhDict = try XCTUnwrap(NSDictionary(contentsOfFile: zhPath) as? [String: String])

        let enKeys = Set(enDict.keys)
        let zhKeys = Set(zhDict.keys)

        XCTAssertEqual(enKeys.subtracting(zhKeys), [], "Missing zh-Hant keys")
        XCTAssertEqual(zhKeys.subtracting(enKeys), [], "Missing en keys")
        XCTAssertFalse(enKeys.isEmpty)

        for (key, value) in enDict {
            XCTAssertFalse(value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, "Empty en value for \(key)")
        }
        for (key, value) in zhDict {
            XCTAssertFalse(value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, "Empty zh-Hant value for \(key)")
        }
    }
}
