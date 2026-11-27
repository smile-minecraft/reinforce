import SwiftUI

struct DiagramBlockView: View {
    let source: String
    let syntax: DiagramSyntaxKind
    let svgCache: String?
    let nodeBindings: [DiagramNodeBinding]
    let l10n: LocalizationManager
    var onOpenCard: ((String) -> Void)?

    @Environment(\.colorScheme) private var colorScheme
    @State private var measuredHeight: CGFloat = 180

    private var svgToRender: String? {
        if let svgCache, !svgCache.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return svgCache
        }
        if syntax == .svg, !source.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return source
        }
        return nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let svg = svgToRender {
                let ratio = LoadedMediaAsset.parseSVGAspectRatio(from: svg)
                if let ratio {
                    SVGWebView(svgMarkup: svg, isDark: colorScheme == .dark)
                        .aspectRatio(ratio, contentMode: .fit)
                        .frame(maxWidth: .infinity)
                } else {
                    AutoSizingHTMLView(
                        html: wrapSVGHTML(svg, isDark: colorScheme == .dark),
                        contentHeight: $measuredHeight
                    )
                    .frame(height: max(120, measuredHeight))
                }
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    Text("\(l10n.t("blocks.diagramSource")) (\(syntax.rawValue))")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(ReinforceTheme.inkMuted)
                    ScrollView(.horizontal, showsIndicators: false) {
                        Text(source)
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(ReinforceTheme.ink)
                            .padding(10)
                    }
                    .background(ReinforceTheme.subtle)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
            }

            if !nodeBindings.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(nodeBindings, id: \.nodeId) { binding in
                            Button {
                                onOpenCard?(binding.cardId)
                            } label: {
                                Label(binding.nodeId, systemImage: "arrow.up.right.square")
                                    .font(.caption.weight(.medium))
                                    .foregroundStyle(ReinforceTheme.accent)
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 6)
                                    .background(ReinforceTheme.accentSoft)
                                    .clipShape(Capsule())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
        }
        .padding(12)
        .background(ReinforceTheme.surface)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(ReinforceTheme.border, lineWidth: 1)
        )
    }

    private func wrapSVGHTML(_ svg: String, isDark: Bool) -> String {
        OfflineWebViewFactory.wrapSVGDocument(svgMarkup: svg, isDark: isDark, fillViewport: false)
    }
}
