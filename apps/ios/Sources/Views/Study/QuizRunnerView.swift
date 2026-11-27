import SwiftUI

struct QuizRunnerView: View {
    @State var viewModel: QuizSessionViewModel
    let store: AppStore
    let l10n: LocalizationManager
    let onClose: () -> Void

    @Environment(\.scenePhase) private var scenePhase
    @State private var activeAnchor: AnchorSheetItem?

    private static let errorTagKeys = [
        "concept_gap",
        "misread",
        "confused_term",
        "careless",
        "time_pressure"
    ]

    var body: some View {
        VStack(spacing: 0) {
            if let summary = viewModel.summary {
                QuizSummaryView(
                    summary: summary,
                    store: store,
                    l10n: l10n,
                    onClose: onClose,
                    onOpenAnchor: { activeAnchor = AnchorSheetItem(id: $0) }
                )
            } else {
                runnerHeader
                runnerBody
            }
        }
        .background(ReinforceTheme.canvas)
        .sheet(item: $activeAnchor) { item in
            AnchorViewerSheet(anchorId: item.id, store: store, l10n: l10n)
        }
        .onChange(of: scenePhase) { _, newPhase in
            viewModel.handleScenePhaseChange(newPhase)
        }
        .onChange(of: viewModel.summary != nil) { _, hasSummary in
            if hasSummary {
                store.syncManager.notifyLocalStudyChange()
            }
        }
        .onDisappear {
            viewModel.stopTimer()
        }
    }

    private var runnerHeader: some View {
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                Button {
                    viewModel.stopTimer()
                    onClose()
                } label: {
                    Image(systemName: "xmark")
                        .font(.body.weight(.semibold))
                        .foregroundStyle(ReinforceTheme.inkMuted)
                        .frame(width: 36, height: ReinforceTheme.minTouchTarget)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(l10n.t("study.runner.exit"))
                .accessibilityIdentifier("quiz-exit-button")

                VStack(alignment: .leading, spacing: 2) {
                    Text(modeTitle(viewModel.mode))
                        .font(.caption.weight(.medium))
                        .foregroundStyle(ReinforceTheme.inkMuted)
                        .lineLimit(1)
                    Text(
                        l10n.t(
                            "study.runner.progress",
                            min(viewModel.displayIndex + 1, max(1, viewModel.questions.count)),
                            viewModel.questions.count
                        )
                    )
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(ReinforceTheme.ink)
                    .lineLimit(1)
                    .accessibilityIdentifier("quiz-progress-label")
                }
                .layoutPriority(0)

                Spacer(minLength: 4)

                if viewModel.perQuestionLimitSecs != nil {
                    timerBadge(seconds: viewModel.questionRemainingSecs)
                } else if viewModel.totalExamLimitSecs != nil {
                    timerBadge(seconds: viewModel.totalRemainingSecs)
                }

                if viewModel.feedback == nil && (!viewModel.isExpired && !viewModel.isSubmitted) {
                    Button {
                        if viewModel.isPausedByUserOrBackground {
                            viewModel.resumeSession()
                        } else {
                            viewModel.pauseManually()
                        }
                    } label: {
                        Image(systemName: viewModel.isPausedByUserOrBackground ? "play.fill" : "pause.fill")
                            .font(.subheadline)
                            .foregroundStyle(ReinforceTheme.ink)
                            .frame(width: 36, height: 36)
                            .background(ReinforceTheme.canvas)
                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .stroke(ReinforceTheme.border, lineWidth: 1)
                            )
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(viewModel.isPausedByUserOrBackground ? l10n.t("study.runner.resume") : l10n.t("study.runner.pause"))
                    .accessibilityIdentifier("quiz-pause-resume-button")
                }

                Button {
                    viewModel.finishAndCommit()
                } label: {
                    Label(l10n.t("study.runner.finish"), systemImage: "flag.checkered")
                        .font(.caption.weight(.semibold))
                        .lineLimit(1)
                        .fixedSize(horizontal: true, vertical: false)
                        .foregroundStyle(ReinforceTheme.accent)
                        .padding(.horizontal, 10)
                        .frame(height: 36)
                        .background(ReinforceTheme.accentSoft)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("quiz-finish-button")
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(ReinforceTheme.surface)

            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    ReinforceTheme.subtle
                    ReinforceTheme.accent
                        .frame(width: geo.size.width * viewModel.progressFraction)
                }
            }
            .frame(height: 3)
        }
    }

    private func timerBadge(seconds: Double) -> some View {
        let warn = viewModel.isTimerWarning
        return HStack(spacing: 4) {
            Image(systemName: "clock")
                .font(.caption.weight(.semibold))
            Text(QuizSessionViewModel.formatTimer(seconds))
                .font(.subheadline.monospacedDigit().weight(.bold))
                .lineLimit(1)
        }
        .fixedSize(horizontal: true, vertical: false)
        .foregroundStyle(warn ? ReinforceTheme.bad : ReinforceTheme.ink)
        .padding(.horizontal, 8)
        .frame(height: 36)
        .background(warn ? ReinforceTheme.badSoft : ReinforceTheme.subtle)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(warn ? ReinforceTheme.bad : ReinforceTheme.border, lineWidth: 1)
        )
        .accessibilityIdentifier("quiz-timer-badge")
    }

    private var runnerBody: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if viewModel.isFullExam && !viewModel.isSubmitted {
                    Text(l10n.t("study.runner.deferredBanner"))
                        .font(.caption.weight(.medium))
                        .foregroundStyle(ReinforceTheme.accent)
                        .padding(10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(ReinforceTheme.accentSoft)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }

                if viewModel.deferredNoticeVisible {
                    Text(l10n.t("study.runner.recorded"))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(ReinforceTheme.ok)
                        .padding(10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(ReinforceTheme.okSoft)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }

                if let err = viewModel.actionError {
                    Text(err)
                        .font(.subheadline)
                        .foregroundStyle(ReinforceTheme.bad)
                        .padding(12)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(ReinforceTheme.badSoft)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }

                if viewModel.isPausedByUserOrBackground {
                    pausedCard
                } else if viewModel.isExpired {
                    expiredCard
                } else if viewModel.isSubmitted && viewModel.feedback == nil {
                    submittedReadyCard
                } else if let question = viewModel.activeQuestion {
                    activeQuestionStage(question)
                }
            }
            .padding()
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }

    private var pausedCard: some View {
        VStack(spacing: 14) {
            Image(systemName: "pause.circle.fill")
                .font(.system(size: 44))
                .foregroundStyle(viewModel.pausedByBackground ? ReinforceTheme.warn : ReinforceTheme.accent)
            Text(l10n.t("study.runner.pausedTitle"))
                .font(.title3.weight(.bold))
                .foregroundStyle(ReinforceTheme.ink)
            Text(
                viewModel.pausedByBackground
                    ? l10n.t("study.runner.pausedBackground")
                    : l10n.t("study.runner.pausedManual")
            )
            .font(.subheadline)
            .foregroundStyle(ReinforceTheme.inkMuted)
            .multilineTextAlignment(.center)
            .accessibilityIdentifier(viewModel.pausedByBackground ? "quiz-paused-background-notice" : "quiz-paused-manual-notice")

            Button {
                viewModel.resumeSession()
            } label: {
                Label(l10n.t("study.runner.resume"), systemImage: "play.fill")
                    .font(.headline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 24)
                    .frame(minHeight: 48)
                    .background(ReinforceTheme.accent)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("quiz-resume-overlay-button")
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 32)
        .reinforceCard()
    }

    private var expiredCard: some View {
        VStack(spacing: 14) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 44))
                .foregroundStyle(ReinforceTheme.warn)
            Text(l10n.t("study.runner.expiredTitle"))
                .font(.title3.weight(.bold))
                .foregroundStyle(ReinforceTheme.ink)
            Text(l10n.t("study.runner.expiredDesc"))
                .font(.subheadline)
                .foregroundStyle(ReinforceTheme.inkMuted)
                .multilineTextAlignment(.center)
            Button {
                viewModel.finishAndCommit()
            } label: {
                Label(l10n.t("study.runner.finish"), systemImage: "flag.checkered")
                    .font(.headline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 24)
                    .frame(minHeight: 48)
                    .background(ReinforceTheme.accent)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            .buttonStyle(.plain)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 32)
        .reinforceCard()
    }

    private var submittedReadyCard: some View {
        VStack(spacing: 14) {
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 44))
                .foregroundStyle(ReinforceTheme.ok)
            Text(l10n.t("study.runner.summary.title"))
                .font(.title3.weight(.bold))
                .foregroundStyle(ReinforceTheme.ink)
            Button {
                viewModel.finishAndCommit()
            } label: {
                Label(l10n.t("study.runner.finish"), systemImage: "flag.checkered")
                    .font(.headline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 24)
                    .frame(minHeight: 48)
                    .background(ReinforceTheme.accent)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("quiz-submitted-finish-button")
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 32)
        .reinforceCard()
    }

    @ViewBuilder
    private func activeQuestionStage(_ question: QuestionCard) -> some View {
        let stemBlocks = ContentBlock.decodeArray(from: question.stemBlocksJson)
        let showPracticalCanvas = viewModel.practicalMedia != nil && (
            question.questionType == .pinLocate ||
            question.questionType == .pinIdentify ||
            question.questionType == .imageQa
        )
        let nonCanvasBlocks = stemBlocks.filter { block in
            guard showPracticalCanvas else { return true }
            switch block {
            case .interactivePins, .imageOcclusion:
                return false
            default:
                return true
            }
        }

        // Shared QuestionGroup vignette
        if let group = viewModel.currentQuestionGroup {
            let groupBlocks = ContentBlock.decodeArray(from: group.sharedStemBlocksJson)
            if !groupBlocks.isEmpty {
                VStack(alignment: .leading, spacing: 10) {
                    Text(group.title ?? l10n.t("study.runner.sharedStem"))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(ReinforceTheme.accent)
                    ForEach(Array(groupBlocks.enumerated()), id: \.offset) { _, block in
                        BlockRendererView(
                            block: block,
                            resolveBlobURL: { store.resolveBlobURL(for: $0) },
                            l10n: l10n,
                            hidePinTerms: viewModel.feedback == nil
                        )
                    }
                }
                .reinforceCard()
            }
        }

        // Zoomable Practical Canvas
        if showPracticalCanvas, let media = viewModel.practicalMedia {
            PracticalCanvasView(
                baseURL: store.resolveBlobURL(for: media.resourceId),
                overlayURL: media.overlayResourceId.flatMap { store.resolveBlobURL(for: $0) },
                pins: media.pins,
                mode: question.questionType == .pinLocate ? .locate : (question.questionType == .pinIdentify ? .identify : .view),
                placedClicks: viewModel.pinClicks,
                activePinNo: viewModel.nextUnplacedPinNo,
                revealPins: viewModel.feedback != nil,
                l10n: l10n,
                onPlacePin: { viewModel.placePin(at: $0) }
            )
        }

        // Stem card
        VStack(alignment: .leading, spacing: 12) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    badge(questionTypeLabel(question.questionType), fg: ReinforceTheme.inkMuted, bg: ReinforceTheme.subtle)
                    badge(question.subject, fg: ReinforceTheme.inkMuted, bg: ReinforceTheme.subtle)
                    if question.gradingMode == .anyAccepted {
                        badge(l10n.t("study.runner.grading.anyAccepted"), fg: ReinforceTheme.warn, bg: ReinforceTheme.warnSoft)
                    }
                    if question.gradingMode == .voided {
                        badge(l10n.t("study.runner.grading.voided"), fg: ReinforceTheme.inkMuted, bg: ReinforceTheme.subtle)
                    }
                }
            }

            ForEach(Array(nonCanvasBlocks.enumerated()), id: \.offset) { _, block in
                BlockRendererView(
                    block: block,
                    resolveBlobURL: { store.resolveBlobURL(for: $0) },
                    l10n: l10n,
                    hidePinTerms: viewModel.feedback == nil
                )
            }
        }
        .reinforceCard()

        // Choice options
        if !question.options.isEmpty && (question.questionType == .singleChoice || question.questionType == .multipleChoice) {
            optionsSection(question)
        }

        // Pin locate prompt bar
        if question.questionType == .pinLocate && viewModel.feedback == nil {
            HStack {
                Text(
                    viewModel.pinClicks.count >= viewModel.requiredPinNos.count
                        ? l10n.t("study.runner.pinLocateAllPlaced")
                        : l10n.t("study.runner.pinLocatePrompt", viewModel.nextUnplacedPinNo ?? 1)
                )
                .font(.subheadline)
                .foregroundStyle(ReinforceTheme.ink)
                Spacer()
                if !viewModel.pinClicks.isEmpty {
                    Button {
                        viewModel.clearPins()
                    } label: {
                        Label(l10n.t("study.runner.pinClear"), systemImage: "arrow.counterclockwise")
                            .font(.caption.weight(.medium))
                            .foregroundStyle(ReinforceTheme.ink)
                            .padding(.horizontal, 10)
                            .frame(minHeight: 36)
                            .background(ReinforceTheme.canvas)
                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .stroke(ReinforceTheme.border, lineWidth: 1)
                            )
                    }
                    .buttonStyle(.plain)
                }
            }
            .reinforceCard()
        }

        // Free-text input
        if question.questionType == .shortAnswer
            || question.questionType == .vocab
            || question.questionType == .imageQa
            || question.questionType == .imageOcclusion
            || question.questionType == .pinIdentify {
            VStack(alignment: .leading, spacing: 8) {
                Text(
                    question.questionType == .pinIdentify
                        ? l10n.t("study.runner.pinIdentifyPrompt")
                        : l10n.t("study.runner.typeAnswer")
                )
                .font(.caption.weight(.medium))
                .foregroundStyle(ReinforceTheme.inkMuted)

                TextField(l10n.t("study.runner.typeAnswer"), text: $viewModel.textAnswer)
                    .textFieldStyle(.plain)
                    .disabled(viewModel.feedback != nil)
                    .padding(12)
                    .frame(minHeight: 46)
                    .background(ReinforceTheme.canvas)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .stroke(ReinforceTheme.border, lineWidth: 1)
                    )
                    .submitLabel(.done)
                    .onSubmit {
                        if viewModel.canSubmit {
                            viewModel.submitAnswer()
                        }
                    }
                    .accessibilityIdentifier("quiz-text-answer-field")
            }
            .reinforceCard()
        }

        // Immediate grading feedback
        if let fb = viewModel.feedback {
            feedbackCard(fb)
        }

        // Confidence & Error tags
        confidenceAndTagsCard

        // Primary CTA button
        if let fb = viewModel.feedback {
            Button {
                viewModel.nextAfterFeedback()
            } label: {
                let isLast: Bool = {
                    if case .submitted = fb.nextState { return true }
                    return false
                }()
                Label(
                    isLast ? l10n.t("study.runner.viewSummary") : l10n.t("study.runner.nextQuestion"),
                    systemImage: "arrow.right"
                )
                .font(.headline)
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity)
                .frame(minHeight: 50)
                .background(ReinforceTheme.accent)
                .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("quiz-next-button")
        } else {
            Button {
                viewModel.submitAnswer()
            } label: {
                Text(l10n.t("study.runner.submitAnswer"))
                    .font(.headline)
                    .foregroundStyle(.white)
                    .frame(maxWidth: .infinity)
                    .frame(minHeight: 50)
                    .background(viewModel.canSubmit ? ReinforceTheme.accent : ReinforceTheme.accent.opacity(0.4))
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            .buttonStyle(.plain)
            .disabled(!viewModel.canSubmit)
            .accessibilityIdentifier("quiz-submit-button")
        }
    }

    private func optionsSection(_ question: QuestionCard) -> some View {
        let acceptedKeys: Set<String> = {
            var set = Set(question.answer)
            if question.gradingMode == .anyAccepted {
                for group in question.acceptedAnswers {
                    set.formUnion(group)
                }
            }
            return set
        }()

        return VStack(spacing: 10) {
            ForEach(question.options, id: \.key) { opt in
                let isSelected = viewModel.selectedKeys.contains(opt.key)
                let fb = viewModel.feedback
                let isCorrectOpt = fb != nil && acceptedKeys.contains(opt.key)
                let isWrongSelected = fb != nil && isSelected && fb?.grading.voided != true && !acceptedKeys.isEmpty && !acceptedKeys.contains(opt.key)

                let borderColor: Color = {
                    if isCorrectOpt { return ReinforceTheme.ok }
                    if isWrongSelected { return ReinforceTheme.bad }
                    if isSelected { return ReinforceTheme.accent }
                    return ReinforceTheme.border
                }()

                let bgColor: Color = {
                    if isCorrectOpt { return ReinforceTheme.okSoft.opacity(0.65) }
                    if isWrongSelected { return ReinforceTheme.badSoft.opacity(0.65) }
                    if isSelected { return ReinforceTheme.accentSoft }
                    return ReinforceTheme.surface
                }()

                VStack(alignment: .leading, spacing: 8) {
                    Button {
                        viewModel.toggleOption(opt.key)
                    } label: {
                        HStack(alignment: .top, spacing: 12) {
                            Text(opt.key)
                                .font(.caption.monospaced().weight(.bold))
                                .foregroundStyle(isSelected ? .white : ReinforceTheme.inkMuted)
                                .frame(width: 28, height: 28)
                                .background(isSelected ? ReinforceTheme.accent : ReinforceTheme.canvas)
                                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                                        .stroke(isSelected ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                                )

                            Text(opt.content)
                                .font(.body.weight(.medium))
                                .foregroundStyle(ReinforceTheme.ink)
                                .multilineTextAlignment(.leading)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .frame(minHeight: ReinforceTheme.minTouchTarget, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .disabled(fb != nil)
                    .accessibilityIdentifier("quiz-option-\(opt.key)")

                    if fb != nil, let explanation = opt.explanation, !explanation.isEmpty {
                        Text(explanation)
                            .font(.caption)
                            .foregroundStyle(ReinforceTheme.inkMuted)
                            .padding(.leading, 40)
                    }
                }
                .padding(12)
                .background(bgColor)
                .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .stroke(borderColor, lineWidth: isSelected || isCorrectOpt || isWrongSelected ? 1.5 : 1)
                )
            }
        }
    }

    private func feedbackCard(_ fb: ImmediateFeedback) -> some View {
        let isVoided = fb.grading.voided
        let isCorrect = fb.grading.isCorrect
        let fg: Color = isVoided ? ReinforceTheme.ink : (isCorrect ? ReinforceTheme.ok : ReinforceTheme.bad)
        let bg: Color = isVoided ? ReinforceTheme.subtle : (isCorrect ? ReinforceTheme.okSoft : ReinforceTheme.badSoft)
        let explanationBlocks = ContentBlock.decodeArray(from: fb.question.explanationBlocksJson)

        return VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: isVoided ? "info.circle.fill" : (isCorrect ? "checkmark.circle.fill" : "xmark.circle.fill"))
                    .foregroundStyle(fg)
                Text(
                    fb.timedOut
                        ? l10n.t("study.runner.grading.timedOut")
                        : isVoided
                            ? l10n.t("study.runner.grading.voided")
                            : isCorrect
                                ? l10n.t("study.runner.grading.correct")
                                : l10n.t("study.runner.grading.incorrect")
                )
                .font(.headline)
                .foregroundStyle(fg)
            }
            .accessibilityIdentifier("quiz-feedback-banner")

            if !fb.question.answer.isEmpty {
                Text("\(l10n.t("study.runner.grading.officialAnswer")): \(fb.question.answer.joined(separator: ", "))")
                    .font(.subheadline)
                    .foregroundStyle(ReinforceTheme.ink)
            }

            if let basis = fb.question.answerBasis, !basis.isEmpty {
                Text("\(l10n.t("study.runner.grading.answerBasis")): \(basis)")
                    .font(.caption)
                    .foregroundStyle(ReinforceTheme.inkMuted)
            }

            if !explanationBlocks.isEmpty {
                Divider().background(ReinforceTheme.border)
                Text(l10n.t("study.runner.grading.explanation"))
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(ReinforceTheme.inkMuted)
                ForEach(Array(explanationBlocks.enumerated()), id: \.offset) { _, block in
                    BlockRendererView(
                        block: block,
                        resolveBlobURL: { store.resolveBlobURL(for: $0) },
                        l10n: l10n
                    )
                }
            }

            if !fb.question.anchorIds.isEmpty {
                HStack(spacing: 8) {
                    ForEach(Array(fb.question.anchorIds.enumerated()), id: \.element) { idx, aid in
                        Button {
                            activeAnchor = AnchorSheetItem(id: aid)
                        } label: {
                            Label("\(l10n.t("reading.sourceAnchor")) #\(idx + 1)", systemImage: "doc.text.magnifyingglass")
                                .font(.caption.weight(.medium))
                                .foregroundStyle(ReinforceTheme.accent)
                                .padding(.horizontal, 10)
                                .frame(minHeight: 34)
                                .background(ReinforceTheme.surface)
                                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(bg)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(fg.opacity(0.35), lineWidth: 1)
        )
    }

    private var confidenceAndTagsCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 8) {
                Text(l10n.t("study.runner.confidence"))
                    .font(.caption.weight(.medium))
                    .foregroundStyle(ReinforceTheme.inkMuted)

                HStack(spacing: 8) {
                    confidenceButton(.certain, key: "study.runner.confidence.certain")
                    confidenceButton(.split, key: "study.runner.confidence.split")
                    confidenceButton(.guessed, key: "study.runner.confidence.guessed")
                }
            }

            VStack(alignment: .leading, spacing: 8) {
                Text(l10n.t("study.runner.errorTags"))
                    .font(.caption.weight(.medium))
                    .foregroundStyle(ReinforceTheme.inkMuted)

                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(Self.errorTagKeys, id: \.self) { tag in
                            let active = viewModel.errorTags.contains(tag)
                            Button {
                                viewModel.toggleErrorTag(tag)
                            } label: {
                                Text(l10n.t("study.runner.errorTag.\(tag)"))
                                    .font(.caption.weight(.medium))
                                    .foregroundStyle(active ? ReinforceTheme.warn : ReinforceTheme.inkMuted)
                                    .padding(.horizontal, 12)
                                    .frame(minHeight: 36)
                                    .background(active ? ReinforceTheme.warnSoft : ReinforceTheme.canvas)
                                    .clipShape(Capsule())
                                    .overlay(
                                        Capsule().stroke(active ? ReinforceTheme.warn : ReinforceTheme.border, lineWidth: 1)
                                    )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
        }
        .reinforceCard()
    }

    private func confidenceButton(_ level: Confidence, key: String) -> some View {
        let selected = viewModel.confidence == level
        return Button {
            viewModel.confidence = level
        } label: {
            Text(l10n.t(key))
                .font(.caption.weight(.semibold))
                .foregroundStyle(selected ? ReinforceTheme.accent : ReinforceTheme.inkMuted)
                .frame(maxWidth: .infinity)
                .frame(minHeight: 40)
                .background(selected ? ReinforceTheme.accentSoft : ReinforceTheme.canvas)
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .stroke(selected ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                )
        }
        .buttonStyle(.plain)
        .disabled(viewModel.feedback != nil)
    }

    private func badge(_ text: String, fg: Color, bg: Color) -> some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(fg)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(bg)
            .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
    }

    private func modeTitle(_ mode: SessionMode) -> String {
        switch mode {
        case .dailyDue: return l10n.t("study.modes.daily_due.name")
        case .fullExam: return l10n.t("study.modes.full_exam.name")
        case .topicDrill: return l10n.t("study.modes.topic_drill.name")
        case .practicalExam: return l10n.t("study.modes.practical_exam.name")
        case .weaknessDrill: return l10n.t("study.modes.weakness_drill.name")
        }
    }

    private func questionTypeLabel(_ type: QuestionType) -> String {
        switch type {
        case .singleChoice: return l10n.t("study.runner.questionType.single_choice")
        case .multipleChoice: return l10n.t("study.runner.questionType.multiple_choice")
        case .shortAnswer: return l10n.t("study.runner.questionType.short_answer")
        case .vocab: return l10n.t("study.runner.questionType.vocab")
        case .imageQa: return l10n.t("study.runner.questionType.image_qa")
        case .imageOcclusion: return l10n.t("study.runner.questionType.image_occlusion")
        case .pinIdentify: return l10n.t("study.runner.questionType.pin_identify")
        case .pinLocate: return l10n.t("study.runner.questionType.pin_locate")
        }
    }
}
