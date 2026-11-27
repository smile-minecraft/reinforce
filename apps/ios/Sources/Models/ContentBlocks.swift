import Foundation

struct BlockRect: Codable, Equatable, Hashable, Sendable {
    let x: Double
    let y: Double
    let w: Double
    let h: Double
}

struct OcclusionMask: Codable, Equatable, Hashable, Sendable {
    let groupId: String
    let rect: BlockRect

    enum CodingKeys: String, CodingKey {
        case groupId = "group_id"
        case rect
    }
}

struct DiagramNodeBinding: Codable, Equatable, Hashable, Sendable {
    let nodeId: String
    let cardId: String

    enum CodingKeys: String, CodingKey {
        case nodeId = "node_id"
        case cardId = "card_id"
    }
}

struct DiagramPin: Codable, Equatable, Hashable, Identifiable, Sendable {
    let no: Int
    let x: Double
    let y: Double
    let labelX: Double
    let labelY: Double
    let termPrimary: String
    let termSecondary: String?
    let detail: String?
    let hint: String?
    let linkedCardId: String?

    var id: Int { no }

    enum CodingKeys: String, CodingKey {
        case no
        case x
        case y
        case labelX = "label_x"
        case labelY = "label_y"
        case termPrimary = "term_primary"
        case termSecondary = "term_secondary"
        case detail
        case hint
        case linkedCardId = "linked_card_id"
    }
}

enum DiagramSyntaxKind: String, Codable, Equatable, Hashable, Sendable {
    case mermaid
    case tikz
    case svg
}

enum ContentBlock: Codable, Equatable, Hashable, Sendable {
    case markdown(text: String)
    case math(latex: String, display: Bool)
    case diagram(source: String, syntax: DiagramSyntaxKind, svgCache: String?, nodeBindings: [DiagramNodeBinding])
    case imageOcclusion(resourceId: String, masks: [OcclusionMask])
    case interactivePins(resourceId: String, overlayResourceId: String?, pins: [DiagramPin])

    private enum CodingKeys: String, CodingKey {
        case type
        case text
        case latex
        case display
        case source
        case syntax
        case svgCache = "svg_cache"
        case nodeBindings = "node_bindings"
        case resourceId = "resource_id"
        case masks
        case overlayResourceId = "overlay_resource_id"
        case pins
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)
        switch type {
        case "markdown":
            let text = try container.decode(String.self, forKey: .text)
            self = .markdown(text: text)
        case "math":
            let latex = try container.decode(String.self, forKey: .latex)
            let display = try container.decodeIfPresent(Bool.self, forKey: .display) ?? false
            self = .math(latex: latex, display: display)
        case "diagram":
            let source = try container.decode(String.self, forKey: .source)
            let syntax = try container.decode(DiagramSyntaxKind.self, forKey: .syntax)
            let svgCache = try container.decodeIfPresent(String.self, forKey: .svgCache)
            let nodeBindings = try container.decodeIfPresent([DiagramNodeBinding].self, forKey: .nodeBindings) ?? []
            self = .diagram(source: source, syntax: syntax, svgCache: svgCache, nodeBindings: nodeBindings)
        case "image_occlusion":
            let resourceId = try container.decode(String.self, forKey: .resourceId)
            let masks = try container.decodeIfPresent([OcclusionMask].self, forKey: .masks) ?? []
            self = .imageOcclusion(resourceId: resourceId, masks: masks)
        case "interactive_pins":
            let resourceId = try container.decode(String.self, forKey: .resourceId)
            let overlayResourceId = try container.decodeIfPresent(String.self, forKey: .overlayResourceId)
            let pins = try container.decodeIfPresent([DiagramPin].self, forKey: .pins) ?? []
            self = .interactivePins(resourceId: resourceId, overlayResourceId: overlayResourceId, pins: pins)
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .type,
                in: container,
                debugDescription: "Unknown content block type: \(type)"
            )
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .markdown(let text):
            try container.encode("markdown", forKey: .type)
            try container.encode(text, forKey: .text)
        case .math(let latex, let display):
            try container.encode("math", forKey: .type)
            try container.encode(latex, forKey: .latex)
            try container.encode(display, forKey: .display)
        case .diagram(let source, let syntax, let svgCache, let nodeBindings):
            try container.encode("diagram", forKey: .type)
            try container.encode(source, forKey: .source)
            try container.encode(syntax, forKey: .syntax)
            try container.encodeIfPresent(svgCache, forKey: .svgCache)
            try container.encode(nodeBindings, forKey: .nodeBindings)
        case .imageOcclusion(let resourceId, let masks):
            try container.encode("image_occlusion", forKey: .type)
            try container.encode(resourceId, forKey: .resourceId)
            try container.encode(masks, forKey: .masks)
        case .interactivePins(let resourceId, let overlayResourceId, let pins):
            try container.encode("interactive_pins", forKey: .type)
            try container.encode(resourceId, forKey: .resourceId)
            try container.encodeIfPresent(overlayResourceId, forKey: .overlayResourceId)
            try container.encode(pins, forKey: .pins)
        }
    }

    static func decodeArray(from jsonString: String) -> [ContentBlock] {
        guard let data = jsonString.data(using: .utf8), !jsonString.isEmpty else {
            return []
        }
        return (try? JSONDecoder().decode([ContentBlock].self, from: data)) ?? []
    }

    var plainSummary: String {
        switch self {
        case .markdown(let text):
            return text.replacingOccurrences(of: "\n", with: " ")
        case .math(let latex, _):
            return latex
        case .diagram(_, let syntax, _, _):
            return "[\(syntax.rawValue)]"
        case .imageOcclusion:
            return "[image occlusion]"
        case .interactivePins(_, _, let pins):
            return "[\(pins.count) pins]"
        }
    }
}

struct PracticalMediaInfo: Equatable, Sendable {
    let resourceId: String
    let overlayResourceId: String?
    let pins: [DiagramPin]
}

extension Array where Element == ContentBlock {
    var practicalMedia: PracticalMediaInfo? {
        for block in self {
            switch block {
            case .interactivePins(let resourceId, let overlayResourceId, let pins):
                return PracticalMediaInfo(
                    resourceId: resourceId,
                    overlayResourceId: overlayResourceId,
                    pins: pins
                )
            case .imageOcclusion(let resourceId, _):
                return PracticalMediaInfo(resourceId: resourceId, overlayResourceId: nil, pins: [])
            default:
                continue
            }
        }
        return nil
    }
}
