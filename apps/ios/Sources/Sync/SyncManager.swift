import Foundation
import Observation
import SwiftUI

enum SyncTriggerReason: String, Sendable {
    case manual
    case signIn
    case foreground
    case localChange
    case networkRestored
    case retryBackoff
}

enum SyncStatus: Equatable, Sendable {
    case signedOut
    case idle
    case syncing
    case offlineQueued(pendingCount: Int)
    case error(message: String)
    case authExpired(message: String)
}

struct SyncRunSummary: Equatable, Sendable {
    let pushedAttempts: Int
    let duplicateAttempts: Int
    let pushedMeta: Int
    let pulledAttempts: Int
    let pulledMeta: Int
    let importedContentItems: Int
    let downloadedBlobs: Int
    let contentUnchanged: Bool
}

@MainActor
@Observable
final class SyncManager {
    static let snapshotHashKey = "reinforce.sync.contentSnapshotSha256"
    static let lastSyncedAtKey = "reinforce.sync.lastSyncedAt"

    let database: LocalDatabase
    let blobsURL: URL
    let localUserId: String
    let apiClient: ReinforceAPIClient
    let credentialStore: CredentialStoreProtocol
    let networkMonitor: NetworkMonitorProtocol
    let userDefaults: UserDefaults
    let retryBaseDelaySeconds: Double

    var onDataMutated: ((Bool) -> Void)?

    private(set) var session: StoredServerSession?
    private(set) var status: SyncStatus = .signedOut
    private(set) var isOnline: Bool
    private(set) var isSyncing: Bool = false
    private(set) var pendingAttemptCount: Int = 0
    private(set) var pendingMetaCount: Int = 0
    private(set) var lastSyncedAt: Date?
    private(set) var lastSyncError: String?
    private(set) var lastSyncSummary: SyncRunSummary?

    private var contentSnapshotSha256: String?
    private var needsFollowUpSync: Bool = false
    private var activeSyncTask: Task<Bool, Never>?
    private var retryTask: Task<Void, Never>?
    private var consecutiveFailures: Int = 0

    var isSignedIn: Bool { session != nil }

    var totalPendingCount: Int { pendingAttemptCount + pendingMetaCount }

    init(
        database: LocalDatabase,
        blobsURL: URL,
        localUserId: String,
        apiClient: ReinforceAPIClient = ReinforceAPIClient(),
        credentialStore: CredentialStoreProtocol = KeychainCredentialStore(),
        networkMonitor: NetworkMonitorProtocol = PathNetworkMonitor(),
        userDefaults: UserDefaults = .standard,
        retryBaseDelaySeconds: Double = 5.0,
        resetState: Bool = false
    ) {
        self.database = database
        self.blobsURL = blobsURL
        self.localUserId = localUserId
        self.apiClient = apiClient
        self.credentialStore = credentialStore
        self.networkMonitor = networkMonitor
        self.userDefaults = userDefaults
        self.retryBaseDelaySeconds = retryBaseDelaySeconds
        self.isOnline = networkMonitor.isOnline

        if resetState {
            try? credentialStore.deleteSession()
            apiClient.clearStoredCookies(for: nil)
            userDefaults.removeObject(forKey: Self.snapshotHashKey)
            userDefaults.removeObject(forKey: Self.lastSyncedAtKey)
        }

        self.contentSnapshotSha256 = userDefaults.string(forKey: Self.snapshotHashKey)
        if let ts = userDefaults.object(forKey: Self.lastSyncedAtKey) as? TimeInterval {
            self.lastSyncedAt = Date(timeIntervalSince1970: ts)
        }

        if let stored = try? credentialStore.loadSession() {
            self.session = stored
            _ = try? database.bindRemoteUser(localUserId: localUserId, remoteUserId: stored.remoteUserId)
            refreshPendingCounts()
            self.status = isOnline ? .idle : .offlineQueued(pendingCount: totalPendingCount)
        } else {
            self.session = nil
            refreshPendingCounts()
            self.status = .signedOut
        }

        networkMonitor.onStatusChange = { [weak self] online in
            if Thread.isMainThread {
                MainActor.assumeIsolated {
                    self?.handleNetworkStatusChange(online)
                }
            } else {
                Task { @MainActor [weak self] in
                    self?.handleNetworkStatusChange(online)
                }
            }
        }
        networkMonitor.start()
    }

    func stopMonitoring() {
        retryTask?.cancel()
        retryTask = nil
        networkMonitor.stop()
    }

    // MARK: - Auth Lifecycle

    func inspectServer(rawURL: String) async throws -> AuthStatusResponse {
        let serverURL = try ReinforceAPIClient.normalizeServerURL(rawURL)
        return try await apiClient.authStatus(serverURL: serverURL)
    }

    func signIn(rawURL: String, username: String, password: String) async throws {
        let serverURL = try ReinforceAPIClient.normalizeServerURL(rawURL)
        let authStatus = try await apiClient.authStatus(serverURL: serverURL)

        let newSession: StoredServerSession
        switch authStatus.authMode {
        case .single:
            let info = try await apiClient.currentSession(serverURL: serverURL)
            newSession = StoredServerSession(
                serverURL: serverURL.absoluteString,
                authMode: .single,
                remoteUserId: info.userId,
                role: info.role,
                username: username.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    ? nil
                    : username.trimmingCharacters(in: .whitespacesAndNewlines),
                sessionCookie: nil
            )
        case .invite:
            let trimmedUser = username.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmedUser.isEmpty, !password.isEmpty else {
                throw ReinforceAPIError.unauthorized("Username and password are required for this server.")
            }
            let loginResponse = try await apiClient.login(
                serverURL: serverURL,
                username: trimmedUser,
                password: password
            )
            newSession = StoredServerSession(
                serverURL: serverURL.absoluteString,
                authMode: .invite,
                remoteUserId: loginResponse.userId,
                role: loginResponse.role,
                username: trimmedUser,
                sessionCookie: loginResponse.sessionCookie
            )
        }

        let previousRemoteId = session?.remoteUserId
        let previousServer = session?.serverURL

        _ = try database.bindRemoteUser(localUserId: localUserId, remoteUserId: newSession.remoteUserId)
        try credentialStore.saveSession(newSession)

        if previousRemoteId != newSession.remoteUserId || previousServer != newSession.serverURL {
            contentSnapshotSha256 = nil
            userDefaults.removeObject(forKey: Self.snapshotHashKey)
        }

        retryTask?.cancel()
        retryTask = nil
        consecutiveFailures = 0
        lastSyncError = nil
        session = newSession
        refreshPendingCounts()
        status = .idle

        _ = await syncNow(reason: .signIn)
    }

    func signOut() async {
        retryTask?.cancel()
        retryTask = nil
        needsFollowUpSync = false
        consecutiveFailures = 0

        let oldSession = session
        if let oldSession, let url = oldSession.normalizedURL {
            await apiClient.logout(serverURL: url, sessionCookie: oldSession.sessionCookie)
            await apiClient.clearStoredCookiesAsync(for: url)
        } else {
            await apiClient.clearStoredCookiesAsync(for: nil)
        }

        try? credentialStore.deleteSession()
        userDefaults.removeObject(forKey: Self.snapshotHashKey)
        userDefaults.removeObject(forKey: Self.lastSyncedAtKey)

        session = nil
        contentSnapshotSha256 = nil
        lastSyncedAt = nil
        lastSyncError = nil
        lastSyncSummary = nil
        status = .signedOut
        refreshPendingCounts()
    }

    // MARK: - Triggers

    func handleScenePhaseChange(_ phase: ScenePhase) {
        guard phase == .active, isSignedIn else { return }
        if case .authExpired = status { return }
        scheduleSync(reason: .foreground)
    }

    func notifyLocalStudyChange() {
        isOnline = networkMonitor.isOnline
        refreshPendingCounts()
        guard isSignedIn else { return }
        if case .authExpired = status { return }
        if !isOnline {
            status = .offlineQueued(pendingCount: totalPendingCount)
            return
        }
        scheduleSync(reason: .localChange)
    }

    func scheduleSync(reason: SyncTriggerReason) {
        guard isSignedIn else { return }
        Task { @MainActor [weak self] in
            guard let self else { return }
            _ = await self.syncNow(reason: reason)
        }
    }

    private func handleNetworkStatusChange(_ online: Bool) {
        let wasOffline = !isOnline
        isOnline = online
        refreshPendingCounts()

        guard isSignedIn else { return }

        if !online {
            retryTask?.cancel()
            retryTask = nil
            if !isSyncing {
                status = .offlineQueued(pendingCount: totalPendingCount)
            }
            return
        }

        if wasOffline {
            if case .authExpired = status { return }
            consecutiveFailures = 0
            scheduleSync(reason: .networkRestored)
        }
    }

    func refreshPendingCounts() {
        if let pending = try? database.pendingSync(localUserId: localUserId, limit: 500) {
            pendingAttemptCount = pending.attempts.count
            pendingMetaCount = pending.userCardMeta.count
        }
    }

    // MARK: - Full Sync Cycle

    @discardableResult
    func syncNow(reason: SyncTriggerReason = .manual) async -> Bool {
        guard let activeSession = session, let serverURL = activeSession.normalizedURL else {
            status = .signedOut
            return false
        }

        if isSyncing {
            needsFollowUpSync = true
            if let existing = activeSyncTask {
                return await existing.value
            }
            return false
        }

        isOnline = networkMonitor.isOnline
        refreshPendingCounts()

        if !isOnline {
            status = .offlineQueued(pendingCount: totalPendingCount)
            return false
        }

        retryTask?.cancel()
        retryTask = nil
        isSyncing = true
        status = .syncing

        let task = Task<Bool, Never> { @MainActor [weak self] in
            guard let self else { return false }
            let succeeded = await self.executeSyncCycle(session: activeSession, serverURL: serverURL)
            self.isSyncing = false
            self.activeSyncTask = nil

            let runFollowUp = self.needsFollowUpSync
            self.needsFollowUpSync = false
            if runFollowUp && succeeded && self.isSignedIn && self.isOnline {
                return await self.syncNow(reason: .localChange)
            }
            return succeeded
        }
        activeSyncTask = task
        return await task.value
    }

    private func executeSyncCycle(session activeSession: StoredServerSession, serverURL: URL) async -> Bool {
        do {
            _ = try database.bindRemoteUser(localUserId: localUserId, remoteUserId: activeSession.remoteUserId)

            var pushedAttempts = 0
            var duplicateAttempts = 0
            var pushedMeta = 0
            var pulledAttempts = 0
            var pulledMeta = 0
            var importedContentItems = 0
            var downloadedBlobs = 0
            var contentUnchanged = true

            // 1. Push pending attempts and personal card metadata in pages
            for _ in 0..<1000 {
                let pending = try database.pendingSync(localUserId: localUserId, limit: 200)
                if pending.attempts.isEmpty && pending.userCardMeta.isEmpty {
                    break
                }

                if !pending.attempts.isEmpty {
                    let pushResult = try await apiClient.pushAttempts(
                        serverURL: serverURL,
                        sessionCookie: activeSession.sessionCookie,
                        attempts: pending.attempts
                    )
                    pushedAttempts += pushResult.accepted
                    duplicateAttempts += pushResult.duplicates
                    let ids = pending.attempts.map(\.id)
                    try database.confirmUpload(localUserId: localUserId, attemptIds: ids, metaStamps: [])
                }

                if !pending.userCardMeta.isEmpty {
                    let metaResult = try await apiClient.pushUserCardMeta(
                        serverURL: serverURL,
                        sessionCookie: activeSession.sessionCookie,
                        rows: pending.userCardMeta
                    )
                    pushedMeta += metaResult.applied
                    let stamps = pending.userCardMeta.map {
                        SyncMetaStamp(cardId: $0.cardId, updatedAt: $0.updatedAt)
                    }
                    try database.confirmUpload(localUserId: localUserId, attemptIds: [], metaStamps: stamps)
                }

                if !pending.hasMoreAttempts && !pending.hasMoreUserCardMeta {
                    break
                }
            }

            // 2. Pull remote attempts in cursor pages
            var cursor = (try database.syncState(localUserId: localUserId))?.pullCursor ?? 0
            for _ in 0..<1000 {
                let page = try await apiClient.pullAttempts(
                    serverURL: serverURL,
                    sessionCookie: activeSession.sessionCookie,
                    cursor: cursor,
                    limit: 200
                )
                if !page.attempts.isEmpty || page.nextCursor > cursor {
                    let applied = try database.applyRemoteBatch(
                        localUserId: localUserId,
                        attempts: page.attempts,
                        userCardMeta: [],
                        nextCursor: page.nextCursor
                    )
                    pulledAttempts += Int(applied.insertedAttempts)
                    let previousCursor = cursor
                    cursor = applied.pullCursor
                    if !page.hasMore || page.attempts.isEmpty || cursor <= previousCursor {
                        break
                    }
                } else {
                    break
                }
            }

            // 3. Pull remote user card metadata
            let remoteMeta = try await apiClient.pullUserCardMeta(
                serverURL: serverURL,
                sessionCookie: activeSession.sessionCookie
            )
            if !remoteMeta.isEmpty {
                let currentCursor = (try database.syncState(localUserId: localUserId))?.pullCursor ?? cursor
                let applied = try database.applyRemoteBatch(
                    localUserId: localUserId,
                    attempts: [],
                    userCardMeta: remoteMeta,
                    nextCursor: currentCursor
                )
                pulledMeta += Int(applied.appliedUserCardMeta)
            }

            // 4. Pull content snapshot (short-circuited via known sha256 when library is already populated)
            let localNotesEmpty = ((try? database.notes(subject: nil, limit: 1)) ?? []).isEmpty
            let localCardsEmpty = ((try? database.knowledgeCards(subject: nil, limit: 1)) ?? []).isEmpty
            let knownHash = (localNotesEmpty && localCardsEmpty) ? nil : contentSnapshotSha256

            let snapshot = try await apiClient.fetchContentSnapshot(
                serverURL: serverURL,
                sessionCookie: activeSession.sessionCookie,
                knownSha256: knownHash
            )
            contentUnchanged = snapshot.unchanged

            if !snapshot.unchanged {
                let bundle = snapshot.bundleJsonl ?? ""
                if !bundle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    var seenMissingHashes = Set<String>()
                    let missingAssets = (snapshot.assets ?? []).filter { asset in
                        guard seenMissingHashes.insert(asset.sha256).inserted else { return false }
                        let localBlob = blobsURL.appendingPathComponent(asset.sha256)
                        return !FileManager.default.fileExists(atPath: localBlob.path)
                    }

                    var stagingDir: URL?
                    if !missingAssets.isEmpty {
                        let tempAssetsDir = FileManager.default.temporaryDirectory
                            .appendingPathComponent("reinforce-sync-assets-\(UUID().uuidString)", isDirectory: true)
                        try FileManager.default.createDirectory(at: tempAssetsDir, withIntermediateDirectories: true)
                        stagingDir = tempAssetsDir
                    }
                    defer {
                        if let stagingDir {
                            try? FileManager.default.removeItem(at: stagingDir)
                        }
                    }

                    if let stagingDir {
                        for asset in missingAssets {
                            let blobBytes = try await apiClient.downloadResourceBlob(
                                serverURL: serverURL,
                                sessionCookie: activeSession.sessionCookie,
                                resourceId: asset.resourceId
                            )
                            let targetFile = stagingDir.appendingPathComponent(asset.sha256)
                            try blobBytes.write(to: targetFile, options: .atomic)
                            downloadedBlobs += 1
                        }
                    }

                    let importSummary = try database.importContentSnapshot(
                        bundleJsonl: bundle,
                        assetsDir: stagingDir?.path
                    )
                    importedContentItems = Int(importSummary.applied)
                }

                contentSnapshotSha256 = snapshot.snapshotSha256
                userDefaults.set(snapshot.snapshotSha256, forKey: Self.snapshotHashKey)
            }

            // Guard against logout race while async network calls were in flight
            guard session?.remoteUserId == activeSession.remoteUserId else {
                return false
            }

            refreshPendingCounts()
            let now = Date()
            lastSyncedAt = now
            userDefaults.set(now.timeIntervalSince1970, forKey: Self.lastSyncedAtKey)
            lastSyncError = nil
            consecutiveFailures = 0
            lastSyncSummary = SyncRunSummary(
                pushedAttempts: pushedAttempts,
                duplicateAttempts: duplicateAttempts,
                pushedMeta: pushedMeta,
                pulledAttempts: pulledAttempts,
                pulledMeta: pulledMeta,
                importedContentItems: importedContentItems,
                downloadedBlobs: downloadedBlobs,
                contentUnchanged: contentUnchanged
            )
            status = isOnline ? .idle : .offlineQueued(pendingCount: totalPendingCount)

            let mediaUpdated = downloadedBlobs > 0
            if pulledAttempts > 0 || pulledMeta > 0 || importedContentItems > 0 || mediaUpdated || pushedAttempts > 0 {
                onDataMutated?(mediaUpdated)
            }
            return true
        } catch let apiError as ReinforceAPIError {
            handleSyncError(apiError)
            return false
        } catch {
            handleSyncError(.networkError(error.localizedDescription))
            return false
        }
    }

    private func handleSyncError(_ error: ReinforceAPIError) {
        refreshPendingCounts()
        let message = error.localizedDescription

        guard isSignedIn else {
            status = .signedOut
            return
        }

        if error.isUnauthorized {
            lastSyncError = message
            status = .authExpired(message: message)
            return
        }

        if !isOnline || error == .offline {
            lastSyncError = message
            status = .offlineQueued(pendingCount: totalPendingCount)
            return
        }

        lastSyncError = message
        status = .error(message: message)

        if error.isRetryableNetworkFailure && retryBaseDelaySeconds > 0 {
            scheduleBackoffRetry()
        }
    }

    private func scheduleBackoffRetry() {
        retryTask?.cancel()
        consecutiveFailures = min(consecutiveFailures + 1, 5)
        let delay = retryBaseDelaySeconds * pow(2.0, Double(consecutiveFailures - 1))
        retryTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(delay))
            guard !Task.isCancelled, let self, self.isSignedIn, self.isOnline else { return }
            _ = await self.syncNow(reason: .retryBackoff)
        }
    }

    // MARK: - Server PDF Export

    func exportPDF(preset: PDFExportPreset, scope: PDFExportScope) async throws -> URL {
        guard let activeSession = session, let serverURL = activeSession.normalizedURL else {
            throw ReinforceAPIError.unauthorized("Sign in to a Reinforce server to export PDFs.")
        }
        if !isOnline {
            throw ReinforceAPIError.offline
        }

        let exported = try await apiClient.exportPDF(
            serverURL: serverURL,
            sessionCookie: activeSession.sessionCookie,
            preset: preset,
            scope: scope
        )

        let exportDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("ReinforceExports", isDirectory: true)
        try FileManager.default.createDirectory(at: exportDir, withIntermediateDirectories: true)

        let safeBaseName = sanitizeFilename(exported.suggestedFilename, fallbackPreset: preset)
        let targetURL = exportDir.appendingPathComponent(safeBaseName)
        if FileManager.default.fileExists(atPath: targetURL.path) {
            try? FileManager.default.removeItem(at: targetURL)
        }
        try exported.data.write(to: targetURL, options: .atomic)
        return targetURL
    }

    private func sanitizeFilename(_ name: String, fallbackPreset: PDFExportPreset) -> String {
        let base = URL(fileURLWithPath: name).lastPathComponent
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: ":", with: "-")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if base.isEmpty {
            return "reinforce-\(fallbackPreset.rawValue).pdf"
        }
        return base.lowercased().hasSuffix(".pdf") ? base : "\(base).pdf"
    }
}
