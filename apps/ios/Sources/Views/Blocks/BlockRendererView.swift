import SwiftUI

struct BlockRendererView: View {
    let block: ContentBlock
    let resolveBlobURL: (String) -> URL?
    let l10n: LocalizationManager
    var hidePinTerms: Bool = false
    var onOpenCard: ((String) -> Void)?

    var body: some View {
        switch block {
        case .markdown(let text):
            MarkdownBlockView(text: text)
        case .math(let latex, let display):
            MathBlockView(latex: latex, display: display)
        case .diagram(let source, let syntax, let svgCache, let nodeBindings):
            DiagramBlockView(
                source: source,
                syntax: syntax,
                svgCache: svgCache,
                nodeBindings: nodeBindings,
                l10n: l10n,
                onOpenCard: onOpenCard
            )
        case .imageOcclusion(let resourceId, let masks):
            ImageOcclusionBlockView(
                resourceId: resourceId,
                masks: masks,
                blobURL: resolveBlobURL(resourceId),
                l10n: l10n
            )
        case .interactivePins(let resourceId, let overlayResourceId, let pins):
            InteractivePinsBlockView(
                resourceId: resourceId,
                overlayResourceId: overlayResourceId,
                pins: pins,
                baseURL: resolveBlobURL(resourceId),
                overlayURL: overlayResourceId.flatMap(resolveBlobURL),
                l10n: l10n,
                hideLabelsAndTerms: hidePinTerms,
                onOpenCard: onOpenCard
            )
        }
    }
}
