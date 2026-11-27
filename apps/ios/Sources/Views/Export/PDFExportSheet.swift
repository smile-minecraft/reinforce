import SwiftUI
import UIKit

enum PDFExportContext: Identifiable, Equatable {
    case note(id: String, title: String)
    case subject(name: String)
    case weakness(defaultThreshold: Double)

    var id: String {
        switch self {
        case .note(let id, _): return "note-\(id)"
        case .subject(let name): return "subject-\(name)"
        case .weakness: return "weakness"
        }
    }
}

private struct ShareFileItem: Identifiable {
    let id = UUID()
    let url: URL
}

struct ShareActivityView: UIViewControllerRepresentable {
    let activityItems: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: activityItems, applicationActivities: nil)
    }

    func updateUIViewController(_ uiViewController: UIActivityViewController, context: Context) {}
}

struct PDFExportSheet: View {
    let context: PDFExportContext
    let store: AppStore
    let l10n: LocalizationManager
    let onRequestServerSetup: () -> Void

    @Environment(\.dismiss) private var dismiss

    @State private var selectedPreset: PDFExportPreset = .goodnotes
    @State private var weaknessThreshold: Double = 0.5
    @State private var isExporting: Bool = false
    @State private var exportError: String?
    @State private var exportedURL: URL?
    @State private var exportedByteSize: Int?
    @State private var shareItem: ShareFileItem?

    init(
        context: PDFExportContext,
        store: AppStore,
        l10n: LocalizationManager,
        onRequestServerSetup: @escaping () -> Void
    ) {
        self.context = context
        self.store = store
        self.l10n = l10n
        self.onRequestServerSetup = onRequestServerSetup
        if case .weakness(let threshold) = context {
            self._weaknessThreshold = State(initialValue: threshold)
        }
    }

    private var sync: SyncManager { store.syncManager }

    private var scopeTitle: String {
        switch context {
        case .note(_, let title):
            return l10n.t("export.scope.note", title)
        case .subject(let name):
            return l10n.t("export.scope.subject", name)
        case .weakness:
            return l10n.t("export.scope.weakness")
        }
    }

    private var exportScope: PDFExportScope {
        switch context {
        case .note(let id, _):
            return .note(noteId: id)
        case .subject(let name):
            return .subject(subject: name)
        case .weakness:
            return .weakness(threshold: weaknessThreshold)
        }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    // Scope card
                    VStack(alignment: .leading, spacing: 8) {
                        Label(l10n.t("export.scopeHeading"), systemImage: "doc.richtext")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.inkMuted)
                        Text(scopeTitle)
                            .font(.headline)
                            .foregroundStyle(ReinforceTheme.ink)
                        Text(l10n.t("export.deepLinkHint"))
                            .font(.caption)
                            .foregroundStyle(ReinforceTheme.inkMuted)

                        if case .weakness = context {
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    Text(l10n.t("export.weaknessThreshold"))
                                        .font(.caption.weight(.medium))
                                        .foregroundStyle(ReinforceTheme.ink)
                                    Spacer()
                                    Text(String(format: "%.0f%%", weaknessThreshold * 100))
                                        .font(.caption.monospacedDigit().weight(.semibold))
                                        .foregroundStyle(ReinforceTheme.accent)
                                }
                                Slider(value: $weaknessThreshold, in: 0.1...1.0, step: 0.05)
                                    .tint(ReinforceTheme.accent)
                            }
                            .padding(.top, 6)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .reinforceCard()

                    // Preset selector
                    VStack(alignment: .leading, spacing: 10) {
                        Text(l10n.t("export.presetHeading"))
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.ink)

                        ForEach(PDFExportPreset.allCases) { preset in
                            presetRow(preset)
                        }
                    }
                    .reinforceCard()

                    if !sync.isSignedIn {
                        VStack(alignment: .leading, spacing: 10) {
                            Label(l10n.t("export.serverRequiredTitle"), systemImage: "icloud.slash")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(ReinforceTheme.warn)
                            Text(l10n.t("export.serverRequiredBody"))
                                .font(.caption)
                                .foregroundStyle(ReinforceTheme.inkMuted)
                            Button {
                                dismiss()
                                onRequestServerSetup()
                            } label: {
                                Label(l10n.t("sync.connectHeading"), systemImage: "link")
                                    .font(.subheadline.weight(.semibold))
                                    .foregroundStyle(ReinforceTheme.accent)
                                    .padding(.horizontal, 14)
                                    .frame(minHeight: ReinforceTheme.minTouchTarget)
                                    .background(ReinforceTheme.accentSoft)
                                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("export-open-sync-button")
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .reinforceCard()
                    }

                    if let exportError {
                        HStack(alignment: .top, spacing: 8) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .foregroundStyle(ReinforceTheme.bad)
                            Text(exportError)
                                .font(.subheadline)
                                .foregroundStyle(ReinforceTheme.bad)
                        }
                        .padding(12)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(ReinforceTheme.badSoft)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .accessibilityIdentifier("export-error-banner")
                    }

                    if let exportedURL {
                        HStack(spacing: 10) {
                            Image(systemName: "checkmark.circle.fill")
                                .foregroundStyle(ReinforceTheme.ok)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(exportedURL.lastPathComponent)
                                    .font(.subheadline.weight(.semibold))
                                    .foregroundStyle(ReinforceTheme.ink)
                                    .lineLimit(1)
                                if let size = exportedByteSize {
                                    Text(ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file))
                                        .font(.caption)
                                        .foregroundStyle(ReinforceTheme.inkMuted)
                                }
                            }
                            Spacer()
                            Button {
                                shareItem = ShareFileItem(url: exportedURL)
                            } label: {
                                Label(l10n.t("export.shareAgain"), systemImage: "square.and.arrow.up")
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(ReinforceTheme.accent)
                                    .padding(.horizontal, 10)
                                    .frame(minHeight: 36)
                                    .background(ReinforceTheme.accentSoft)
                                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("export-share-again-button")
                        }
                        .padding(12)
                        .background(ReinforceTheme.okSoft)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .accessibilityIdentifier("export-ready-banner")
                    }

                    Button {
                        Task { await generateAndShare() }
                    } label: {
                        HStack(spacing: 8) {
                            if isExporting {
                                ProgressView()
                                    .tint(.white)
                            } else {
                                Image(systemName: "square.and.arrow.up")
                            }
                            Text(isExporting ? l10n.t("export.generating") : l10n.t("export.generateAndShare"))
                        }
                        .font(.headline)
                        .foregroundStyle(.white)
                        .frame(maxWidth: .infinity)
                        .frame(minHeight: 50)
                        .background(sync.isSignedIn ? ReinforceTheme.accent : ReinforceTheme.inkMuted)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                    .buttonStyle(.plain)
                    .disabled(isExporting || !sync.isSignedIn)
                    .accessibilityIdentifier("export-generate-button")
                }
                .padding()
                .frame(maxWidth: 640, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .background(ReinforceTheme.canvas)
            .navigationTitle(l10n.t("export.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(l10n.t("common.close")) {
                        dismiss()
                    }
                    .accessibilityIdentifier("export-sheet-close-button")
                }
            }
            .sheet(item: $shareItem) { item in
                ShareActivityView(activityItems: [item.url])
                    .presentationDetents([.medium, .large])
            }
        }
    }

    private func presetRow(_ preset: PDFExportPreset) -> some View {
        let active = selectedPreset == preset
        let icon: String = {
            switch preset {
            case .standard: return "doc.text"
            case .goodnotes: return "pencil.and.outline"
            case .worksheet: return "checklist.unchecked"
            }
        }()

        return Button {
            selectedPreset = preset
            exportError = nil
        } label: {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: icon)
                    .font(.title3)
                    .foregroundStyle(active ? ReinforceTheme.accent : ReinforceTheme.inkMuted)
                    .frame(width: 28)

                VStack(alignment: .leading, spacing: 3) {
                    Text(l10n.t("export.preset.\(preset.rawValue).name"))
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(active ? ReinforceTheme.accent : ReinforceTheme.ink)
                    Text(l10n.t("export.preset.\(preset.rawValue).desc"))
                        .font(.caption)
                        .foregroundStyle(ReinforceTheme.inkMuted)
                        .multilineTextAlignment(.leading)
                }

                Spacer()

                Image(systemName: active ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(active ? ReinforceTheme.accent : ReinforceTheme.border)
            }
            .padding(12)
            .background(active ? ReinforceTheme.accentSoft : ReinforceTheme.canvas)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(active ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: active ? 1.5 : 1)
            )
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("export-preset-\(preset.rawValue)")
    }

    private func generateAndShare() async {
        exportError = nil
        isExporting = true
        defer { isExporting = false }

        do {
            let fileURL = try await sync.exportPDF(preset: selectedPreset, scope: exportScope)
            exportedURL = fileURL
            exportedByteSize = (try? FileManager.default.attributesOfItem(atPath: fileURL.path)[.size] as? Int)
            shareItem = ShareFileItem(url: fileURL)
        } catch {
            exportError = error.localizedDescription
        }
    }
}
