import SwiftUI

struct MarkdownBlockView: View {
    let text: String
    var muted: Bool = false

    private var elements: [MarkdownElement] {
        MarkdownParser.parse(text)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(elements) { element in
                elementView(element)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private func elementView(_ element: MarkdownElement) -> some View {
        switch element {
        case .heading(_, let level, let headingText):
            inlineText(headingText)
                .font(headingFont(for: level))
                .foregroundStyle(ReinforceTheme.ink)
                .padding(.top, level <= 2 ? 6 : 2)

        case .paragraph(_, let para):
            inlineText(para)
                .font(.body)
                .foregroundStyle(muted ? ReinforceTheme.inkMuted : ReinforceTheme.ink)
                .fixedSize(horizontal: false, vertical: true)

        case .bulletList(_, let items):
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("•")
                            .font(.body.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.accent)
                        inlineText(item)
                            .font(.body)
                            .foregroundStyle(muted ? ReinforceTheme.inkMuted : ReinforceTheme.ink)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }

        case .orderedList(_, let items):
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { idx, item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(idx + 1).")
                            .font(.callout.monospacedDigit().weight(.semibold))
                            .foregroundStyle(ReinforceTheme.accent)
                        inlineText(item)
                            .font(.body)
                            .foregroundStyle(muted ? ReinforceTheme.inkMuted : ReinforceTheme.ink)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }

        case .codeBlock(_, _, let code):
            ScrollView(.horizontal, showsIndicators: false) {
                Text(code)
                    .font(.system(.footnote, design: .monospaced))
                    .foregroundStyle(ReinforceTheme.ink)
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(ReinforceTheme.subtle)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(ReinforceTheme.border, lineWidth: 1)
            )

        case .blockquote(_, let quote):
            HStack(spacing: 0) {
                Rectangle()
                    .fill(ReinforceTheme.accent)
                    .frame(width: 3)
                inlineText(quote)
                    .font(.callout)
                    .foregroundStyle(ReinforceTheme.inkMuted)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(ReinforceTheme.subtle)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))

        case .callout(_, let kind, let title, let isOpenByDefault, let body):
            MarkdownCalloutView(
                kind: kind,
                title: title,
                isOpenByDefault: isOpenByDefault,
                bodyText: body
            )

        case .table(_, let headers, let rows):
            MarkdownTableView(headers: headers, rows: rows)
        }
    }

    private func headingFont(for level: Int) -> Font {
        switch level {
        case 1: return .title2.weight(.bold)
        case 2: return .title3.weight(.semibold)
        case 3: return .headline
        default: return .subheadline.weight(.semibold)
        }
    }

    private func inlineText(_ raw: String) -> Text {
        if let attributed = try? AttributedString(
            markdown: raw,
            options: AttributedString.MarkdownParsingOptions(
                interpretedSyntax: .inlineOnlyPreservingWhitespace
            )
        ) {
            return Text(attributed)
        }
        return Text(raw)
    }
}

private struct MarkdownCalloutView: View {
    let kind: String
    let title: String
    let bodyText: String
    @State private var isExpanded: Bool

    init(kind: String, title: String, isOpenByDefault: Bool, bodyText: String) {
        self.kind = kind
        self.title = title
        self.bodyText = bodyText
        self._isExpanded = State(initialValue: isOpenByDefault)
    }

    private var stripeColor: Color {
        switch kind {
        case "warning", "caution": return ReinforceTheme.warn
        case "tip", "success": return ReinforceTheme.ok
        case "danger", "error": return ReinforceTheme.bad
        default: return ReinforceTheme.accent
        }
    }

    var body: some View {
        HStack(spacing: 0) {
            Rectangle()
                .fill(stripeColor)
                .frame(width: 3)
            DisclosureGroup(isExpanded: $isExpanded) {
                MarkdownBlockView(text: bodyText, muted: false)
                    .padding(.top, 6)
            } label: {
                Text(title)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(ReinforceTheme.ink)
            }
            .tint(stripeColor)
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
        }
        .background(ReinforceTheme.subtle)
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .stroke(ReinforceTheme.border, lineWidth: 1)
        )
    }
}

private struct MarkdownTableView: View {
    let headers: [String]
    let rows: [[String]]

    private var columnCount: Int {
        max(headers.count, rows.map(\.count).max() ?? 0)
    }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: true) {
            Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(0..<columnCount, id: \.self) { col in
                        let value = col < headers.count ? headers[col] : ""
                        Text(value)
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.ink)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 10)
                            .frame(minWidth: 92, maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
                            .background(ReinforceTheme.subtle)
                    }
                }

                Divider()
                    .background(ReinforceTheme.border)

                ForEach(Array(rows.enumerated()), id: \.offset) { rowIdx, row in
                    let rowBg = rowIdx.isMultiple(of: 2) ? ReinforceTheme.surface : ReinforceTheme.subtle.opacity(0.45)
                    GridRow {
                        ForEach(0..<columnCount, id: \.self) { col in
                            let value = col < row.count ? row[col] : ""
                            Text(value)
                                .font(.subheadline)
                                .foregroundStyle(ReinforceTheme.ink)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 9)
                                .frame(minWidth: 92, maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
                                .background(rowBg)
                        }
                    }

                    if rowIdx < rows.count - 1 {
                        Divider()
                            .background(ReinforceTheme.border)
                    }
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(ReinforceTheme.border, lineWidth: 1)
            )
        }
    }
}
