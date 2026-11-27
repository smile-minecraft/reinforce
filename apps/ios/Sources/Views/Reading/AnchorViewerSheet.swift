import SwiftUI

struct AnchorSheetItem: Identifiable {
    let id: String
}

struct AnchorViewerSheet: View {
    let anchorId: String
    let store: AppStore
    let l10n: LocalizationManager

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let anchor = store.loadAnchor(id: anchorId)
        let resource = anchor.flatMap { store.loadResource(id: $0.resourceId) }
        let blobURL = resource.flatMap { store.resolveBlobURL(for: $0.id) }

        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if let anchor, let resource {
                        metadataPills(anchor: anchor, resource: resource)

                        if resource.kind == .image {
                            if let blobURL {
                                let ratio = MediaAssetCache.shared.asset(for: blobURL)?.aspectRatio ?? (16.0 / 10.0)
                                GeometryReader { geo in
                                    ZStack(alignment: .topLeading) {
                                        ResourceImageView(fileURL: blobURL, fallbackAspectRatio: ratio)
                                            .frame(width: geo.size.width, height: geo.size.height)

                                        if let rect = anchor.rect {
                                            RoundedRectangle(cornerRadius: 6, style: .continuous)
                                                .fill(ReinforceTheme.accent.opacity(0.16))
                                                .overlay(
                                                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                                                        .stroke(ReinforceTheme.accent, lineWidth: 2.5)
                                                )
                                                .frame(
                                                    width: CGFloat(rect.w) * geo.size.width,
                                                    height: CGFloat(rect.h) * geo.size.height
                                                )
                                                .position(
                                                    x: (CGFloat(rect.x) + CGFloat(rect.w) / 2.0) * geo.size.width,
                                                    y: (CGFloat(rect.y) + CGFloat(rect.h) / 2.0) * geo.size.height
                                                )
                                                .accessibilityLabel(l10n.t("anchor.regionHighlight"))
                                        }
                                    }
                                }
                                .aspectRatio(ratio, contentMode: .fit)
                                .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                                        .stroke(ReinforceTheme.border, lineWidth: 1)
                                )
                            } else {
                                Text(l10n.t("anchor.blobUnavailable"))
                                    .font(.subheadline)
                                    .foregroundStyle(ReinforceTheme.inkMuted)
                                    .reinforceCard()
                            }
                        }

                        if resource.license != nil || resource.attributionNotice != nil {
                            VStack(alignment: .leading, spacing: 6) {
                                if let license = resource.license {
                                    Text("\(l10n.t("anchor.license")): \(license)")
                                        .font(.caption)
                                        .foregroundStyle(ReinforceTheme.inkMuted)
                                }
                                if let notice = resource.attributionNotice {
                                    Text("\(l10n.t("anchor.attribution")): \(notice)")
                                        .font(.caption)
                                        .foregroundStyle(ReinforceTheme.inkMuted)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(12)
                            .background(ReinforceTheme.subtle)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                    } else {
                        Text(l10n.t("common.empty"))
                            .foregroundStyle(ReinforceTheme.inkMuted)
                    }
                }
                .padding()
            }
            .background(ReinforceTheme.canvas)
            .navigationTitle(resource?.title ?? l10n.t("anchor.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(l10n.t("common.close")) {
                        dismiss()
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func metadataPills(anchor: Anchor, resource: Resource) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                if let page = anchor.pdfPage {
                    pill(l10n.t("anchor.pdfPage", Int(page)), tint: ReinforceTheme.accent, bg: ReinforceTheme.accentSoft)
                }
                if let printed = anchor.printedPage {
                    pill(l10n.t("anchor.printedPage", Int(printed)), tint: ReinforceTheme.inkMuted, bg: ReinforceTheme.subtle)
                }
                if let legend = anchor.legendPdfPage {
                    pill(l10n.t("anchor.legendPage", Int(legend)), tint: ReinforceTheme.inkMuted, bg: ReinforceTheme.subtle)
                }
                if resource.sensitivity == .restricted {
                    pill(
                        l10n.t("anchor.restrictedNotice", "\(resource.exportPolicy)"),
                        tint: ReinforceTheme.warn,
                        bg: ReinforceTheme.warnSoft
                    )
                }
            }
        }
    }

    private func pill(_ text: String, tint: Color, bg: Color) -> some View {
        Text(text)
            .font(.caption.weight(.medium))
            .foregroundStyle(tint)
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(bg)
            .clipShape(Capsule())
    }
}
