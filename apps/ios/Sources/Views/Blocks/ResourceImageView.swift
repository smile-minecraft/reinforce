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

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        let webView = WKWebView(frame: .zero, configuration: config)
        webView.isOpaque = false
        webView.backgroundColor = .clear
        webView.scrollView.backgroundColor = .clear
        webView.scrollView.isScrollEnabled = false
        webView.scrollView.bounces = false
        webView.isUserInteractionEnabled = false
        return webView
    }

    func updateUIView(_ webView: WKWebView, context: Context) {
        let key = "\(isDark ? "d" : "l"):\(svgMarkup.hashValue)"
        if context.coordinator.lastKey == key { return }
        context.coordinator.lastKey = key

        let ink = isDark ? "#e6edf3" : "#141923"
        let html = """
        <!doctype html>
        <html>
        <head>
        <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
        <style>
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
        </style>
        </head>
        <body>\(svgMarkup)</body>
        </html>
        """
        webView.loadHTMLString(html, baseURL: nil)
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator {
        var lastKey: String?
    }
}

struct ResourceImageView: View {
    let fileURL: URL?
    var fallbackAspectRatio: CGFloat = 16.0 / 10.0

    @Environment(\.colorScheme) private var colorScheme

    var loaded: LoadedMediaAsset? {
        MediaAssetCache.shared.asset(for: fileURL)
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
                    SVGWebView(svgMarkup: markup, isDark: colorScheme == .dark)
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
    }
}
