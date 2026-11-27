import SwiftUI

struct ReadingHomeView: View {
    let store: AppStore
    let l10n: LocalizationManager
    let onRequestImport: () -> Void
    let onOpenNote: (Note) -> Void
    let onOpenCard: (String) -> Void

    @State private var searchQuery: String = ""
    @State private var launchError: String?

    private var hasSearch: Bool {
        !searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                if let banner = store.importBannerMessage {
                    HStack(spacing: 10) {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundStyle(ReinforceTheme.ok)
                        Text(banner)
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(ReinforceTheme.ok)
                        Spacer()
                        Button {
                            store.importBannerMessage = nil
                        } label: {
                            Image(systemName: "xmark")
                                .font(.caption.weight(.bold))
                                .foregroundStyle(ReinforceTheme.ok)
                        }
                    }
                    .padding(12)
                    .background(ReinforceTheme.okSoft)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .accessibilityIdentifier("import-success-banner")
                }

                if let error = store.importErrorMessage {
                    HStack(spacing: 10) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(ReinforceTheme.bad)
                        Text(error)
                            .font(.subheadline)
                            .foregroundStyle(ReinforceTheme.bad)
                        Spacer()
                        Button {
                            store.importErrorMessage = nil
                        } label: {
                            Image(systemName: "xmark")
                                .font(.caption.weight(.bold))
                                .foregroundStyle(ReinforceTheme.bad)
                        }
                    }
                    .padding(12)
                    .background(ReinforceTheme.badSoft)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }

                if store.isEmptyLibrary {
                    emptyLibraryCard
                } else {
                    dailyRecallStrip

                    if let launchError {
                        Text(launchError)
                            .font(.subheadline)
                            .foregroundStyle(ReinforceTheme.warn)
                            .padding(12)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(ReinforceTheme.warnSoft)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    }

                    searchAndSubjectBar

                    if hasSearch {
                        searchResultsSection
                    } else {
                        notesSection
                        cardsSection
                    }
                }
            }
            .padding()
            .frame(maxWidth: 960, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .background(ReinforceTheme.canvas)
        .navigationTitle(l10n.t("reading.title"))
    }

    private var emptyLibraryCard: some View {
        VStack(spacing: 16) {
            Image(systemName: "books.vertical.fill")
                .font(.system(size: 42))
                .foregroundStyle(ReinforceTheme.accent)

            Text(l10n.t("import.emptyTitle"))
                .font(.title3.weight(.bold))
                .foregroundStyle(ReinforceTheme.ink)
                .multilineTextAlignment(.center)

            Text(l10n.t("import.emptySubtitle"))
                .font(.subheadline)
                .foregroundStyle(ReinforceTheme.inkMuted)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 460)

            Button {
                onRequestImport()
            } label: {
                Label(l10n.t("import.button"), systemImage: "square.and.arrow.down")
                    .font(.headline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 22)
                    .frame(minHeight: 48)
                    .background(ReinforceTheme.accent)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("empty-import-button")

            Text(l10n.t("import.hintFolder"))
                .font(.caption)
                .foregroundStyle(ReinforceTheme.inkMuted)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 36)
        .padding(.horizontal, 20)
        .reinforceCard()
    }

    private var dailyRecallStrip: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Label(l10n.t("reading.dailyDueBannerTitle"), systemImage: "sparkles")
                        .font(.headline)
                        .foregroundStyle(ReinforceTheme.ink)
                    Text(l10n.t("reading.dailyDueBannerDesc"))
                        .font(.subheadline)
                        .foregroundStyle(ReinforceTheme.inkMuted)
                }
                Spacer()
                VStack(alignment: .trailing, spacing: 4) {
                    Text(l10n.t("reading.dueCount", store.dueQuestions.count))
                        .font(.caption.monospacedDigit().weight(.semibold))
                        .foregroundStyle(ReinforceTheme.accent)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(ReinforceTheme.accentSoft)
                        .clipShape(Capsule())
                        .accessibilityIdentifier("reading-due-count")

                    if store.scheduledCardCount > 0 {
                        Text(l10n.t("reading.reviewedCount", store.scheduledCardCount))
                            .font(.caption2.monospacedDigit().weight(.medium))
                            .foregroundStyle(ReinforceTheme.ok)
                            .accessibilityIdentifier("reading-reviewed-count")
                    }
                }
            }

            HStack(spacing: 10) {
                Button {
                    quickStartDailyDue()
                } label: {
                    Label(l10n.t("reading.startDailyDue"), systemImage: "play.fill")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 14)
                        .frame(minHeight: ReinforceTheme.minTouchTarget)
                        .background(ReinforceTheme.accent)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("quick-start-daily-due")

                Button {
                    quickStartWeakness()
                } label: {
                    Label(l10n.t("reading.startWeakness"), systemImage: "flame.fill")
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(ReinforceTheme.ink)
                        .padding(.horizontal, 14)
                        .frame(minHeight: ReinforceTheme.minTouchTarget)
                        .background(ReinforceTheme.canvas)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 10, style: .continuous)
                                .stroke(ReinforceTheme.border, lineWidth: 1)
                        )
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("quick-start-weakness")
            }
        }
        .reinforceCard()
    }

    private var searchAndSubjectBar: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(ReinforceTheme.inkMuted)
                TextField(l10n.t("common.search"), text: $searchQuery)
                    .textFieldStyle(.plain)
                    .autocorrectionDisabled()
                if hasSearch {
                    Button {
                        searchQuery = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(ReinforceTheme.inkMuted)
                    }
                }
            }
            .padding(.horizontal, 12)
            .frame(minHeight: ReinforceTheme.minTouchTarget)
            .background(ReinforceTheme.surface)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(ReinforceTheme.border, lineWidth: 1)
            )

            if !store.subjects.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        subjectChip(title: l10n.t("common.allSubjects"), selected: store.selectedSubject == nil) {
                            store.selectedSubject = nil
                        }
                        ForEach(store.subjects, id: \.subject) { summary in
                            subjectChip(title: summary.subject, selected: store.selectedSubject == summary.subject) {
                                store.selectedSubject = summary.subject
                            }
                        }
                    }
                }
            }
        }
    }

    private func subjectChip(title: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(selected ? ReinforceTheme.accent : ReinforceTheme.inkMuted)
                .padding(.horizontal, 12)
                .frame(minHeight: 36)
                .background(selected ? ReinforceTheme.accentSoft : ReinforceTheme.surface)
                .clipShape(Capsule())
                .overlay(
                    Capsule().stroke(selected ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                )
        }
        .buttonStyle(.plain)
    }

    private var searchResultsSection: some View {
        let hits = store.searchLibrary(query: searchQuery)
        let matchedNotes = store.notes.filter {
            $0.title.localizedCaseInsensitiveContains(searchQuery) ||
            $0.subject.localizedCaseInsensitiveContains(searchQuery)
        }
        return VStack(alignment: .leading, spacing: 12) {
            Text(l10n.t("reading.searchResultsHeading"))
                .font(.headline)
                .foregroundStyle(ReinforceTheme.ink)

            if matchedNotes.isEmpty && hits.isEmpty {
                Text(l10n.t("reading.noSearchResults"))
                    .font(.subheadline)
                    .foregroundStyle(ReinforceTheme.inkMuted)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 24)
                    .reinforceCard()
            } else {
                VStack(spacing: 8) {
                    ForEach(matchedNotes, id: \.id) { note in
                        Button { onOpenNote(note) } label: {
                            HStack {
                                Text(note.title)
                                    .font(.subheadline.weight(.medium))
                                    .foregroundStyle(ReinforceTheme.ink)
                                Spacer()
                                Text(l10n.t("reading.notesHeading"))
                                    .font(.caption2)
                                    .foregroundStyle(ReinforceTheme.inkMuted)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 3)
                                    .background(ReinforceTheme.subtle)
                                    .clipShape(Capsule())
                            }
                            .padding(12)
                            .reinforceCard(padding: 0)
                        }
                        .buttonStyle(.plain)
                    }

                    ForEach(Array(hits.enumerated()), id: \.offset) { _, hit in
                        switch hit {
                        case .knowledgeCard(let id):
                            if let card = store.loadKnowledgeCard(id: id) {
                                Button { onOpenCard(id) } label: {
                                    HStack {
                                        Text(card.title)
                                            .font(.subheadline.weight(.medium))
                                            .foregroundStyle(ReinforceTheme.ink)
                                        Spacer()
                                        Text(l10n.t("reading.cardsHeading"))
                                            .font(.caption2)
                                            .foregroundStyle(ReinforceTheme.accent)
                                            .padding(.horizontal, 8)
                                            .padding(.vertical, 3)
                                            .background(ReinforceTheme.accentSoft)
                                            .clipShape(Capsule())
                                    }
                                    .padding(12)
                                    .reinforceCard(padding: 0)
                                }
                                .buttonStyle(.plain)
                            }
                        case .questionCard(let id):
                            if let q = try? store.database.questionCard(id: id) {
                                let preview = ContentBlock.decodeArray(from: q.stemBlocksJson)
                                    .map(\.plainSummary)
                                    .joined(separator: " ")
                                HStack {
                                    Text(preview.isEmpty ? id : preview)
                                        .font(.subheadline)
                                        .foregroundStyle(ReinforceTheme.ink)
                                        .lineLimit(2)
                                    Spacer()
                                    Text("Q")
                                        .font(.caption2.weight(.bold))
                                        .foregroundStyle(ReinforceTheme.inkMuted)
                                        .padding(.horizontal, 8)
                                        .padding(.vertical, 3)
                                        .background(ReinforceTheme.subtle)
                                        .clipShape(Capsule())
                                }
                                .padding(12)
                                .reinforceCard(padding: 0)
                            }
                        }
                    }
                }
            }
        }
    }

    private var notesSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label(l10n.t("reading.notesHeading"), systemImage: "book.closed")
                    .font(.headline)
                    .foregroundStyle(ReinforceTheme.ink)
                Spacer()
                Text(l10n.t("reading.subjectCount", store.filteredNotes.count))
                    .font(.caption)
                    .foregroundStyle(ReinforceTheme.inkMuted)
            }

            if store.filteredNotes.isEmpty {
                Text(l10n.t("reading.noNotes"))
                    .font(.subheadline)
                    .foregroundStyle(ReinforceTheme.inkMuted)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 20)
                    .reinforceCard()
            } else {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 260), spacing: 12)], spacing: 12) {
                    ForEach(store.filteredNotes, id: \.id) { note in
                        let cardCount = countCards(in: note.outline)
                        let sections = note.outline.compactMap { node -> String? in
                            if case .section(let heading, _, _) = node { return heading }
                            return nil
                        }.joined(separator: " / ")

                        Button {
                            onOpenNote(note)
                        } label: {
                            VStack(alignment: .leading, spacing: 8) {
                                HStack {
                                    Text(note.subject)
                                        .font(.caption2.weight(.semibold))
                                        .foregroundStyle(ReinforceTheme.inkMuted)
                                        .padding(.horizontal, 8)
                                        .padding(.vertical, 3)
                                        .background(ReinforceTheme.subtle)
                                        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                                    Spacer()
                                    Text(l10n.t("reading.cardCount", cardCount))
                                        .font(.caption2)
                                        .foregroundStyle(ReinforceTheme.inkMuted)
                                }

                                Text(note.title)
                                    .font(.headline)
                                    .foregroundStyle(ReinforceTheme.ink)
                                    .multilineTextAlignment(.leading)

                                if !sections.isEmpty {
                                    Text(sections)
                                        .font(.caption)
                                        .foregroundStyle(ReinforceTheme.inkMuted)
                                        .lineLimit(2)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .reinforceCard()
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("note-row-\(note.title)")
                    }
                }
            }
        }
    }

    private var cardsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label(l10n.t("reading.cardsHeading"), systemImage: "square.stack.3d.up")
                    .font(.headline)
                    .foregroundStyle(ReinforceTheme.ink)
                Spacer()
                Text(l10n.t("reading.subjectCount", store.filteredCards.count))
                    .font(.caption)
                    .foregroundStyle(ReinforceTheme.inkMuted)
            }

            if store.filteredCards.isEmpty {
                Text(l10n.t("reading.noCards"))
                    .font(.subheadline)
                    .foregroundStyle(ReinforceTheme.inkMuted)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 20)
                    .reinforceCard()
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(store.filteredCards.enumerated()), id: \.element.id) { idx, card in
                        Button {
                            onOpenCard(card.id)
                        } label: {
                            HStack(spacing: 12) {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(card.title)
                                        .font(.subheadline.weight(.semibold))
                                        .foregroundStyle(ReinforceTheme.ink)
                                        .multilineTextAlignment(.leading)
                                    HStack(spacing: 6) {
                                        Text(card.subject)
                                            .font(.caption2)
                                            .foregroundStyle(ReinforceTheme.inkMuted)
                                        ForEach(card.topics.prefix(3), id: \.self) { topic in
                                            Text(topic)
                                                .font(.caption2)
                                                .foregroundStyle(ReinforceTheme.inkMuted)
                                                .padding(.horizontal, 6)
                                                .padding(.vertical, 2)
                                                .background(ReinforceTheme.subtle)
                                                .clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
                                        }
                                    }
                                }
                                Spacer()
                                let verified = card.verification == .verified
                                Text(verified ? l10n.t("common.verified") : l10n.t("common.unchecked"))
                                    .font(.caption2.weight(.medium))
                                    .foregroundStyle(verified ? ReinforceTheme.ok : ReinforceTheme.warn)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 3)
                                    .background(verified ? ReinforceTheme.okSoft : ReinforceTheme.warnSoft)
                                    .clipShape(Capsule())
                            }
                            .padding(.horizontal, 14)
                            .padding(.vertical, 12)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("card-row-\(card.title)")

                        if idx < store.filteredCards.count - 1 {
                            Divider().background(ReinforceTheme.border)
                        }
                    }
                }
                .reinforceCard(padding: 0)
            }
        }
    }

    private func countCards(in nodes: [OutlineNode]) -> Int {
        nodes.reduce(0) { total, node in
            switch node {
            case .cardRef: return total + 1
            case .section(_, _, let children): return total + countCards(in: children)
            }
        }
    }

    private func quickStartDailyDue() {
        launchError = nil
        do {
            let launched = try store.startDailyDue()
            if !launched {
                launchError = l10n.t("study.options.emptySessionError")
            }
        } catch {
            launchError = error.localizedDescription
        }
    }

    private func quickStartWeakness() {
        launchError = nil
        do {
            let launched = try store.startWeaknessDrill()
            if !launched {
                launchError = l10n.t("study.options.emptySessionError")
            }
        } catch {
            launchError = error.localizedDescription
        }
    }
}
