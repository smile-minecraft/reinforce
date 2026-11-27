import SwiftUI

struct QuizSummaryView: View {
    let summary: CompletedQuizSummary
    let store: AppStore
    let l10n: LocalizationManager
    let onClose: () -> Void
    let onOpenAnchor: (String) -> Void

    private var scorePercentage: Int {
        Int((summary.score.ratio * 100.0).rounded())
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                VStack(alignment: .leading, spacing: 14) {
                    Text(l10n.t("study.runner.summary.title"))
                        .font(.title2.weight(.bold))
                        .foregroundStyle(ReinforceTheme.ink)
                        .accessibilityIdentifier("quiz-summary-title")

                    HStack(alignment: .firstTextBaseline, spacing: 18) {
                        Text("\(scorePercentage)%")
                            .font(.system(size: 44, weight: .bold, design: .monospaced))
                            .foregroundStyle(ReinforceTheme.accent)
                            .accessibilityIdentifier("quiz-summary-score")

                        VStack(alignment: .leading, spacing: 4) {
                            Text(
                                l10n.t(
                                    "study.runner.summary.correctOf",
                                    Int(summary.score.correct),
                                    Int(summary.score.scoreable)
                                )
                            )
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.ink)

                            if summary.score.voided > 0 {
                                Text(l10n.t("study.runner.summary.voidedCount", Int(summary.score.voided)))
                                    .font(.caption)
                                    .foregroundStyle(ReinforceTheme.inkMuted)
                            }

                            Text(l10n.t("study.runner.summary.attemptsSaved", Int(summary.attemptsWritten)))
                                .font(.caption)
                                .foregroundStyle(ReinforceTheme.ok)
                                .accessibilityIdentifier("quiz-summary-attempts-saved")
                        }
                    }

                    Button {
                        onClose()
                    } label: {
                        Text(l10n.t("study.runner.summary.backToStudy"))
                            .font(.headline)
                            .foregroundStyle(.white)
                            .frame(maxWidth: .infinity)
                            .frame(minHeight: 48)
                            .background(ReinforceTheme.accent)
                            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("quiz-summary-done-button")
                }
                .reinforceCard()

                if !summary.entries.isEmpty {
                    VStack(alignment: .leading, spacing: 10) {
                        Text(l10n.t("study.runner.summary.reviewHeading"))
                            .font(.headline)
                            .foregroundStyle(ReinforceTheme.ink)

                        ForEach(summary.entries) { entry in
                            SummaryReviewRow(
                                entry: entry,
                                store: store,
                                l10n: l10n,
                                onOpenAnchor: onOpenAnchor
                            )
                        }
                    }
                }
            }
            .padding()
            .frame(maxWidth: 800, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .background(ReinforceTheme.canvas)
    }
}

private struct SummaryReviewRow: View {
    let entry: QuizReviewEntry
    let store: AppStore
    let l10n: LocalizationManager
    let onOpenAnchor: (String) -> Void

    @State private var expanded = false

    private var stemBlocks: [ContentBlock] {
        ContentBlock.decodeArray(from: entry.question.stemBlocksJson)
    }

    private var explanationBlocks: [ContentBlock] {
        ContentBlock.decodeArray(from: entry.question.explanationBlocksJson)
    }

    private var isVoided: Bool {
        entry.grading?.voided == true || entry.question.gradingMode == .voided
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                withAnimation(.easeInOut(duration: 0.15)) {
                    expanded.toggle()
                }
            } label: {
                HStack(spacing: 10) {
                    Text("#\(entry.index + 1)")
                        .font(.caption.monospacedDigit().weight(.bold))
                        .foregroundStyle(ReinforceTheme.inkMuted)

                    Image(systemName: isVoided ? "info.circle.fill" : (entry.grading?.isCorrect == true ? "checkmark.circle.fill" : "xmark.circle.fill"))
                        .foregroundStyle(isVoided ? ReinforceTheme.inkMuted : (entry.grading?.isCorrect == true ? ReinforceTheme.ok : ReinforceTheme.bad))

                    VStack(alignment: .leading, spacing: 2) {
                        Text(stemBlocks.map(\.plainSummary).joined(separator: " "))
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(ReinforceTheme.ink)
                            .lineLimit(1)
                        Text("\(l10n.t("study.runner.summary.yourAnswer")): \(formatSubmission(entry.submission))")
                            .font(.caption)
                            .foregroundStyle(ReinforceTheme.inkMuted)
                    }

                    Spacer()

                    Image(systemName: expanded ? "chevron.up" : "chevron.down")
                        .font(.caption)
                        .foregroundStyle(ReinforceTheme.inkMuted)
                }
                .padding(14)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if expanded {
                Divider().background(ReinforceTheme.border)
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(Array(stemBlocks.enumerated()), id: \.offset) { _, block in
                        BlockRendererView(
                            block: block,
                            resolveBlobURL: { store.resolveBlobURL(for: $0) },
                            l10n: l10n
                        )
                    }

                    if !entry.question.answer.isEmpty {
                        Text("\(l10n.t("study.runner.grading.officialAnswer")): \(entry.question.answer.joined(separator: ", "))")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.ok)
                            .padding(10)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(ReinforceTheme.okSoft)
                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    }

                    if let basis = entry.question.answerBasis, !basis.isEmpty {
                        Text("\(l10n.t("study.runner.grading.answerBasis")): \(basis)")
                            .font(.caption)
                            .foregroundStyle(ReinforceTheme.inkMuted)
                    }

                    if !explanationBlocks.isEmpty {
                        VStack(alignment: .leading, spacing: 6) {
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
                    }

                    if !entry.question.anchorIds.isEmpty {
                        HStack(spacing: 8) {
                            ForEach(Array(entry.question.anchorIds.enumerated()), id: \.element) { idx, anchorId in
                                Button {
                                    onOpenAnchor(anchorId)
                                } label: {
                                    Label("\(l10n.t("reading.sourceAnchor")) #\(idx + 1)", systemImage: "doc.text.magnifyingglass")
                                        .font(.caption.weight(.medium))
                                        .foregroundStyle(ReinforceTheme.accent)
                                        .padding(.horizontal, 10)
                                        .padding(.vertical, 6)
                                        .background(ReinforceTheme.accentSoft)
                                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
                .padding(14)
            }
        }
        .reinforceCard(padding: 0)
    }

    private func formatSubmission(_ submission: AnswerSubmission) -> String {
        switch submission {
        case .keys(let values):
            return values.joined(separator: ", ")
        case .text(let value):
            return value
        case .pinClicks(let clicks):
            return clicks.map { String(format: "#%d (%.2f, %.2f)", $0.no, $0.point.x, $0.point.y) }.joined(separator: ", ")
        case .timedOut:
            return "—"
        }
    }
}
