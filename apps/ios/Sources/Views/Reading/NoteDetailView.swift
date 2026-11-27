import SwiftUI

struct NoteDetailView: View {
    let note: Note
    let store: AppStore
    let l10n: LocalizationManager
    var onOpenCard: (String) -> Void
    var onRequestExport: ((PDFExportContext) -> Void)? = nil

    @State private var outlineNodes: [NoteOutlineNode] = []
    @State private var loadedCards: [String: KnowledgeCard] = [:]
    @State private var activeAnchor: AnchorSheetItem?
    @State private var showOutlinePopover = false
    @State private var errorMessage: String?

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(note.subject)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.accent)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 4)
                            .background(ReinforceTheme.accentSoft)
                            .clipShape(Capsule())

                        Text(note.title)
                            .font(.largeTitle.weight(.bold))
                            .foregroundStyle(ReinforceTheme.ink)
                    }

                    if let errorMessage {
                        Text(errorMessage)
                            .font(.subheadline)
                            .foregroundStyle(ReinforceTheme.bad)
                            .padding(12)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(ReinforceTheme.badSoft)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    } else {
                        OutlineNodesListView(
                            nodes: outlineNodes,
                            loadedCards: loadedCards,
                            store: store,
                            l10n: l10n,
                            prefix: "sec",
                            depth: 0,
                            onOpenCard: onOpenCard,
                            onOpenAnchor: { activeAnchor = AnchorSheetItem(id: $0) }
                        )
                    }
                }
                .padding()
                .frame(maxWidth: 820, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .background(ReinforceTheme.canvas)
            .navigationTitle(note.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItemGroup(placement: .topBarTrailing) {
                    if let onRequestExport {
                        Button {
                            onRequestExport(.note(id: note.id, title: note.title))
                        } label: {
                            Label(l10n.t("export.title"), systemImage: "square.and.arrow.up")
                        }
                        .accessibilityIdentifier("note-export-pdf-button")
                    }

                    let sections = collectTOC(nodes: outlineNodes, prefix: "sec", depth: 0)
                    if !sections.isEmpty {
                        Button {
                            showOutlinePopover = true
                        } label: {
                            Label(l10n.t("reading.outline"), systemImage: "list.bullet.indent")
                        }
                        .popover(isPresented: $showOutlinePopover) {
                            NavigationStack {
                                List(sections) { entry in
                                    Button {
                                        showOutlinePopover = false
                                        withAnimation {
                                            proxy.scrollTo(entry.id, anchor: .top)
                                        }
                                    } label: {
                                        Text(entry.heading)
                                            .font(entry.depth == 0 ? .subheadline.weight(.semibold) : .subheadline)
                                            .foregroundStyle(ReinforceTheme.ink)
                                            .padding(.leading, CGFloat(entry.depth * 14))
                                    }
                                }
                                .navigationTitle(l10n.t("reading.outline"))
                                .navigationBarTitleDisplayMode(.inline)
                            }
                            .frame(minWidth: 260, minHeight: 220)
                        }
                    }
                }
            }
            .sheet(item: $activeAnchor) { item in
                AnchorViewerSheet(anchorId: item.id, store: store, l10n: l10n)
            }
            .onAppear { loadNoteData() }
            .onChange(of: note.id) { _, _ in loadNoteData() }
        }
    }

    private func loadNoteData() {
        errorMessage = nil
        do {
            let resolved = try store.loadNoteOutline(noteId: note.id)
            outlineNodes = resolved
            var cache: [String: KnowledgeCard] = [:]
            for cardId in collectCardIds(resolved) {
                if let full = store.loadKnowledgeCard(id: cardId) {
                    cache[cardId] = full
                }
            }
            loadedCards = cache
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func collectCardIds(_ nodes: [NoteOutlineNode]) -> [String] {
        var ids: [String] = []
        for node in nodes {
            switch node {
            case .section(_, _, let children):
                ids.append(contentsOf: collectCardIds(children))
            case .card(let summary):
                ids.append(summary.id)
            }
        }
        return ids
    }

    private struct TOCEntry: Identifiable {
        let id: String
        let heading: String
        let depth: Int
    }

    private func collectTOC(nodes: [NoteOutlineNode], prefix: String, depth: Int) -> [TOCEntry] {
        var list: [TOCEntry] = []
        for (idx, node) in nodes.enumerated() {
            if case .section(let heading, _, let children) = node {
                let id = "\(prefix)-\(idx)"
                list.append(TOCEntry(id: id, heading: heading, depth: depth))
                list.append(contentsOf: collectTOC(nodes: children, prefix: id, depth: depth + 1))
            }
        }
        return list
    }
}

private struct OutlineNodesListView: View {
    let nodes: [NoteOutlineNode]
    let loadedCards: [String: KnowledgeCard]
    let store: AppStore
    let l10n: LocalizationManager
    let prefix: String
    let depth: Int
    let onOpenCard: (String) -> Void
    let onOpenAnchor: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            ForEach(Array(nodes.enumerated()), id: \.offset) { idx, node in
                switch node {
                case .section(let heading, let introMarkdown, let children):
                    let sectionId = "\(prefix)-\(idx)"
                    VStack(alignment: .leading, spacing: 12) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(heading)
                                .font(depth == 0 ? .title2.weight(.bold) : .title3.weight(.semibold))
                                .foregroundStyle(ReinforceTheme.ink)
                            if let intro = introMarkdown, !intro.isEmpty {
                                MarkdownBlockView(text: intro, muted: true)
                            }
                        }
                        .padding(.bottom, 4)
                        .overlay(alignment: .bottom) {
                            Divider().background(ReinforceTheme.border)
                        }

                        OutlineNodesListView(
                            nodes: children,
                            loadedCards: loadedCards,
                            store: store,
                            l10n: l10n,
                            prefix: sectionId,
                            depth: depth + 1,
                            onOpenCard: onOpenCard,
                            onOpenAnchor: onOpenAnchor
                        )
                        .padding(.leading, depth > 0 ? 12 : 0)
                    }
                    .id(sectionId)

                case .card(let summary):
                    if let fullCard = loadedCards[summary.id] {
                        KnowledgeCardView(
                            card: fullCard,
                            store: store,
                            l10n: l10n,
                            showCardLink: true,
                            onOpenCard: onOpenCard,
                            onOpenAnchor: onOpenAnchor
                        )
                    } else {
                        Text(l10n.t("reading.missingCard"))
                            .font(.subheadline)
                            .foregroundStyle(ReinforceTheme.bad)
                            .reinforceCard()
                    }
                }
            }
        }
    }
}
