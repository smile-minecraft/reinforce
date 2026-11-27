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
            html: wrapMathHTML(mathML: mathML, display: display, isDark: colorScheme == .dark),
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

    private func wrapMathHTML(mathML: String, display: Bool, isDark: Bool) -> String {
        let ink = isDark ? "#e6edf3" : "#141923"
        let fontSize = display ? "20px" : "16px"
        let align = display ? "center" : "left"
        return """
        <!doctype html>
        <html>
        <head>
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <style>
          html, body {
            margin: 0;
            padding: 4px 2px;
            background: transparent;
            color: \(ink);
            font-size: \(fontSize);
            text-align: \(align);
            font-family: "STIX Two Math", "Cambria Math", "Times New Roman", serif;
            overflow-x: auto;
            overflow-y: hidden;
          }
          math {
            font-size: 1em;
          }
        </style>
        </head>
        <body>\(mathML)</body>
        </html>
        """
    }
}

struct AutoSizingHTMLView: UIViewRepresentable {
    let html: String
    @Binding var contentHeight: CGFloat

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = context.coordinator
        webView.isOpaque = false
        webView.backgroundColor = .clear
        webView.scrollView.backgroundColor = .clear
        webView.scrollView.bounces = false
        webView.scrollView.showsVerticalScrollIndicator = false
        return webView
    }

    func updateUIView(_ webView: WKWebView, context: Context) {
        if context.coordinator.lastHTML == html { return }
        context.coordinator.lastHTML = html
        webView.loadHTMLString(html, baseURL: nil)
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(contentHeight: $contentHeight)
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        var lastHTML: String?
        @Binding var contentHeight: CGFloat

        init(contentHeight: Binding<CGFloat>) {
            self._contentHeight = contentHeight
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
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
