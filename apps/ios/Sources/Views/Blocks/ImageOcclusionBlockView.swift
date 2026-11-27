import SwiftUI

struct ImageOcclusionBlockView: View {
    let resourceId: String
    let masks: [OcclusionMask]
    let blobURL: URL?
    let l10n: LocalizationManager

    @State private var revealedGroups: Set<String> = []

    private var allGroups: Set<String> {
        Set(masks.map(\.groupId))
    }

    private var allRevealed: Bool {
        !allGroups.isEmpty && allGroups.isSubset(of: revealedGroups)
    }

    private var aspectRatio: CGFloat {
        MediaAssetCache.shared.asset(for: blobURL)?.aspectRatio ?? (16.0 / 10.0)
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(l10n.t("blocks.occlusionHint"))
                    .font(.caption)
                    .foregroundStyle(ReinforceTheme.inkMuted)
                Spacer()
                Button {
                    if allRevealed {
                        revealedGroups.removeAll()
                    } else {
                        revealedGroups = allGroups
                    }
                } label: {
                    Label(
                        allRevealed ? l10n.t("blocks.hideAll") : l10n.t("blocks.revealAll"),
                        systemImage: allRevealed ? "eye.slash" : "eye"
                    )
                    .font(.caption.weight(.medium))
                    .foregroundStyle(ReinforceTheme.ink)
                    .padding(.horizontal, 10)
                    .frame(minHeight: 36)
                    .background(ReinforceTheme.surface)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .stroke(ReinforceTheme.border, lineWidth: 1)
                    )
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("occlusion-toggle-all")
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(ReinforceTheme.subtle.opacity(0.65))

            Divider().background(ReinforceTheme.border)

            GeometryReader { geo in
                ZStack(alignment: .topLeading) {
                    ResourceImageView(fileURL: blobURL, fallbackAspectRatio: aspectRatio)
                        .frame(width: geo.size.width, height: geo.size.height)

                    ForEach(Array(masks.enumerated()), id: \.offset) { index, mask in
                        let revealed = revealedGroups.contains(mask.groupId)
                        let width = max(24, CGFloat(mask.rect.w) * geo.size.width)
                        let height = max(24, CGFloat(mask.rect.h) * geo.size.height)
                        let centerX = (CGFloat(mask.rect.x) + CGFloat(mask.rect.w) / 2.0) * geo.size.width
                        let centerY = (CGFloat(mask.rect.y) + CGFloat(mask.rect.h) / 2.0) * geo.size.height

                        Button {
                            if revealed {
                                revealedGroups.remove(mask.groupId)
                            } else {
                                revealedGroups.insert(mask.groupId)
                            }
                        } label: {
                            ZStack {
                                RoundedRectangle(cornerRadius: 6, style: .continuous)
                                    .fill(revealed ? ReinforceTheme.accent.opacity(0.12) : ReinforceTheme.accent)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 6, style: .continuous)
                                            .stroke(ReinforceTheme.accent, lineWidth: 2)
                                    )
                                if !revealed {
                                    Text(mask.groupId)
                                        .font(.caption.weight(.bold))
                                        .foregroundStyle(.white)
                                        .lineLimit(1)
                                        .padding(.horizontal, 4)
                                }
                            }
                            .frame(width: width, height: height)
                        }
                        .buttonStyle(.plain)
                        .position(x: centerX, y: centerY)
                        .accessibilityLabel(l10n.t("blocks.maskGroup", mask.groupId))
                        .accessibilityIdentifier("occlusion-mask-\(index)")
                    }
                }
            }
            .aspectRatio(aspectRatio, contentMode: .fit)
            .background(ReinforceTheme.subtle.opacity(0.3))
        }
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(ReinforceTheme.border, lineWidth: 1)
        )
    }
}
