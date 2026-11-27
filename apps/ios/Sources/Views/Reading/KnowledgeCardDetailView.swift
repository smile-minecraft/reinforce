import SwiftUI

struct KnowledgeCardDetailView: View {
    let cardId: String
    let store: AppStore
    let l10n: LocalizationManager
    var onOpenCard: (String) -> Void

    @State private var card: KnowledgeCard?
    @State private var relatedCards: [KnowledgeCard] = []
    @State private var starred: Bool = false
    @State private var personalNote: String = ""
    @State private var activeAnchor: AnchorSheetItem?
    @State private var drillError: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                if let card {
                    HStack(spacing: 10) {
                        Button {
                            drillCard(card)
                        } label: {
                            Label(l10n.t("reading.drillThisCard"), systemImage: "play.fill")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.white)
                                .padding(.horizontal, 16)
                                .frame(minHeight: ReinforceTheme.minTouchTarget)
                                .background(ReinforceTheme.accent)
                                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                        .buttonStyle(.plain)

                        Button {
                            toggleStar(card)
                        } label: {
                            Label(
                                starred ? l10n.t("reading.unstarCard") : l10n.t("reading.starCard"),
                                systemImage: starred ? "star.fill" : "star"
                            )
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(starred ? ReinforceTheme.warn : ReinforceTheme.ink)
                            .padding(.horizontal, 14)
                            .frame(minHeight: ReinforceTheme.minTouchTarget)
                            .background(starred ? ReinforceTheme.warnSoft : ReinforceTheme.surface)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .stroke(starred ? ReinforceTheme.warn : ReinforceTheme.border, lineWidth: 1)
                            )
                        }
                        .buttonStyle(.plain)

                        Spacer()
                    }

                    if let drillError {
                        Text(drillError)
                            .font(.subheadline)
                            .foregroundStyle(ReinforceTheme.warn)
                            .padding(12)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(ReinforceTheme.warnSoft)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    }

                    KnowledgeCardView(
                        card: card,
                        store: store,
                        l10n: l10n,
                        showCardLink: false,
                        onOpenCard: onOpenCard,
                        onOpenAnchor: { activeAnchor = AnchorSheetItem(id: $0) }
                    )
                    .accessibilityIdentifier("card-detail-content")

                    // Personal note saved locally via userCardMeta
                    VStack(alignment: .leading, spacing: 10) {
                        HStack {
                            Label(l10n.t("reading.personalNote"), systemImage: "note.text")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(ReinforceTheme.ink)
                            Spacer()
                            Button(l10n.t("common.save")) {
                                savePersonalMeta(card)
                            }
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.accent)
                        }

                        TextField(
                            l10n.t("reading.personalNotePlaceholder"),
                            text: $personalNote,
                            axis: .vertical
                        )
                        .lineLimit(2...5)
                        .padding(10)
                        .background(ReinforceTheme.canvas)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .stroke(ReinforceTheme.border, lineWidth: 1)
                        )
                    }
                    .reinforceCard()

                    // Graph neighbors
                    VStack(alignment: .leading, spacing: 10) {
                        Label(l10n.t("reading.relatedNodes"), systemImage: "point.3.connected.trianglepath.dotted")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.ink)

                        if relatedCards.isEmpty {
                            Text(l10n.t("reading.noRelatedNodes"))
                                .font(.subheadline)
                                .foregroundStyle(ReinforceTheme.inkMuted)
                        } else {
                            VStack(spacing: 8) {
                                ForEach(relatedCards, id: \.id) { related in
                                    Button {
                                        onOpenCard(related.id)
                                    } label: {
                                        HStack {
                                            VStack(alignment: .leading, spacing: 2) {
                                                Text(related.title)
                                                    .font(.subheadline.weight(.medium))
                                                    .foregroundStyle(ReinforceTheme.ink)
                                                Text(related.subject)
                                                    .font(.caption)
                                                    .foregroundStyle(ReinforceTheme.inkMuted)
                                            }
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
                        }
                    }
                    .reinforceCard()
                } else {
                    Text(l10n.t("reading.missingCard"))
                        .foregroundStyle(ReinforceTheme.inkMuted)
                }
            }
            .padding()
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .background(ReinforceTheme.canvas)
        .navigationTitle(card?.title ?? l10n.t("reading.cardsHeading"))
        .navigationBarTitleDisplayMode(.inline)
        .sheet(item: $activeAnchor) { item in
            AnchorViewerSheet(anchorId: item.id, store: store, l10n: l10n)
        }
        .onAppear { loadCard() }
        .onChange(of: cardId) { _, _ in loadCard() }
    }

    private func loadCard() {
        drillError = nil
        card = store.loadKnowledgeCard(id: cardId)
        if let meta = try? store.database.userCardMeta(userId: store.userId, cardId: cardId) {
            starred = meta.starred
            personalNote = meta.personalNote ?? ""
        } else {
            starred = false
            personalNote = ""
        }
        let neighbors = store.loadNeighbors(cardId: cardId)
        relatedCards = neighbors.compactMap { neighbor in
            if case .card(let id) = neighbor.endpoint, id != cardId {
                return store.loadKnowledgeCard(id: id)
            }
            return nil
        }
    }

    private func toggleStar(_ card: KnowledgeCard) {
        starred.toggle()
        savePersonalMeta(card)
    }

    private func savePersonalMeta(_ card: KnowledgeCard) {
        let noteValue = personalNote.trimmingCharacters(in: .whitespacesAndNewlines)
        store.saveUserCardMeta(
            cardId: card.id,
            starred: starred,
            personalTags: [],
            personalNote: noteValue.isEmpty ? nil : noteValue
        )
    }

    private func drillCard(_ card: KnowledgeCard) {
        drillError = nil
        do {
            let launched = try store.startTopicDrill(cardIds: [card.id])
            if !launched {
                drillError = l10n.t("study.options.emptySessionError")
            }
        } catch {
            drillError = error.localizedDescription
        }
    }
}
