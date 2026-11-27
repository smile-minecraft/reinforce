import Foundation

enum MarkdownElement: Equatable, Identifiable, Sendable {
    case heading(id: Int, level: Int, text: String)
    case paragraph(id: Int, text: String)
    case bulletList(id: Int, items: [String])
    case orderedList(id: Int, items: [String])
    case codeBlock(id: Int, language: String?, code: String)
    case blockquote(id: Int, text: String)
    case callout(id: Int, kind: String, title: String, isOpenByDefault: Bool, body: String)
    case table(id: Int, headers: [String], rows: [[String]])

    var id: Int {
        switch self {
        case .heading(let id, _, _),
             .paragraph(let id, _),
             .bulletList(let id, _),
             .orderedList(let id, _),
             .codeBlock(let id, _, _),
             .blockquote(let id, _),
             .callout(let id, _, _, _, _),
             .table(let id, _, _):
            return id
        }
    }
}

enum MarkdownParser {
    static func parse(_ source: String) -> [MarkdownElement] {
        let normalized = source.replacingOccurrences(of: "\r\n", with: "\n")
        let lines = normalized.components(separatedBy: "\n")
        var elements: [MarkdownElement] = []
        var index = 0
        var nextId = 0

        func makeId() -> Int {
            defer { nextId += 1 }
            return nextId
        }

        while index < lines.count {
            let line = lines[index]
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            if trimmed.isEmpty {
                index += 1
                continue
            }

            // Fenced code block
            if trimmed.hasPrefix("```") {
                let lang = String(trimmed.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                var codeLines: [String] = []
                index += 1
                while index < lines.count && !lines[index].trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                    codeLines.append(lines[index])
                    index += 1
                }
                if index < lines.count { index += 1 }
                elements.append(.codeBlock(
                    id: makeId(),
                    language: lang.isEmpty ? nil : lang,
                    code: codeLines.joined(separator: "\n")
                ))
                continue
            }

            // Heading
            if let heading = parseHeading(trimmed) {
                elements.append(.heading(id: makeId(), level: heading.level, text: heading.text))
                index += 1
                continue
            }

            // GFM Pipe Table
            if index + 1 < lines.count,
               isTableRow(trimmed),
               isTableSeparator(lines[index + 1].trimmingCharacters(in: .whitespaces)) {
                let headers = splitTableRow(trimmed)
                index += 2
                var rows: [[String]] = []
                while index < lines.count {
                    let rowTrimmed = lines[index].trimmingCharacters(in: .whitespaces)
                    if rowTrimmed.isEmpty || !isTableRow(rowTrimmed) {
                        break
                    }
                    rows.append(splitTableRow(rowTrimmed))
                    index += 1
                }
                elements.append(.table(id: makeId(), headers: headers, rows: rows))
                continue
            }

            // Callout or Blockquote
            if trimmed.hasPrefix(">") {
                if let calloutHeader = parseCalloutHeader(trimmed) {
                    var bodyLines: [String] = []
                    index += 1
                    while index < lines.count && lines[index].trimmingCharacters(in: .whitespaces).hasPrefix(">") {
                        bodyLines.append(stripQuotePrefix(lines[index]))
                        index += 1
                    }
                    elements.append(.callout(
                        id: makeId(),
                        kind: calloutHeader.kind,
                        title: calloutHeader.title,
                        isOpenByDefault: calloutHeader.isOpen,
                        body: bodyLines.joined(separator: "\n")
                    ))
                    continue
                }

                var quoteLines: [String] = []
                while index < lines.count && lines[index].trimmingCharacters(in: .whitespaces).hasPrefix(">") {
                    quoteLines.append(stripQuotePrefix(lines[index]))
                    index += 1
                }
                elements.append(.blockquote(id: makeId(), text: quoteLines.joined(separator: "\n")))
                continue
            }

            // Bullet list
            if isBulletItem(trimmed) {
                var items: [String] = []
                while index < lines.count {
                    let t = lines[index].trimmingCharacters(in: .whitespaces)
                    guard let itemText = stripBulletPrefix(t) else { break }
                    items.append(itemText)
                    index += 1
                }
                elements.append(.bulletList(id: makeId(), items: items))
                continue
            }

            // Ordered list
            if stripOrderedPrefix(trimmed) != nil {
                var items: [String] = []
                while index < lines.count {
                    let t = lines[index].trimmingCharacters(in: .whitespaces)
                    guard let itemText = stripOrderedPrefix(t) else { break }
                    items.append(itemText)
                    index += 1
                }
                elements.append(.orderedList(id: makeId(), items: items))
                continue
            }

            // Paragraph
            var paraLines: [String] = []
            while index < lines.count {
                let current = lines[index]
                let currentTrimmed = current.trimmingCharacters(in: .whitespaces)
                if currentTrimmed.isEmpty
                    || currentTrimmed.hasPrefix("```")
                    || parseHeading(currentTrimmed) != nil
                    || currentTrimmed.hasPrefix(">")
                    || isBulletItem(currentTrimmed)
                    || stripOrderedPrefix(currentTrimmed) != nil
                    || (index + 1 < lines.count && isTableRow(currentTrimmed) && isTableSeparator(lines[index + 1].trimmingCharacters(in: .whitespaces))) {
                    break
                }
                paraLines.append(currentTrimmed)
                index += 1
            }
            elements.append(.paragraph(id: makeId(), text: paraLines.joined(separator: " ")))
        }

        return elements
    }

    private static func parseHeading(_ line: String) -> (level: Int, text: String)? {
        var level = 0
        for ch in line {
            if ch == "#" {
                level += 1
            } else {
                break
            }
        }
        guard (1...4).contains(level), line.count > level else { return nil }
        let idx = line.index(line.startIndex, offsetBy: level)
        guard line[idx] == " " else { return nil }
        let text = String(line[line.index(after: idx)...]).trimmingCharacters(in: .whitespaces)
        return text.isEmpty ? nil : (level, text)
    }

    private static func isTableRow(_ line: String) -> Bool {
        line.contains("|")
    }

    private static func isTableSeparator(_ line: String) -> Bool {
        guard line.contains("|") && line.contains("-") else { return false }
        let stripped = line.replacingOccurrences(of: "|", with: "")
            .replacingOccurrences(of: "-", with: "")
            .replacingOccurrences(of: ":", with: "")
            .trimmingCharacters(in: .whitespaces)
        return stripped.isEmpty
    }

    private static func splitTableRow(_ line: String) -> [String] {
        var working = line.trimmingCharacters(in: .whitespaces)
        if working.hasPrefix("|") { working.removeFirst() }
        if working.hasSuffix("|") { working.removeLast() }
        return working.components(separatedBy: "|").map { $0.trimmingCharacters(in: .whitespaces) }
    }

    private static func stripQuotePrefix(_ line: String) -> String {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.hasPrefix(">") else { return trimmed }
        var rest = String(trimmed.dropFirst())
        if rest.hasPrefix(" ") { rest.removeFirst() }
        return rest
    }

    private static func parseCalloutHeader(_ line: String) -> (kind: String, title: String, isOpen: Bool)? {
        let content = stripQuotePrefix(line)
        guard content.hasPrefix("[!") else { return nil }
        guard let closeBracket = content.firstIndex(of: "]") else { return nil }
        let kindStart = content.index(content.startIndex, offsetBy: 2)
        let kind = String(content[kindStart..<closeBracket]).lowercased()
        guard !kind.isEmpty else { return nil }

        var cursor = content.index(after: closeBracket)
        var isOpen = true
        if cursor < content.endIndex, (content[cursor] == "-" || content[cursor] == "+") {
            isOpen = content[cursor] != "-"
            cursor = content.index(after: cursor)
        }
        let rawTitle = String(content[cursor...]).trimmingCharacters(in: .whitespaces)
        let title = rawTitle.isEmpty ? kind.capitalized : rawTitle
        return (kind, title, isOpen)
    }

    private static func isBulletItem(_ line: String) -> Bool {
        stripBulletPrefix(line) != nil
    }

    private static func stripBulletPrefix(_ line: String) -> String? {
        for prefix in ["- ", "* ", "+ "] {
            if line.hasPrefix(prefix) {
                return String(line.dropFirst(prefix.count))
            }
        }
        return nil
    }

    private static func stripOrderedPrefix(_ line: String) -> String? {
        var idx = line.startIndex
        var digits = 0
        while idx < line.endIndex, line[idx].isNumber {
            digits += 1
            idx = line.index(after: idx)
        }
        guard digits > 0, idx < line.endIndex, line[idx] == "." else { return nil }
        let afterDot = line.index(after: idx)
        guard afterDot < line.endIndex, line[afterDot] == " " else { return nil }
        return String(line[line.index(after: afterDot)...])
    }
}
