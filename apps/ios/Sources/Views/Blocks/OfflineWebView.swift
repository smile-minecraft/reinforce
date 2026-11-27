import UIKit
import WebKit

final class OfflineWKWebView: WKWebView {
    var onBoundsChange: ((CGRect) -> Void)?
    private var lastReportedSize: CGSize = .zero

    override func layoutSubviews() {
        super.layoutSubviews()
        if bounds.size != lastReportedSize {
            lastReportedSize = bounds.size
            onBoundsChange?(bounds)
        }
    }
}

enum OfflineWebViewFactory {
    static let contentSecurityPolicy = "default-src 'none'; style-src 'unsafe-inline'; img-src data:"

    static var cspMetaTag: String {
        #"<meta http-equiv="Content-Security-Policy" content="\#(contentSecurityPolicy)">"#
    }

    static func makeConfiguration() -> WKWebViewConfiguration {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        config.websiteDataStore = .nonPersistent()
        config.allowsInlineMediaPlayback = false
        config.mediaTypesRequiringUserActionForPlayback = .all
        return config
    }

    static func makeWebView(scrollEnabled: Bool = false) -> OfflineWKWebView {
        let webView = OfflineWKWebView(frame: .zero, configuration: makeConfiguration())
        webView.isOpaque = false
        webView.backgroundColor = .clear
        webView.underPageBackgroundColor = .clear
        webView.scrollView.backgroundColor = .clear
        webView.scrollView.isScrollEnabled = scrollEnabled
        webView.scrollView.bounces = false
        webView.scrollView.contentInsetAdjustmentBehavior = .never
        webView.allowsLinkPreview = false
        webView.allowsBackForwardNavigationGestures = false
        return webView
    }

    static func wrapHTML(
        body: String,
        css: String,
        viewport: String = "width=device-width, initial-scale=1.0"
    ) -> String {
        """
        <!doctype html>
        <html>
        <head>
        <meta charset="utf-8">
        <meta name="viewport" content="\(viewport)">
        \(cspMetaTag)
        <style>
        \(css)
        </style>
        </head>
        <body>\(body)</body>
        </html>
        """
    }

    static func wrapSVGDocument(svgMarkup: String, isDark: Bool, fillViewport: Bool) -> String {
        let ink = isDark ? "#e6edf3" : "#141923"
        let viewport = fillViewport
            ? "width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no"
            : "width=device-width, initial-scale=1.0"
        let css: String
        if fillViewport {
            css = """
              html, body {
                margin: 0;
                padding: 0;
                width: 100%;
                height: 100%;
                overflow: hidden;
                background: transparent;
                color: \(ink);
              }
              svg {
                display: block;
                width: 100% !important;
                height: 100% !important;
              }
            """
        } else {
            css = """
              html, body {
                margin: 0;
                padding: 0;
                background: transparent;
                color: \(ink);
              }
              svg {
                display: block;
                max-width: 100%;
                height: auto;
                margin: 0 auto;
              }
            """
        }
        return wrapHTML(body: svgMarkup, css: css, viewport: viewport)
    }

    static func wrapMathDocument(mathML: String, display: Bool, isDark: Bool) -> String {
        let ink = isDark ? "#e6edf3" : "#141923"
        let fontSize = display ? "20px" : "16px"
        let align = display ? "center" : "left"
        let css = """
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
        """
        return wrapHTML(body: mathML, css: css)
    }
}

final class OfflineWebNavigationDelegate: NSObject, WKNavigationDelegate, WKUIDelegate {
    private(set) var allowsNextInitialLoad = false
    var onDidFinish: ((WKWebView) -> Void)?
    var onDidFail: ((WKWebView, Error) -> Void)?

    func loadHTML(_ html: String, in webView: WKWebView) {
        allowsNextInitialLoad = true
        webView.loadHTMLString(html, baseURL: nil)
    }

    func navigationPolicy(
        for url: URL?,
        navigationType: WKNavigationType,
        isMainFrame: Bool
    ) -> WKNavigationActionPolicy {
        guard allowsNextInitialLoad,
              isMainFrame,
              navigationType == .other,
              url?.absoluteString == "about:blank" else {
            return .cancel
        }
        allowsNextInitialLoad = false
        return .allow
    }

    func webView(
        _ webView: WKWebView,
        decidePolicyFor navigationAction: WKNavigationAction,
        preferences: WKWebpagePreferences,
        decisionHandler: @escaping (WKNavigationActionPolicy, WKWebpagePreferences) -> Void
    ) {
        preferences.allowsContentJavaScript = false
        let policy = navigationPolicy(
            for: navigationAction.request.url,
            navigationType: navigationAction.navigationType,
            isMainFrame: navigationAction.targetFrame?.isMainFrame ?? false
        )
        decisionHandler(policy, preferences)
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        onDidFinish?(webView)
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        onDidFail?(webView, error)
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        onDidFail?(webView, error)
    }

    func webView(
        _ webView: WKWebView,
        createWebViewWith configuration: WKWebViewConfiguration,
        for navigationAction: WKNavigationAction,
        windowFeatures: WKWindowFeatures
    ) -> WKWebView? {
        nil
    }
}
