import SwiftUI

struct KnowledgeCardView: View {
    let card: KnowledgeCard
    let store: AppStore
    let l10n: LocalizationManager
    var showCardLink: Bool = false
    var onOpenCard: ((String) -> Void)?
    var onOpenAnchor: ((String) -> Void)?

    private var blocks: [ContentBlock] {
        ContentBlock.decodeArray(from: card.blocksJson)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 10) {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 8) {
                        Text(card.title)
                            .font(.headline)
                            .foregroundStyle(ReinforceTheme.ink)
                        verificationBadge
                    }

                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 6) {
                            Text(card.subject)
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(ReinforceTheme.inkMuted)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 3)
                                .background(ReinforceTheme.subtle)
                                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))

                            ForEach(card.topics, id: \.self) { topic in
                                Text(topic)
                                    .font(.caption2)
                                    .foregroundStyle(ReinforceTheme.inkMuted)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 3)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 6, style: .continuous)
                                            .stroke(ReinforceTheme.border, lineWidth: 1)
                                    )
                            }
                        }
                    }
                }

                Spacer(minLength: 4)

                if showCardLink, let onOpenCard {
                    Button {
                        onOpenCard(card.id)
                    } label: {
                        Label(l10n.t("reading.openCard"), systemImage: "arrow.up.right")
                            .font(.caption.weight(.medium))
                            .foregroundStyle(ReinforceTheme.accent)
                            .padding(.horizontal, 10)
                            .frame(minHeight: 34)
                            .background(ReinforceTheme.accentSoft)
                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    }
                    .buttonStyle(.plain)
                }
            }

            VStack(alignment: .leading, spacing: 12) {
                ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                    BlockRendererView(
                        block: block,
                        resolveBlobURL: { store.resolveBlobURL(for: $0) },
                        l10n: l10n,
                        onOpenCard: onOpenCard
                    )
                }
            }

            if !card.anchorIds.isEmpty, let onOpenAnchor {
                Divider().background(ReinforceTheme.border)
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        Text(l10n.t("reading.sources"))
                            .font(.caption.weight(.medium))
                            .foregroundStyle(ReinforceTheme.inkMuted)

                        ForEach(Array(card.anchorIds.enumerated()), id: \.element) { idx, anchorId in
                            Button {
                                onOpenAnchor(anchorId)
                            } label: {
                                Label(
                                    "\(l10n.t("reading.sourceAnchor")) #\(idx + 1)",
                                    systemImage: "doc.text.magnifyingglass"
                                )
                                .font(.caption.weight(.medium))
                                .foregroundStyle(ReinforceTheme.accent)
                                .padding(.horizontal, 10)
                                .frame(minHeight: 34)
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
        }
        .reinforceCard()
    }

    private var verificationBadge: some View {
        let verified = card.verification == .verified
        return Label(
            verified ? l10n.t("common.verified") : l10n.t("common.unchecked"),
            systemImage: verified ? "checkmark.circle.fill" : "clock"
        )
        .font(.caption2.weight(.medium))
        .foregroundStyle(verified ? ReinforceTheme.ok : ReinforceTheme.warn)
        .padding(.horizontal, 7)
        .padding(.vertical, 3)
        .background(verified ? ReinforceTheme.okSoft : ReinforceTheme.warnSoft)
        .clipShape(Capsule())
    }
}
