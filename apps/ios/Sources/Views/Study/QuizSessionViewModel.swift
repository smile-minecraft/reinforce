import Foundation
import Observation
import SwiftUI

struct QuizReviewEntry: Identifiable {
    let id = UUID()
    let index: Int
    let question: QuestionCard
    let submission: AnswerSubmission
    let confidence: Confidence
    var errorTags: [String]
    var grading: GradingOutcome?
}

struct ImmediateFeedback {
    let question: QuestionCard
    let submission: AnswerSubmission
    let grading: GradingOutcome
    let nextState: SessionState
    let timedOut: Bool
}

struct CompletedQuizSummary {
    let score: Score
    let attemptsWritten: UInt32
    let entries: [QuizReviewEntry]
}

@MainActor
@Observable
final class QuizSessionViewModel {
    let session: QuizSessionHandle
    let database: LocalDatabase
    let userId: String
    let mode: SessionMode
    let perQuestionLimitSecs: Int?
    let totalExamLimitSecs: Int?
    private let nowProvider: () -> Date

    var questions: [QuestionCard] = []
    var state: SessionState = .notStarted
    var currentQuestion: QuestionCard? = nil
    var currentQuestionGroup: QuestionGroup? = nil

    // Draft answer state
    var selectedKeys: [String] = []
    var textAnswer: String = ""
    var pinClicks: [PinClick] = []
    var confidence: Confidence = .certain
    var errorTags: [String] = []

    // Flow state
    var feedback: ImmediateFeedback? = nil
    var deferredNoticeVisible: Bool = false
    var actionError: String? = nil
    var history: [QuizReviewEntry] = []
    var summary: CompletedQuizSummary? = nil

    // Timer & pause state
    var questionRemainingSecs: Double
    var totalRemainingSecs: Double
    var pausedForReview: Bool = false
    var pausedByBackground: Bool = false

    private var timerTask: Task<Void, Never>?
    private var lastFrameDate: Date?

    init(
        session: QuizSessionHandle,
        database: LocalDatabase,
        userId: String,
        mode: SessionMode,
        secondsPerQuestion: Int?,
        totalTimeSecs: Int?,
        nowProvider: @escaping () -> Date = Date.init,
        autoStartTimer: Bool = true
    ) {
        self.session = session
        self.database = database
        self.userId = userId
        self.mode = mode
        self.perQuestionLimitSecs = mode == .practicalExam ? (secondsPerQuestion ?? 30) : nil
        self.totalExamLimitSecs = mode == .fullExam ? totalTimeSecs : nil
        self.questionRemainingSecs = Double(mode == .practicalExam ? (secondsPerQuestion ?? 30) : 30)
        self.totalRemainingSecs = Double(totalTimeSecs ?? 0)
        self.nowProvider = nowProvider
        self.lastFrameDate = nowProvider()

        syncFromSession()
        if autoStartTimer {
            startTimerLoopIfNeeded()
        }
    }

    var isPractical: Bool { mode == .practicalExam }
    var isFullExam: Bool { mode == .fullExam }
    var isTimedMode: Bool { perQuestionLimitSecs != nil || totalExamLimitSecs != nil }

    var isPausedByUserOrBackground: Bool {
        if case .paused = state, !pausedForReview {
            return true
        }
        return false
    }

    var isExpired: Bool {
        if case .expired = state { return true }
        return false
    }

    var isSubmitted: Bool {
        if case .submitted = state { return true }
        return false
    }

    var activeQuestion: QuestionCard? {
        feedback?.question ?? currentQuestion
    }

    var currentIndex: Int {
        switch state {
        case .inProgress(let index), .paused(let index), .expired(let index):
            return Int(index)
        case .submitted:
            return questions.count
        case .notStarted:
            return 0
        }
    }

    var displayIndex: Int {
        if let feedback, let last = history.last {
            _ = feedback
            return last.index
        }
        return currentIndex
    }

    var progressFraction: Double {
        guard !questions.isEmpty else { return 1.0 }
        let completed = Double(displayIndex + (feedback != nil ? 1 : 0))
        return min(1.0, max(0.0, completed / Double(questions.count)))
    }

    var practicalMedia: PracticalMediaInfo? {
        guard let q = activeQuestion else { return nil }
        return ContentBlock.decodeArray(from: q.stemBlocksJson).practicalMedia
    }

    var requiredPinNos: [Int] {
        guard let q = activeQuestion, q.questionType == .pinLocate else { return [] }
        let answerNos = q.answer.compactMap { Int($0.trimmingCharacters(in: .whitespaces)) }
        if !answerNos.isEmpty { return answerNos }
        let stemNos = practicalMedia?.pins.map(\.no) ?? []
        return stemNos.isEmpty ? [1] : stemNos
    }

    var nextUnplacedPinNo: Int? {
        for no in requiredPinNos {
            if !pinClicks.contains(where: { Int($0.no) == no }) {
                return no
            }
        }
        return requiredPinNos.last
    }

    var canSubmit: Bool {
        guard feedback == nil else { return false }
        guard case .inProgress = state else { return false }
        return buildSubmission() != nil
    }

    var isTimerWarning: Bool {
        if perQuestionLimitSecs != nil && questionRemainingSecs <= 5 { return true }
        if totalExamLimitSecs != nil && totalRemainingSecs <= 60 { return true }
        return false
    }

    func stopTimer() {
        timerTask?.cancel()
        timerTask = nil
        lastFrameDate = nil
    }

    func syncFromSession() {
        questions = (try? session.questions()) ?? []
        state = (try? session.state()) ?? .submitted
        currentQuestion = try? session.currentQuestion()
        if let groupId = activeQuestion?.questionGroupId {
            currentQuestionGroup = try? database.questionGroup(id: groupId)
        } else {
            currentQuestionGroup = nil
        }
    }

    func handleScenePhaseChange(_ phase: ScenePhase) {
        guard isTimedMode, summary == nil else { return }
        switch phase {
        case .inactive, .background:
            if case .inProgress = state, feedback == nil {
                pausedByBackground = true
                stopTimer()
                try? session.pause(now: StudyTimestamp.rfc3339(nowProvider()))
                syncFromSession()
            }
        case .active:
            // Keep paused so user sees exact preserved countdown and resumes intentionally.
            break
        @unknown default:
            break
        }
    }

    func pauseManually() {
        guard case .inProgress = state, feedback == nil else { return }
        pausedByBackground = false
        stopTimer()
        do {
            try session.pause(now: StudyTimestamp.rfc3339(nowProvider()))
            syncFromSession()
        } catch {
            actionError = error.localizedDescription
        }
    }

    func resumeSession() {
        guard case .paused = state else { return }
        pausedByBackground = false
        pausedForReview = false
        do {
            try session.resume(now: StudyTimestamp.rfc3339(nowProvider()))
            syncFromSession()
            startTimerLoopIfNeeded()
        } catch {
            actionError = error.localizedDescription
        }
    }

    func placePin(at point: NormalizedPoint) {
        guard feedback == nil, let target = nextUnplacedPinNo else { return }
        pinClicks.removeAll { Int($0.no) == target }
        pinClicks.append(PinClick(no: UInt32(target), point: point))
    }

    func clearPins() {
        pinClicks.removeAll()
    }

    func toggleOption(_ key: String) {
        guard feedback == nil, let q = activeQuestion else { return }
        if q.questionType == .singleChoice {
            selectedKeys = [key]
        } else if let idx = selectedKeys.firstIndex(of: key) {
            selectedKeys.remove(at: idx)
        } else {
            selectedKeys.append(key)
        }
    }

    func toggleErrorTag(_ tag: String) {
        if let idx = errorTags.firstIndex(of: tag) {
            errorTags.remove(at: idx)
        } else {
            errorTags.append(tag)
        }
        if feedback != nil && !history.isEmpty {
            let lastIdx = history.count - 1
            if let existing = history[lastIdx].errorTags.firstIndex(of: tag) {
                history[lastIdx].errorTags.remove(at: existing)
            } else {
                history[lastIdx].errorTags.append(tag)
            }
        }
    }

    func buildSubmission() -> AnswerSubmission? {
        guard let q = activeQuestion else { return nil }
        if q.gradingMode == .voided {
            if !q.options.isEmpty {
                let chosen = selectedKeys.isEmpty ? [q.options[0].key] : selectedKeys
                return .keys(values: chosen)
            }
            let trimmed = textAnswer.trimmingCharacters(in: .whitespacesAndNewlines)
            return .text(value: trimmed.isEmpty ? "-" : trimmed)
        }

        switch q.questionType {
        case .singleChoice, .multipleChoice:
            return selectedKeys.isEmpty ? nil : .keys(values: selectedKeys)
        case .pinLocate:
            return pinClicks.isEmpty ? nil : .pinClicks(clicks: pinClicks)
        case .shortAnswer, .vocab, .imageQa, .imageOcclusion, .pinIdentify:
            let trimmed = textAnswer.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : .text(value: trimmed)
        }
    }

    func submitAnswer() {
        guard case .inProgress(let idx) = state, let q = currentQuestion else { return }
        guard let submission = buildSubmission() else { return }

        actionError = nil
        let nowStr = StudyTimestamp.rfc3339(nowProvider())
        do {
            let answer = QuizAnswer(
                submission: submission,
                confidence: confidence,
                errorTags: errorTags
            )
            let grading = try session.submitAnswer(answer: answer, now: nowStr)
            let nextState = try session.state()
            history.append(
                QuizReviewEntry(
                    index: Int(idx),
                    question: q,
                    submission: submission,
                    confidence: confidence,
                    errorTags: errorTags,
                    grading: grading
                )
            )

            if let grading {
                if isPractical, case .inProgress = nextState {
                    pausedForReview = true
                    stopTimer()
                    try? session.pause(now: nowStr)
                }
                syncFromSession()
                feedback = ImmediateFeedback(
                    question: q,
                    submission: submission,
                    grading: grading,
                    nextState: nextState,
                    timedOut: false
                )
            } else {
                resetDraft()
                deferredNoticeVisible = true
                syncFromSession()
                Task { @MainActor in
                    try? await Task.sleep(for: .milliseconds(1200))
                    deferredNoticeVisible = false
                }
            }
        } catch {
            actionError = error.localizedDescription
        }
    }

    func nextAfterFeedback() {
        guard let currentFeedback = feedback else { return }
        let nextState = currentFeedback.nextState
        feedback = nil
        resetDraft()
        if let limit = perQuestionLimitSecs {
            questionRemainingSecs = Double(limit)
        }

        if case .submitted = nextState {
            pausedForReview = false
            finishAndCommit()
            return
        }

        if pausedForReview {
            pausedForReview = false
            try? session.resume(now: StudyTimestamp.rfc3339(nowProvider()))
        }
        syncFromSession()
        startTimerLoopIfNeeded()
    }

    func finishAndCommit() {
        stopTimer()
        actionError = nil
        let nowStr = StudyTimestamp.rfc3339(nowProvider())
        do {
            let graded = try session.finish(now: nowStr)
            let written = try database.commitSession(session: session, userId: userId, now: nowStr)
            let score = try session.score()

            // Merge deferred grading outcomes into review history
            var updatedHistory = history
            for i in updatedHistory.indices {
                if updatedHistory[i].grading == nil,
                   let match = graded.first(where: { $0.questionId == updatedHistory[i].question.id }) {
                    updatedHistory[i].grading = GradingOutcome(
                        isCorrect: match.isCorrect,
                        scoreable: match.scoreable,
                        voided: match.voided,
                        affectsScheduling: true,
                        note: match.note
                    )
                }
            }
            history = updatedHistory
            feedback = nil
            syncFromSession()
            summary = CompletedQuizSummary(score: score, attemptsWritten: written, entries: updatedHistory)
        } catch {
            actionError = error.localizedDescription
        }
    }

    func tickOnce(at date: Date? = nil) {
        guard isTimedMode, case .inProgress = state, feedback == nil, summary == nil else {
            return
        }
        let now = date ?? nowProvider()
        let dt: Double
        if let last = lastFrameDate {
            dt = max(0, now.timeIntervalSince(last))
        } else {
            dt = 0
        }
        lastFrameDate = now

        if perQuestionLimitSecs != nil {
            questionRemainingSecs = max(0, questionRemainingSecs - dt)
        }
        if totalExamLimitSecs != nil {
            totalRemainingSecs = max(0, totalRemainingSecs - dt)
        }

        let nowStr = StudyTimestamp.rfc3339(now)
        let timedOutQuestion = currentQuestion
        let timedOutIndex = currentIndex

        guard let outcome = try? session.tick(now: nowStr) else { return }
        switch outcome {
        case .none:
            break
        case .questionTimedOut(let graded):
            stopTimer()
            let nextState = (try? session.state()) ?? .submitted
            if let q = timedOutQuestion {
                let outcomeModel = GradingOutcome(
                    isCorrect: graded.isCorrect,
                    scoreable: graded.scoreable,
                    voided: graded.voided,
                    affectsScheduling: true,
                    note: graded.note
                )
                history.append(
                    QuizReviewEntry(
                        index: timedOutIndex,
                        question: q,
                        submission: .timedOut,
                        confidence: .guessed,
                        errorTags: ["time_pressure"],
                        grading: outcomeModel
                    )
                )
                if isPractical, case .inProgress = nextState {
                    pausedForReview = true
                    try? session.pause(now: nowStr)
                }
                syncFromSession()
                feedback = ImmediateFeedback(
                    question: q,
                    submission: .timedOut,
                    grading: outcomeModel,
                    nextState: nextState,
                    timedOut: true
                )
            }
        case .sessionExpired:
            stopTimer()
            syncFromSession()
        }
    }

    private func startTimerLoopIfNeeded() {
        stopTimer()
        guard isTimedMode, case .inProgress = state, feedback == nil, summary == nil else {
            return
        }
        lastFrameDate = nowProvider()
        timerTask = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(250))
                guard !Task.isCancelled, let self else { break }
                self.tickOnce()
            }
        }
    }

    private func resetDraft() {
        selectedKeys = []
        textAnswer = ""
        pinClicks = []
        confidence = .certain
        errorTags = []
        actionError = nil
    }

    static func formatTimer(_ seconds: Double) -> String {
        let clamped = max(0, Int(ceil(seconds)))
        let mins = clamped / 60
        let secs = clamped % 60
        return String(format: "%02d:%02d", mins, secs)
    }
}
