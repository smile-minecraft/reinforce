import SwiftUI
import UIKit
import WebKit

struct LoadedMediaAsset: Equatable {
    enum Payload: Equatable {
        case raster(UIImage)
        case svg(String)
    }

    let payload: Payload
    let aspectRatio: CGFloat

    static func load(from url: URL) -> LoadedMediaAsset? {
        guard let data = try? Data(contentsOf: url), !data.isEmpty else {
            return nil
        }
        if let image = UIImage(data: data), image.size.width > 0, image.size.height > 0 {
            return LoadedMediaAsset(
                payload: .raster(image),
                aspectRatio: image.size.width / image.size.height
            )
        }
        if let text = String(data: data, encoding: .utf8),
           text.range(of: "<svg", options: .caseInsensitive) != nil {
            let ratio = parseSVGAspectRatio(from: text) ?? (16.0 / 10.0)
            return LoadedMediaAsset(payload: .svg(text), aspectRatio: ratio)
        }
        return nil
    }

    static func parseSVGAspectRatio(from svg: String) -> CGFloat? {
        if let viewBoxRange = svg.range(of: #"viewBox\s*=\s*["']([^"']+)["']"#, options: .regularExpression) {
            let match = String(svg[viewBoxRange])
            let numbers = match
                .components(separatedBy: CharacterSet(charactersIn: "0123456789.-").inverted)
                .compactMap { Double($0) }
            if numbers.count >= 4, numbers[2] > 0, numbers[3] > 0 {
                return CGFloat(numbers[2] / numbers[3])
            }
        }
        let width = extractNumericAttribute("width", from: svg)
        let height = extractNumericAttribute("height", from: svg)
        if let width, let height, width > 0, height > 0 {
            return CGFloat(width / height)
        }
        return nil
    }

    private static func extractNumericAttribute(_ name: String, from svg: String) -> Double? {
        let pattern = #"\b"# + name + #"\s*=\s*["']([0-9]+(?:\.[0-9]+)?)"#
        guard let range = svg.range(of: pattern, options: .regularExpression) else { return nil }
        let slice = String(svg[range])
        let digits = slice
            .components(separatedBy: CharacterSet(charactersIn: "0123456789.").inverted)
            .first(where: { !$0.isEmpty })
        return digits.flatMap(Double.init)
    }
}

final class MediaAssetCache {
    static let shared = MediaAssetCache()
    private var storage: [URL: LoadedMediaAsset] = [:]
    private let lock = NSLock()

    func asset(for url: URL?) -> LoadedMediaAsset? {
        guard let url else { return nil }
        lock.lock()
        if let cached = storage[url] {
            lock.unlock()
            return cached
        }
        lock.unlock()

        guard let loaded = LoadedMediaAsset.load(from: url) else { return nil }
        lock.lock()
        storage[url] = loaded
        lock.unlock()
        return loaded
    }

    func clear() {
        lock.lock()
        storage.removeAll()
        lock.unlock()
    }
}

struct SVGWebView: UIViewRepresentable {
    let svgMarkup: String
    let isDark: Bool
    var onLoadStateChange: ((Bool) -> Void)? = nil

    static func makeWebView() -> OfflineWKWebView {
        let webView = OfflineWebViewFactory.makeWebView(scrollEnabled: false)
        webView.isUserInteractionEnabled = false
        return webView
    }

    static func makeHTML(svgMarkup: String, isDark: Bool) -> String {
        OfflineWebViewFactory.wrapSVGDocument(svgMarkup: svgMarkup, isDark: isDark, fillViewport: true)
    }

    func makeUIView(context: Context) -> OfflineWKWebView {
        let webView = Self.makeWebView()
        webView.navigationDelegate = context.coordinator.navigationDelegate
        webView.uiDelegate = context.coordinator.navigationDelegate
        context.coordinator.bind(webView: webView, onLoadStateChange: onLoadStateChange)
        return webView
    }

    func updateUIView(_ webView: OfflineWKWebView, context: Context) {
        context.coordinator.onLoadStateChange = onLoadStateChange
        let html = Self.makeHTML(svgMarkup: svgMarkup, isDark: isDark)
        if context.coordinator.lastHTML == html {
            context.coordinator.checkReadinessIfNeeded(in: webView)
            return
        }
        context.coordinator.load(html: html, in: webView)
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator {
        let navigationDelegate = OfflineWebNavigationDelegate()
        var lastHTML: String?
        var onLoadStateChange: ((Bool) -> Void)?

        private var loadGeneration = 0
        private var hasFinishedNavigation = false
        private var hasReportedLoaded = false
        private var isCheckingReadiness = false

        func bind(webView: OfflineWKWebView, onLoadStateChange: ((Bool) -> Void)?) {
            self.onLoadStateChange = onLoadStateChange
            navigationDelegate.onDidFinish = { [weak self] finishedWebView in
                guard let self else { return }
                self.hasFinishedNavigation = true
                self.checkReadinessIfNeeded(in: finishedWebView)
            }
            navigationDelegate.onDidFail = { [weak self] _, _ in
                guard let self else { return }
                self.hasFinishedNavigation = false
                self.hasReportedLoaded = false
                DispatchQueue.main.async {
                    self.onLoadStateChange?(false)
                }
            }
            webView.onBoundsChange = { [weak self, weak webView] _ in
                guard let self, let webView else { return }
                self.checkReadinessIfNeeded(in: webView)
            }
        }

        func load(html: String, in webView: WKWebView) {
            lastHTML = html
            loadGeneration += 1
            hasFinishedNavigation = false
            hasReportedLoaded = false
            isCheckingReadiness = false
            DispatchQueue.main.async { [weak self] in
                self?.onLoadStateChange?(false)
            }
            navigationDelegate.loadHTML(html, in: webView)
        }

        func checkReadinessIfNeeded(in webView: WKWebView) {
            guard hasFinishedNavigation,
                  !hasReportedLoaded,
                  !isCheckingReadiness,
                  webView.bounds.width > 0,
                  webView.bounds.height > 0 else {
                return
            }

            isCheckingReadiness = true
            let generation = loadGeneration
            let probeJS = """
            (() => {
              const svg = document.querySelector('svg');
              if (!svg) return false;
              const rect = svg.getBoundingClientRect();
              return rect.width > 0 && rect.height > 0;
            })()
            """

            webView.evaluateJavaScript(probeJS) { [weak self, weak webView] result, _ in
                guard let self, let webView, self.loadGeneration == generation else { return }
                let hasRenderedSVG = (result as? Bool) ?? ((result as? NSNumber)?.boolValue ?? false)
                guard hasRenderedSVG else {
                    self.isCheckingReadiness = false
                    return
                }

                // Force a tiny compositor snapshot flush so the WebContent layer is committed to screen.
                let snapConfig = WKSnapshotConfiguration()
                snapConfig.rect = CGRect(x: 0, y: 0, width: 1, height: 1)
                webView.takeSnapshot(with: snapConfig) { [weak self] _, _ in
                    guard let self, self.loadGeneration == generation else { return }
                    self.isCheckingReadiness = false
                    self.hasReportedLoaded = true
                    DispatchQueue.main.async {
                        self.onLoadStateChange?(true)
                    }
                }
            }
        }
    }
}

struct ResourceImageView: View {
    let fileURL: URL?
    var fallbackAspectRatio: CGFloat = 16.0 / 10.0
    var accessibilityId: String? = nil

    @Environment(\.colorScheme) private var colorScheme
    @State private var svgLoaded = false

    var loaded: LoadedMediaAsset? {
        MediaAssetCache.shared.asset(for: fileURL)
    }

    private var isImageLoaded: Bool {
        guard let loaded else { return false }
        switch loaded.payload {
        case .raster:
            return true
        case .svg:
            return svgLoaded
        }
    }

    var body: some View {
        Group {
            if let loaded {
                switch loaded.payload {
                case .raster(let image):
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFill()
                case .svg(let markup):
                    SVGWebView(
                        svgMarkup: markup,
                        isDark: colorScheme == .dark,
                        onLoadStateChange: { ready in
                            if svgLoaded != ready {
                                svgLoaded = ready
                            }
                        }
                    )
                }
            } else {
                ZStack {
                    ReinforceTheme.subtle
                    Image(systemName: "photo")
                        .font(.title2)
                        .foregroundStyle(ReinforceTheme.inkMuted)
                }
            }
        }
        .onChange(of: fileURL) { _, _ in
            svgLoaded = false
        }
        .overlay {
            if let accessibilityId {
                Color.clear
                    .allowsHitTesting(false)
                    .accessibilityElement()
                    .accessibilityIdentifier(accessibilityId)
                    .accessibilityValue(isImageLoaded ? "loaded" : "loading")
            }
        }
    }
}
