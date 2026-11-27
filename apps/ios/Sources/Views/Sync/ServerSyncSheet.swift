import SwiftUI

struct ServerSyncSheet: View {
    let store: AppStore
    let l10n: LocalizationManager

    @Environment(\.dismiss) private var dismiss

    @State private var serverURLText: String = "http://127.0.0.1:3000"
    @State private var usernameText: String = ""
    @State private var passwordText: String = ""
    @State private var detectedStatus: AuthStatusResponse?
    @State private var isCheckingServer: Bool = false
    @State private var isAuthenticating: Bool = false
    @State private var authError: String?

    private var sync: SyncManager { store.syncManager }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    if let session = sync.session {
                        signedInDashboard(session: session)
                    } else {
                        signInForm
                    }
                }
                .padding()
                .frame(maxWidth: 640, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .background(ReinforceTheme.canvas)
            .navigationTitle(l10n.t("sync.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(l10n.t("common.done")) {
                        dismiss()
                    }
                    .accessibilityIdentifier("sync-sheet-done-button")
                }
            }
            .onAppear {
                sync.refreshPendingCounts()
                if let savedURL = sync.session?.serverURL {
                    serverURLText = savedURL
                }
            }
        }
    }

    // MARK: - Signed Out Form

    private var signInForm: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 6) {
                Label(l10n.t("sync.connectHeading"), systemImage: "icloud.and.arrow.up")
                    .font(.headline)
                    .foregroundStyle(ReinforceTheme.ink)
                Text(l10n.t("sync.connectSubtitle"))
                    .font(.subheadline)
                    .foregroundStyle(ReinforceTheme.inkMuted)
            }
            .reinforceCard()

            VStack(alignment: .leading, spacing: 14) {
                VStack(alignment: .leading, spacing: 6) {
                    Text(l10n.t("sync.serverURL"))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(ReinforceTheme.inkMuted)

                    HStack(spacing: 8) {
                        TextField("http://127.0.0.1:3000", text: $serverURLText)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .padding(11)
                            .background(ReinforceTheme.canvas)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .stroke(ReinforceTheme.border, lineWidth: 1)
                            )
                            .accessibilityIdentifier("sync-server-url-field")
                            .onChange(of: serverURLText) { _, _ in
                                detectedStatus = nil
                                authError = nil
                            }

                        Button {
                            Task { await checkServer() }
                        } label: {
                            Group {
                                if isCheckingServer {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Text(l10n.t("sync.checkServer"))
                                        .font(.caption.weight(.semibold))
                                }
                            }
                            .foregroundStyle(ReinforceTheme.accent)
                            .padding(.horizontal, 12)
                            .frame(minHeight: ReinforceTheme.minTouchTarget)
                            .background(ReinforceTheme.accentSoft)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                        .buttonStyle(.plain)
                        .disabled(isCheckingServer || serverURLText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityIdentifier("sync-check-server-button")
                    }
                }

                if let detectedStatus {
                    HStack(spacing: 8) {
                        Image(systemName: detectedStatus.authMode == .single ? "checkmark.shield.fill" : "lock.shield.fill")
                            .foregroundStyle(ReinforceTheme.accent)
                        Text(
                            detectedStatus.authMode == .single
                                ? l10n.t("sync.modeSingleHint")
                                : l10n.t("sync.modeInviteHint")
                        )
                        .font(.caption)
                        .foregroundStyle(ReinforceTheme.ink)
                    }
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(ReinforceTheme.accentSoft)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .accessibilityIdentifier("sync-detected-mode-banner")
                }

                if detectedStatus?.authMode != .single {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(l10n.t("sync.username"))
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.inkMuted)
                        TextField(l10n.t("sync.usernamePlaceholder"), text: $usernameText)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .textContentType(.username)
                            .padding(11)
                            .background(ReinforceTheme.canvas)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .stroke(ReinforceTheme.border, lineWidth: 1)
                            )
                            .accessibilityIdentifier("sync-username-field")
                    }

                    VStack(alignment: .leading, spacing: 6) {
                        Text(l10n.t("sync.password"))
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(ReinforceTheme.inkMuted)
                        SecureField(l10n.t("sync.passwordPlaceholder"), text: $passwordText)
                            .textContentType(.password)
                            .padding(11)
                            .background(ReinforceTheme.canvas)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .stroke(ReinforceTheme.border, lineWidth: 1)
                            )
                            .accessibilityIdentifier("sync-password-field")
                    }
                }

                if let authError {
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(ReinforceTheme.bad)
                        Text(authError)
                            .font(.subheadline)
                            .foregroundStyle(ReinforceTheme.bad)
                    }
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(ReinforceTheme.badSoft)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .accessibilityIdentifier("sync-auth-error")
                }

                Button {
                    Task { await signIn() }
                } label: {
                    HStack(spacing: 8) {
                        if isAuthenticating {
                            ProgressView()
                                .tint(.white)
                        } else {
                            Image(systemName: "arrow.triangle.2.circlepath")
                        }
                        Text(
                            detectedStatus?.authMode == .single
                                ? l10n.t("sync.connectAndSync")
                                : l10n.t("sync.signInAndSync")
                        )
                    }
                    .font(.headline)
                    .foregroundStyle(.white)
                    .frame(maxWidth: .infinity)
                    .frame(minHeight: 50)
                    .background(ReinforceTheme.accent)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                }
                .buttonStyle(.plain)
                .disabled(isAuthenticating || serverURLText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .accessibilityIdentifier("sync-sign-in-button")

                Label(l10n.t("sync.keychainNotice"), systemImage: "key.fill")
                    .font(.caption)
                    .foregroundStyle(ReinforceTheme.inkMuted)
            }
            .reinforceCard()

            if sync.totalPendingCount > 0 {
                HStack(spacing: 10) {
                    Image(systemName: "tray.full.fill")
                        .foregroundStyle(ReinforceTheme.accent)
                    Text(l10n.t("sync.localPendingNotice", sync.pendingAttemptCount, sync.pendingMetaCount))
                        .font(.caption)
                        .foregroundStyle(ReinforceTheme.inkMuted)
                }
                .reinforceCard(padding: 12)
            }
        }
    }

    // MARK: - Signed In Dashboard

    private func signedInDashboard(session: StoredServerSession) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            // Connection summary card
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .center) {
                    syncStatusPill
                    Spacer()
                    Label(l10n.t("sync.keychainBadge"), systemImage: "key.fill")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(ReinforceTheme.ok)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(ReinforceTheme.okSoft)
                        .clipShape(Capsule())
                }

                Divider().background(ReinforceTheme.border)

                infoRow(label: l10n.t("sync.serverURL"), value: session.serverURL)
                infoRow(label: l10n.t("sync.account"), value: "\(session.displayAccountName) (\(session.role))")
                infoRow(
                    label: l10n.t("sync.authMode"),
                    value: session.authMode == .single
                        ? l10n.t("sync.authModeSingle")
                        : l10n.t("sync.authModeInvite")
                )
                infoRow(
                    label: l10n.t("sync.networkState"),
                    value: sync.isOnline ? l10n.t("sync.online") : l10n.t("sync.offline")
                )
            }
            .reinforceCard()

            // Queue & history card
            VStack(alignment: .leading, spacing: 12) {
                Text(l10n.t("sync.queueHeading"))
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(ReinforceTheme.ink)

                HStack(spacing: 10) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(sync.pendingAttemptCount)")
                            .font(.title2.monospacedDigit().weight(.bold))
                            .foregroundStyle(sync.pendingAttemptCount > 0 ? ReinforceTheme.warn : ReinforceTheme.ok)
                            .accessibilityIdentifier("sync-pending-attempts-count")
                        Text(l10n.t("sync.pendingAttempts"))
                            .font(.caption)
                            .foregroundStyle(ReinforceTheme.inkMuted)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .background(ReinforceTheme.canvas)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))

                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(sync.pendingMetaCount)")
                            .font(.title2.monospacedDigit().weight(.bold))
                            .foregroundStyle(sync.pendingMetaCount > 0 ? ReinforceTheme.warn : ReinforceTheme.ok)
                            .accessibilityIdentifier("sync-pending-meta-count")
                        Text(l10n.t("sync.pendingMeta"))
                            .font(.caption)
                            .foregroundStyle(ReinforceTheme.inkMuted)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .background(ReinforceTheme.canvas)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }

                if let lastSyncedAt = sync.lastSyncedAt {
                    infoRow(
                        label: l10n.t("sync.lastSynced"),
                        value: lastSyncedAt.formatted(date: .abbreviated, time: .shortened)
                    )
                }

                if let summary = sync.lastSyncSummary {
                    Text(
                        l10n.t(
                            "sync.lastSummary",
                            summary.pushedAttempts,
                            summary.pulledAttempts,
                            summary.importedContentItems
                        )
                    )
                    .font(.caption)
                    .foregroundStyle(ReinforceTheme.inkMuted)
                    .accessibilityIdentifier("sync-last-summary")
                }

                if let error = sync.lastSyncError {
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(ReinforceTheme.bad)
                        Text(error)
                            .font(.caption)
                            .foregroundStyle(ReinforceTheme.bad)
                    }
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(ReinforceTheme.badSoft)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .accessibilityIdentifier("sync-error-banner")
                }
            }
            .reinforceCard()

            // Action buttons
            VStack(spacing: 10) {
                Button {
                    Task { _ = await sync.syncNow(reason: .manual) }
                } label: {
                    HStack(spacing: 8) {
                        if sync.isSyncing {
                            ProgressView()
                                .tint(.white)
                        } else {
                            Image(systemName: "arrow.triangle.2.circlepath")
                        }
                        Text(sync.isSyncing ? l10n.t("sync.status.syncing") : l10n.t("sync.syncNow"))
                    }
                    .font(.headline)
                    .foregroundStyle(.white)
                    .frame(maxWidth: .infinity)
                    .frame(minHeight: 50)
                    .background(ReinforceTheme.accent)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                }
                .buttonStyle(.plain)
                .disabled(sync.isSyncing)
                .accessibilityIdentifier("sync-now-button")

                Button(role: .destructive) {
                    Task {
                        await sync.signOut()
                        passwordText = ""
                        detectedStatus = nil
                        authError = nil
                    }
                } label: {
                    Label(l10n.t("sync.signOut"), systemImage: "rectangle.portrait.and.arrow.right")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(ReinforceTheme.bad)
                        .frame(maxWidth: .infinity)
                        .frame(minHeight: ReinforceTheme.minTouchTarget)
                        .background(ReinforceTheme.badSoft)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("sync-sign-out-button")
            }
        }
    }

    private var syncStatusPill: some View {
        let (icon, title, fg, bg): (String, String, Color, Color) = {
            switch sync.status {
            case .signedOut:
                return ("icloud.slash", l10n.t("sync.status.signedOut"), ReinforceTheme.inkMuted, ReinforceTheme.subtle)
            case .syncing:
                return ("arrow.triangle.2.circlepath", l10n.t("sync.status.syncing"), ReinforceTheme.accent, ReinforceTheme.accentSoft)
            case .idle:
                return ("checkmark.icloud.fill", l10n.t("sync.status.synced"), ReinforceTheme.ok, ReinforceTheme.okSoft)
            case .offlineQueued(let count):
                return ("wifi.slash", l10n.t("sync.status.offlineQueued", count), ReinforceTheme.warn, ReinforceTheme.warnSoft)
            case .error:
                return ("exclamationmark.icloud.fill", l10n.t("sync.status.error"), ReinforceTheme.bad, ReinforceTheme.badSoft)
            case .authExpired:
                return ("lock.slash.fill", l10n.t("sync.status.authExpired"), ReinforceTheme.bad, ReinforceTheme.badSoft)
            }
        }()

        return Label(title, systemImage: icon)
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(fg)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(bg)
            .clipShape(Capsule())
            .accessibilityIdentifier("sync-status-label")
    }

    private func infoRow(label: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(label)
                .font(.caption)
                .foregroundStyle(ReinforceTheme.inkMuted)
            Spacer()
            Text(value)
                .font(.caption.weight(.medium))
                .foregroundStyle(ReinforceTheme.ink)
                .multilineTextAlignment(.trailing)
                .textSelection(.enabled)
        }
    }

    private func checkServer() async {
        authError = nil
        isCheckingServer = true
        defer { isCheckingServer = false }
        do {
            detectedStatus = try await sync.inspectServer(rawURL: serverURLText)
        } catch {
            authError = error.localizedDescription
        }
    }

    private func signIn() async {
        authError = nil
        isAuthenticating = true
        defer { isAuthenticating = false }
        do {
            try await sync.signIn(
                rawURL: serverURLText,
                username: usernameText,
                password: passwordText
            )
            passwordText = ""
        } catch {
            authError = error.localizedDescription
        }
    }
}
