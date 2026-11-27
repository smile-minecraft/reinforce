import SwiftUI

enum StudyModeChoice: String, CaseIterable, Identifiable {
    case dailyDue = "daily_due"
    case fullExam = "full_exam"
    case topicDrill = "topic_drill"
    case practicalExam = "practical_exam"
    case weaknessDrill = "weakness_drill"

    var id: String { rawValue }

    var iconName: String {
        switch self {
        case .dailyDue: return "sparkles"
        case .fullExam: return "doc.text.fill"
        case .topicDrill: return "square.stack.3d.up.fill"
        case .practicalExam: return "timer"
        case .weaknessDrill: return "flame.fill"
        }
    }
}

struct StudyHubView: View {
    let store: AppStore
    let l10n: LocalizationManager
    let defaultMode: StudyModeChoice

    @State private var selectedMode: StudyModeChoice
    @State private var questionLimit: Int = 20
    @State private var newCardLimit: Int = 10
    @State private var selectedExamSetId: String = ""
    @State private var totalTimeMinutesText: String = ""
    @State private var selectedCardIds: Set<String> = []
    @State private var secondsPerQuestion: Double = 30
    @State private var pinRadius: Double = 0.06
    @State private var affectsScheduling: Bool = true
    @State private var errorMessage: String?

    private static let practicalPresets = [15, 30, 45, 60, 120]
    private static let pinRadii = [0.04, 0.06, 0.08]

    init(store: AppStore, l10n: LocalizationManager, defaultMode: StudyModeChoice = .dailyDue) {
        self.store = store
        self.l10n = l10n
        self.defaultMode = defaultMode
        self._selectedMode = State(initialValue: defaultMode)
    }

    private var activeExamSetId: String {
        if !selectedExamSetId.isEmpty { return selectedExamSetId }
        return store.examSets.first?.examSet.id ?? ""
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                statsStrip
                modeGrid
                configurationCard
            }
            .padding()
            .frame(maxWidth: 920, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .background(ReinforceTheme.canvas)
        .navigationTitle(defaultMode == .practicalExam ? l10n.t("nav.practical") : l10n.t("study.title"))
        .onAppear {
            if selectedExamSetId.isEmpty, let first = store.examSets.first {
                selectedExamSetId = first.examSet.id
            }
        }
    }

    private var statsStrip: some View {
        HStack(spacing: 10) {
            statTile(
                title: l10n.t("study.stats.due"),
                value: "\(store.dueQuestions.count)",
                tint: ReinforceTheme.accent,
                id: "study-stat-due"
            )
            statTile(
                title: l10n.t("study.stats.reviewed"),
                value: "\(store.scheduledCardCount)",
                tint: ReinforceTheme.ok,
                id: "study-stat-reviewed"
            )
            statTile(
                title: l10n.t("study.stats.attempts"),
                value: "\(store.totalAttemptCount)",
                tint: ReinforceTheme.ink,
                id: "study-stat-attempts"
            )
        }
    }

    private func statTile(title: String, value: String, tint: Color, id: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(value)
                .font(.title2.monospacedDigit().weight(.bold))
                .foregroundStyle(tint)
                .accessibilityIdentifier(id)
            Text(title)
                .font(.caption)
                .foregroundStyle(ReinforceTheme.inkMuted)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .reinforceCard(padding: 12)
    }

    private var modeGrid: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 10)], spacing: 10) {
            ForEach(StudyModeChoice.allCases) { mode in
                let active = selectedMode == mode
                Button {
                    selectedMode = mode
                    errorMessage = nil
                } label: {
                    VStack(alignment: .leading, spacing: 10) {
                        Image(systemName: mode.iconName)
                            .font(.title3)
                        Text(l10n.t("study.modes.\(mode.rawValue).name"))
                            .font(.subheadline.weight(.semibold))
                            .multilineTextAlignment(.leading)
                    }
                    .foregroundStyle(active ? ReinforceTheme.accent : ReinforceTheme.ink)
                    .frame(maxWidth: .infinity, minHeight: 82, alignment: .topLeading)
                    .padding(12)
                    .background(active ? ReinforceTheme.accentSoft : ReinforceTheme.surface)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .stroke(active ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: active ? 1.5 : 1)
                    )
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("study-mode-\(mode.rawValue)")
            }
        }
    }

    private var configurationCard: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 4) {
                Text(l10n.t("study.modes.\(selectedMode.rawValue).name"))
                    .font(.title3.weight(.bold))
                    .foregroundStyle(ReinforceTheme.ink)
                Text(l10n.t("study.modes.\(selectedMode.rawValue).desc"))
                    .font(.subheadline)
                    .foregroundStyle(ReinforceTheme.inkMuted)
            }

            modeSpecificControls

            Divider().background(ReinforceTheme.border)

            Toggle(l10n.t("study.options.affectsScheduling"), isOn: $affectsScheduling)
                .tint(ReinforceTheme.accent)
                .font(.subheadline)
                .foregroundStyle(ReinforceTheme.ink)

            if let errorMessage {
                Text(errorMessage)
                    .font(.subheadline)
                    .foregroundStyle(ReinforceTheme.warn)
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(ReinforceTheme.warnSoft)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .accessibilityIdentifier("study-launch-error")
            }

            Button {
                startSelectedMode()
            } label: {
                Label(l10n.t("study.options.startSession"), systemImage: "play.fill")
                    .font(.headline)
                    .foregroundStyle(.white)
                    .frame(maxWidth: .infinity)
                    .frame(minHeight: 50)
                    .background(ReinforceTheme.accent)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("study-start-session-button")
        }
        .reinforceCard()
    }

    @ViewBuilder
    private var modeSpecificControls: some View {
        switch selectedMode {
        case .dailyDue:
            VStack(spacing: 12) {
                Stepper(
                    "\(l10n.t("study.options.questionLimit")): \(questionLimit)",
                    value: $questionLimit,
                    in: 1...200
                )
                Stepper(
                    "\(l10n.t("study.options.newCardLimit")): \(newCardLimit)",
                    value: $newCardLimit,
                    in: 0...100
                )
            }
            .font(.subheadline)

        case .fullExam:
            VStack(alignment: .leading, spacing: 12) {
                Text(l10n.t("study.runner.deferredBanner"))
                    .font(.caption)
                    .foregroundStyle(ReinforceTheme.accent)
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(ReinforceTheme.accentSoft)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))

                if store.examSets.isEmpty {
                    Text(l10n.t("study.options.noExamSets"))
                        .font(.subheadline)
                        .foregroundStyle(ReinforceTheme.inkMuted)
                } else {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(l10n.t("study.options.selectExamSet"))
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.inkMuted)
                        Picker(l10n.t("study.options.selectExamSet"), selection: $selectedExamSetId) {
                            ForEach(store.examSets, id: \.examSet.id) { summary in
                                Text("\(summary.examSet.title) (\(summary.publishedQuestionCount)Q)")
                                    .tag(summary.examSet.id)
                            }
                        }
                        .pickerStyle(.menu)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(10)
                        .background(ReinforceTheme.canvas)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .stroke(ReinforceTheme.border, lineWidth: 1)
                        )
                    }

                    VStack(alignment: .leading, spacing: 6) {
                        Text(l10n.t("study.options.totalTimeMins"))
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.inkMuted)
                        TextField(l10n.t("study.options.noTimeLimit"), text: $totalTimeMinutesText)
                            .keyboardType(.numberPad)
                            .padding(10)
                            .background(ReinforceTheme.canvas)
                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .stroke(ReinforceTheme.border, lineWidth: 1)
                            )
                    }
                }
            }

        case .topicDrill:
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    Text(l10n.t("study.options.selectCards"))
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(ReinforceTheme.ink)
                    Spacer()
                    Text(l10n.t("study.options.selectedCardsCount", selectedCardIds.count))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(ReinforceTheme.accent)
                }

                if store.knowledgeCards.isEmpty {
                    Text(l10n.t("reading.noCards"))
                        .font(.subheadline)
                        .foregroundStyle(ReinforceTheme.inkMuted)
                } else {
                    VStack(spacing: 6) {
                        ForEach(store.knowledgeCards, id: \.id) { card in
                            let picked = selectedCardIds.contains(card.id)
                            Button {
                                if picked {
                                    selectedCardIds.remove(card.id)
                                } else {
                                    selectedCardIds.insert(card.id)
                                }
                            } label: {
                                HStack(spacing: 10) {
                                    Image(systemName: picked ? "checkmark.square.fill" : "square")
                                        .foregroundStyle(picked ? ReinforceTheme.accent : ReinforceTheme.inkMuted)
                                    Text(card.title)
                                        .font(.subheadline.weight(.medium))
                                        .foregroundStyle(picked ? ReinforceTheme.accent : ReinforceTheme.ink)
                                    Spacer()
                                }
                                .padding(10)
                                .background(picked ? ReinforceTheme.accentSoft : ReinforceTheme.canvas)
                                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                                        .stroke(picked ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                                )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }

        case .practicalExam:
            VStack(alignment: .leading, spacing: 14) {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text(l10n.t("study.options.secondsPerQuestion"))
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(ReinforceTheme.ink)
                        Spacer()
                        Text("\(Int(secondsPerQuestion))s")
                            .font(.headline.monospacedDigit())
                            .foregroundStyle(ReinforceTheme.accent)
                            .accessibilityIdentifier("practical-seconds-value")
                    }

                    Slider(value: $secondsPerQuestion, in: 1...300, step: 1)
                        .tint(ReinforceTheme.accent)

                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(Self.practicalPresets, id: \.self) { preset in
                                let active = Int(secondsPerQuestion) == preset
                                Button {
                                    secondsPerQuestion = Double(preset)
                                } label: {
                                    Text("\(preset)s")
                                        .font(.caption.monospacedDigit().weight(.semibold))
                                        .foregroundStyle(active ? ReinforceTheme.accent : ReinforceTheme.inkMuted)
                                        .padding(.horizontal, 12)
                                        .frame(minHeight: 36)
                                        .background(active ? ReinforceTheme.accentSoft : ReinforceTheme.canvas)
                                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                                        .overlay(
                                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                                .stroke(active ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                                        )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text(l10n.t("study.options.pinRadius"))
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(ReinforceTheme.ink)
                    HStack(spacing: 8) {
                        ForEach(Self.pinRadii, id: \.self) { radius in
                            let active = abs(pinRadius - radius) < 0.001
                            Button {
                                pinRadius = radius
                            } label: {
                                Text("±\(Int(round(radius * 100)))%")
                                    .font(.caption.monospacedDigit().weight(.semibold))
                                    .foregroundStyle(active ? ReinforceTheme.accent : ReinforceTheme.inkMuted)
                                    .padding(.horizontal, 12)
                                    .frame(minHeight: 36)
                                    .background(active ? ReinforceTheme.accentSoft : ReinforceTheme.canvas)
                                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .stroke(active ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                                    )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }

        case .weaknessDrill:
            VStack(alignment: .leading, spacing: 10) {
                Text(l10n.t("study.weaknessPreview.heading"))
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(ReinforceTheme.ink)

                if let report = store.weaknessReport,
                   (!report.questions.isEmpty || !report.knowledgeCards.isEmpty) {
                    VStack(spacing: 8) {
                        ForEach(report.questions.prefix(5), id: \.questionId) { entry in
                            HStack {
                                Text(entry.questionId.suffix(8))
                                    .font(.caption.monospaced().weight(.semibold))
                                    .foregroundStyle(ReinforceTheme.ink)
                                Spacer()
                                Text(l10n.t("study.weaknessPreview.errorRate", Int(round(entry.errorRate * 100))))
                                    .font(.caption2.monospacedDigit())
                                    .foregroundStyle(ReinforceTheme.bad)
                                Text(l10n.t("study.weaknessPreview.avgDuration", entry.averageDurationMs / 1000.0))
                                    .font(.caption2.monospacedDigit())
                                    .foregroundStyle(ReinforceTheme.inkMuted)
                            }
                            .padding(10)
                            .background(ReinforceTheme.canvas)
                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        }
                    }
                } else {
                    Text(l10n.t("study.weaknessPreview.empty"))
                        .font(.subheadline)
                        .foregroundStyle(ReinforceTheme.inkMuted)
                        .padding(12)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(ReinforceTheme.subtle)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
            }
        }
    }

    private func startSelectedMode() {
        errorMessage = nil
        do {
            let launched: Bool
            switch selectedMode {
            case .dailyDue:
                launched = try store.startDailyDue(
                    questionLimit: UInt32(questionLimit),
                    newCardLimit: UInt32(newCardLimit),
                    affectsScheduling: affectsScheduling
                )
            case .fullExam:
                guard !activeExamSetId.isEmpty else {
                    errorMessage = l10n.t("study.options.noExamSets")
                    return
                }
                let mins = Int(totalTimeMinutesText.trimmingCharacters(in: .whitespaces))
                let totalSecs = (mins != nil && mins! > 0) ? UInt32(mins! * 60) : nil
                launched = try store.startFullExam(
                    examSetId: activeExamSetId,
                    totalTimeSecs: totalSecs,
                    affectsScheduling: affectsScheduling
                )
            case .topicDrill:
                guard !selectedCardIds.isEmpty else {
                    errorMessage = l10n.t("study.options.selectAtLeastOneCard")
                    return
                }
                launched = try store.startTopicDrill(
                    cardIds: Array(selectedCardIds),
                    affectsScheduling: affectsScheduling
                )
            case .practicalExam:
                launched = try store.startPracticalExam(
                    secondsPerQuestion: UInt32(Int(secondsPerQuestion)),
                    pinRadius: pinRadius,
                    affectsScheduling: affectsScheduling
                )
            case .weaknessDrill:
                launched = try store.startWeaknessDrill(
                    questionLimit: UInt32(questionLimit),
                    affectsScheduling: affectsScheduling
                )
            }
            if !launched {
                errorMessage = l10n.t("study.options.emptySessionError")
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
