import SwiftUI

struct QuestionCardDetailView: View {
    let questionId: String
    let store: AppStore
    let l10n: LocalizationManager
    var onOpenCard: (String) -> Void

    @State private var question: QuestionCard?
    @State private var questionGroup: QuestionGroup?
    @State private var linkedCards: [KnowledgeCard] = []
    @State private var showAnswer: Bool = false
    @State private var activeAnchor: AnchorSheetItem?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                if let question {
                    // Metadata header
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 8) {
                            Text(questionTypeLabel(question.questionType))
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(ReinforceTheme.accent)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(ReinforceTheme.accentSoft)
                                .clipShape(Capsule())

                            Text(question.subject)
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(ReinforceTheme.inkMuted)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(ReinforceTheme.subtle)
                                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))

                            Spacer()
                        }

                        if let group = questionGroup {
                            VStack(alignment: .leading, spacing: 8) {
                                Text(l10n.t("study.runner.sharedStem"))
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(ReinforceTheme.inkMuted)
                                ForEach(Array(ContentBlock.decodeArray(from: group.sharedStemBlocksJson).enumerated()), id: \.offset) { _, block in
                                    BlockRendererView(
                                        block: block,
                                        resolveBlobURL: { store.resolveBlobURL(for: $0) },
                                        l10n: l10n,
                                        onOpenCard: onOpenCard
                                    )
                                }
                            }
                            .padding(12)
                            .background(ReinforceTheme.subtle)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }

                        ForEach(Array(ContentBlock.decodeArray(from: question.stemBlocksJson).enumerated()), id: \.offset) { _, block in
                            BlockRendererView(
                                block: block,
                                resolveBlobURL: { store.resolveBlobURL(for: $0) },
                                l10n: l10n,
                                onOpenCard: onOpenCard
                            )
                        }

                        if !question.options.isEmpty {
                            VStack(spacing: 8) {
                                ForEach(question.options, id: \.key) { option in
                                    let isAnswer = showAnswer && question.answer.contains(option.key)
                                    HStack(alignment: .top, spacing: 10) {
                                        Text(option.key)
                                            .font(.subheadline.weight(.bold))
                                            .foregroundStyle(isAnswer ? ReinforceTheme.ok : ReinforceTheme.accent)
                                            .frame(width: 24)
                                        VStack(alignment: .leading, spacing: 4) {
                                            Text(option.content)
                                                .font(.subheadline)
                                                .foregroundStyle(ReinforceTheme.ink)
                                            if showAnswer, let explanation = option.explanation, !explanation.isEmpty {
                                                Text(explanation)
                                                    .font(.caption)
                                                    .foregroundStyle(ReinforceTheme.inkMuted)
                                            }
                                        }
                                        Spacer()
                                    }
                                    .padding(10)
                                    .background(isAnswer ? ReinforceTheme.okSoft : ReinforceTheme.canvas)
                                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .stroke(isAnswer ? ReinforceTheme.ok : ReinforceTheme.border, lineWidth: 1)
                                    )
                                }
                            }
                            .padding(.top, 4)
                        }
                    }
                    .reinforceCard()

                    // Answer & explanation reveal
                    VStack(alignment: .leading, spacing: 10) {
                        Button {
                            withAnimation { showAnswer.toggle() }
                        } label: {
                            HStack {
                                Label(
                                    showAnswer ? l10n.t("question.hideAnswer") : l10n.t("question.revealAnswer"),
                                    systemImage: showAnswer ? "eye.slash" : "eye"
                                )
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(ReinforceTheme.accent)
                                Spacer()
                                Image(systemName: showAnswer ? "chevron.up" : "chevron.down")
                                    .font(.caption)
                                    .foregroundStyle(ReinforceTheme.inkMuted)
                            }
                            .frame(minHeight: ReinforceTheme.minTouchTarget)
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("question-detail-reveal-answer")

                        if showAnswer {
                            Divider().background(ReinforceTheme.border)
                            HStack(spacing: 8) {
                                Text(l10n.t("study.runner.grading.officialAnswer"))
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(ReinforceTheme.inkMuted)
                                Text(question.answer.joined(separator: ", "))
                                    .font(.subheadline.weight(.bold))
                                    .foregroundStyle(ReinforceTheme.ok)
                            }

                            let explanationBlocks = ContentBlock.decodeArray(from: question.explanationBlocksJson)
                            if !explanationBlocks.isEmpty {
                                ForEach(Array(explanationBlocks.enumerated()), id: \.offset) { _, block in
                                    BlockRendererView(
                                        block: block,
                                        resolveBlobURL: { store.resolveBlobURL(for: $0) },
                                        l10n: l10n,
                                        onOpenCard: onOpenCard
                                    )
                                }
                            }
                        }
                    }
                    .reinforceCard()

                    if !question.anchorIds.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Text(l10n.t("reading.sources"))
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(ReinforceTheme.ink)
                            ScrollView(.horizontal, showsIndicators: false) {
                                HStack(spacing: 8) {
                                    ForEach(Array(question.anchorIds.enumerated()), id: \.element) { idx, anchorId in
                                        Button {
                                            activeAnchor = AnchorSheetItem(id: anchorId)
                                        } label: {
                                            Label(
                                                "\(l10n.t("reading.sourceAnchor")) #\(idx + 1)",
                                                systemImage: "doc.text.magnifyingglass"
                                            )
                                            .font(.caption.weight(.medium))
                                            .foregroundStyle(ReinforceTheme.accent)
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
                            }
                        }
                        .reinforceCard()
                    }

                    if !linkedCards.isEmpty {
                        VStack(alignment: .leading, spacing: 10) {
                            Label(l10n.t("reading.relatedNodes"), systemImage: "square.stack.3d.up")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(ReinforceTheme.ink)
                            ForEach(linkedCards, id: \.id) { card in
                                Button {
                                    onOpenCard(card.id)
                                } label: {
                                    HStack {
                                        Text(card.title)
                                            .font(.subheadline.weight(.medium))
                                            .foregroundStyle(ReinforceTheme.ink)
                                        Spacer()
                                        Image(systemName: "chevron.right")
                                            .font(.caption)
                                            .foregroundStyle(ReinforceTheme.inkMuted)
                                    }
                                    .padding(12)
                                    .background(ReinforceTheme.canvas)
                                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                                            .stroke(ReinforceTheme.border, lineWidth: 1)
                                    )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .reinforceCard()
                    }
                } else {
                    Text(l10n.t("question.missing"))
                        .font(.subheadline)
                        .foregroundStyle(ReinforceTheme.inkMuted)
                        .reinforceCard()
                }
            }
            .padding()
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .background(ReinforceTheme.canvas)
        .navigationTitle(l10n.t("question.detailTitle"))
        .navigationBarTitleDisplayMode(.inline)
        .sheet(item: $activeAnchor) { item in
            AnchorViewerSheet(anchorId: item.id, store: store, l10n: l10n)
        }
        .onAppear { loadQuestion() }
        .onChange(of: questionId) { _, _ in loadQuestion() }
    }

    private func loadQuestion() {
        showAnswer = false
        question = store.loadQuestionCard(id: questionId)
        if let groupId = question?.questionGroupId {
            questionGroup = store.loadQuestionGroup(id: groupId)
        } else {
            questionGroup = nil
        }

        let neighbors = (try? store.database.neighbors(endpoint: .question(id: questionId), depth: 1)) ?? []
        var cardIds: [String] = []
        for neighbor in neighbors {
            if case .card(let id) = neighbor.endpoint, !cardIds.contains(id) {
                cardIds.append(id)
            }
        }
        if let options = question?.options {
            for option in options {
                for id in option.linkedCardIds where !cardIds.contains(id) {
                    cardIds.append(id)
                }
            }
        }
        linkedCards = cardIds.compactMap { store.loadKnowledgeCard(id: $0) }
    }

    private func questionTypeLabel(_ kind: QuestionType) -> String {
        switch kind {
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
