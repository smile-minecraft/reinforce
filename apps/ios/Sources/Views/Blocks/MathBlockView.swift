import SwiftUI
import WebKit

struct MathBlockView: View {
    let latex: String
    let display: Bool

    @Environment(\.colorScheme) private var colorScheme
    @State private var measuredHeight: CGFloat = 56

    var body: some View {
        let mathML = LatexToMathML.renderMathML(latex: latex, display: display)
        AutoSizingHTMLView(
            html: Self.wrapMathHTML(mathML: mathML, display: display, isDark: colorScheme == .dark),
            contentHeight: $measuredHeight
        )
        .frame(height: max(display ? 54 : 32, measuredHeight))
        .padding(.horizontal, display ? 12 : 4)
        .padding(.vertical, display ? 8 : 2)
        .frame(maxWidth: .infinity, alignment: display ? .center : .leading)
        .background(display ? ReinforceTheme.subtle.opacity(0.6) : Color.clear)
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .stroke(display ? ReinforceTheme.border : Color.clear, lineWidth: 1)
        )
        .accessibilityLabel(latex)
    }

    static func wrapMathHTML(mathML: String, display: Bool, isDark: Bool) -> String {
        OfflineWebViewFactory.wrapMathDocument(mathML: mathML, display: display, isDark: isDark)
    }
}

struct AutoSizingHTMLView: UIViewRepresentable {
    let html: String
    @Binding var contentHeight: CGFloat

    static func makeWebView() -> OfflineWKWebView {
        let webView = OfflineWebViewFactory.makeWebView(scrollEnabled: true)
        webView.scrollView.showsVerticalScrollIndicator = false
        return webView
    }

    func makeUIView(context: Context) -> OfflineWKWebView {
        let webView = Self.makeWebView()
        webView.navigationDelegate = context.coordinator.navigationDelegate
        webView.uiDelegate = context.coordinator.navigationDelegate
        context.coordinator.bind(webView: webView)
        return webView
    }

    func updateUIView(_ webView: OfflineWKWebView, context: Context) {
        if context.coordinator.lastHTML == html { return }
        context.coordinator.lastHTML = html
        context.coordinator.navigationDelegate.loadHTML(html, in: webView)
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(contentHeight: $contentHeight)
    }

    final class Coordinator {
        let navigationDelegate = OfflineWebNavigationDelegate()
        var lastHTML: String?
        @Binding var contentHeight: CGFloat

        init(contentHeight: Binding<CGFloat>) {
            self._contentHeight = contentHeight
        }

        func bind(webView: OfflineWKWebView) {
            navigationDelegate.onDidFinish = { [weak self] finishedWebView in
                self?.measureHeight(in: finishedWebView)
            }
            webView.onBoundsChange = { [weak self, weak webView] bounds in
                guard let self, let webView, bounds.width > 0 else { return }
                self.measureHeight(in: webView)
            }
        }

        private func measureHeight(in webView: WKWebView) {
            webView.evaluateJavaScript("Math.ceil(document.body.scrollHeight)") { result, _ in
                if let height = result as? CGFloat, height > 0 {
                    DispatchQueue.main.async {
                        self.contentHeight = min(max(height, 28), 640)
                    }
                } else if let number = result as? NSNumber, number.doubleValue > 0 {
                    DispatchQueue.main.async {
                        self.contentHeight = min(max(CGFloat(number.doubleValue), 28), 640)
                    }
                }
            }
        }
    }
}
