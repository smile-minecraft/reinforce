import Foundation

enum DeepLinkTarget: Equatable, Hashable, Sendable {
    case card(String)
    case question(String)
    case note(String)
}

enum DeepLinkParser {
    static let scheme = "reinforce"

    static func parse(_ url: URL) -> DeepLinkTarget? {
        guard let urlScheme = url.scheme?.lowercased(), urlScheme == scheme else {
            return nil
        }

        let host = url.host?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() ?? ""
        let pathSegments = url.pathComponents
            .filter { $0 != "/" }
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }

        let kind: String
        let identifier: String

        if !host.isEmpty {
            guard pathSegments.count == 1 else { return nil }
            kind = host
            identifier = pathSegments[0]
        } else {
            // Also accept reinforce:///card/<id>
            guard pathSegments.count == 2 else { return nil }
            kind = pathSegments[0].lowercased()
            identifier = pathSegments[1]
        }

        guard !identifier.isEmpty else { return nil }

        switch kind {
        case "card", "cards":
            return .card(identifier)
        case "question", "questions":
            return .question(identifier)
        case "note", "notes":
            return .note(identifier)
        default:
            return nil
        }
    }
}
