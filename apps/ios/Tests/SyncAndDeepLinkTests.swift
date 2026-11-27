import Foundation
import XCTest
@testable import Reinforce

// MARK: - Test Doubles

private final class InMemoryCredentialStore: CredentialStoreProtocol, @unchecked Sendable {
    private let lock = NSLock()
    private var stored: StoredServerSession?

    func loadSession() throws -> StoredServerSession? {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }

    func saveSession(_ session: StoredServerSession) throws {
        lock.lock()
        defer { lock.unlock() }
        stored = session
    }

    func deleteSession() throws {
        lock.lock()
        defer { lock.unlock() }
        stored = nil
    }
}

private final class MockNetworkMonitor: NetworkMonitorProtocol {
    var isOnline: Bool
    var onStatusChange: ((Bool) -> Void)?

    init(isOnline: Bool = true) {
        self.isOnline = isOnline
    }

    func start() {}
    func stop() {}

    func setOnline(_ online: Bool) {
        guard isOnline != online else { return }
        isOnline = online
        onStatusChange?(online)
    }
}

private final class StubURLProtocol: URLProtocol {
    typealias Handler = (URLRequest) throws -> (HTTPURLResponse, Data)

    private static let lock = NSLock()
    private static var _handler: Handler?
    private static var _recordedRequests: [URLRequest] = []

    static func configure(handler: @escaping Handler) {
        lock.lock()
        defer { lock.unlock() }
        _handler = handler
        _recordedRequests = []
    }

    static func recordedRequests() -> [URLRequest] {
        lock.lock()
        defer { lock.unlock() }
        return _recordedRequests
    }

    static func reset() {
        lock.lock()
        defer { lock.unlock() }
        _handler = nil
        _recordedRequests = []
    }

    override class func canInit(with request: URLRequest) -> Bool {
        true
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    override func startLoading() {
        Self.lock.lock()
        let handler = Self._handler
        Self._recordedRequests.append(request)
        Self.lock.unlock()

        guard let handler else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse))
            return
        }

        do {
            let (response, data) = try handler(request)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } catch {
            client?.urlProtocol(self, didFailWithError: error)
        }
    }

    override func stopLoading() {}
}

private extension URLRequest {
    func readBodyData() -> Data? {
        if let httpBody { return httpBody }
        guard let stream = httpBodyStream else { return nil }
        stream.open()
        defer { stream.close() }
        var data = Data()
        let bufferSize = 4096
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
        defer { buffer.deallocate() }
        while stream.hasBytesAvailable {
            let read = stream.read(buffer, maxLength: bufferSize)
            if read > 0 {
                data.append(buffer, count: read)
            } else {
                break
            }
        }
        return data
    }
}

// MARK: - Tests

final class SyncAndDeepLinkTests: XCTestCase {
    private var tempDir: URL!
    private var defaultsSuiteName: String!
    private var testDefaults: UserDefaults!

    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("reinforce-sync-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defaultsSuiteName = "app.reinforce.tests.\(UUID().uuidString)"
        testDefaults = UserDefaults(suiteName: defaultsSuiteName)!
        StubURLProtocol.reset()
    }

    override func tearDownWithError() throws {
        StubURLProtocol.reset()
        if let suite = defaultsSuiteName {
            UserDefaults.standard.removePersistentDomain(forName: suite)
        }
        if let tempDir {
            try? FileManager.default.removeItem(at: tempDir)
        }
    }

    private func makeStubClient() -> ReinforceAPIClient {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [StubURLProtocol.self]
        return ReinforceAPIClient(configuration: config)
    }

    private func makeResponse(
        url: URL,
        status: Int = 200,
        headers: [String: String] = ["Content-Type": "application/json"]
    ) -> HTTPURLResponse {
        HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers)!
    }

    private func fixtureURL(_ name: String) throws -> URL {
        let bundle = Bundle(for: Self.self)
        return try XCTUnwrap(
            bundle.url(forResource: name, withExtension: nil),
            "Missing test fixture folder: \(name)"
        )
    }

    private func makeDemoDatabase() throws -> (LocalDatabase, URL, String) {
        let dbURL = tempDir.appendingPathComponent("reinforce.db")
        let blobsURL = tempDir.appendingPathComponent("blobs", isDirectory: true)
        let db = try LocalDatabase.openWithBlobs(databasePath: dbURL.path, blobsDir: blobsURL.path)
        _ = try db.importBundle(bundlePath: try fixtureURL("demo").path)
        let userId = try db.createUser(displayName: "Sync Tester")
        return (db, blobsURL, userId)
    }

    // MARK: - 1. Deep Link Parsing

    func testDeepLinkParserCardQuestionNoteAndInvalidURLs() throws {
        let cardId = "01a21714-c98d-716e-9b53-ead7c8ee810f"
        let questionId = "01a21714-c98d-70f7-90ba-1bced11dd072"
        let noteId = "01a21714-c98d-769f-be7a-6a1d5cd348bd"

        XCTAssertEqual(
            DeepLinkParser.parse(try XCTUnwrap(URL(string: "reinforce://card/\(cardId)"))),
            .card(cardId)
        )
        XCTAssertEqual(
            DeepLinkParser.parse(try XCTUnwrap(URL(string: "REINFORCE://cards/\(cardId)/"))),
            .card(cardId)
        )
        XCTAssertEqual(
            DeepLinkParser.parse(try XCTUnwrap(URL(string: "reinforce:///card/\(cardId)"))),
            .card(cardId)
        )
        XCTAssertEqual(
            DeepLinkParser.parse(try XCTUnwrap(URL(string: "reinforce://question/\(questionId)"))),
            .question(questionId)
        )
        XCTAssertEqual(
            DeepLinkParser.parse(try XCTUnwrap(URL(string: "reinforce://note/\(noteId)"))),
            .note(noteId)
        )

        // Invalid URLs must return nil
        XCTAssertNil(DeepLinkParser.parse(try XCTUnwrap(URL(string: "https://example.com/card/\(cardId)"))))
        XCTAssertNil(DeepLinkParser.parse(try XCTUnwrap(URL(string: "reinforce://card/"))))
        XCTAssertNil(DeepLinkParser.parse(try XCTUnwrap(URL(string: "reinforce://unknown/\(cardId)"))))
        XCTAssertNil(DeepLinkParser.parse(try XCTUnwrap(URL(string: "reinforce://card/\(cardId)/extra"))))
    }

    // MARK: - 2. Keychain Persistence & Deletion

    func testKeychainCredentialStoreSaveLoadUpdateAndDelete() throws {
        let service = "app.reinforce.ios.test.\(UUID().uuidString)"
        let store = KeychainCredentialStore(service: service, account: "session")
        defer { try? store.deleteSession() }

        XCTAssertNil(try store.loadSession())

        let initial = StoredServerSession(
            serverURL: "http://127.0.0.1:3000",
            authMode: .invite,
            remoteUserId: "01a21714-c98d-7931-a48a-6a1d5cd348bd",
            role: "learner",
            username: "alice",
            sessionCookie: "secret-cookie-token-1"
        )
        try store.saveSession(initial)
        XCTAssertEqual(try store.loadSession(), initial)

        let updated = StoredServerSession(
            serverURL: "https://study.example.com",
            authMode: .invite,
            remoteUserId: "01a21714-c98d-7931-a48a-6a1d5cd348bd",
            role: "owner",
            username: "alice",
            sessionCookie: "secret-cookie-token-2"
        )
        try store.saveSession(updated)
        XCTAssertEqual(try store.loadSession(), updated)

        try store.deleteSession()
        XCTAssertNil(try store.loadSession())

        // Second delete must succeed idempotently
        XCTAssertNoThrow(try store.deleteSession())
    }

    // MARK: - 3. Wire Codec Verbatim Payload Preservation

    func testWireCodecPreservesSubmittedPayloadVerbatim() throws {
        let record = AttemptLogRecord(
            id: "01a21714-c98d-7000-8000-000000000001",
            userId: "01a21714-c98d-7931-a48a-6a1d5cd348bd",
            cardId: "01a21714-c98d-70f7-90ba-1bced11dd072",
            direction: .default,
            sessionMode: .practicalExam,
            submittedPayloadJson: #"{"clicks":[{"no":0,"point":{"x":0,"y":0.5}}]}"#,
            isCorrect: true,
            durationMs: 1200,
            confidence: .certain,
            errorTags: [],
            createdAt: "2026-01-02T10:00:00.000000Z",
            syncCursor: 1,
            affectsScheduling: true
        )

        let encoded = try SyncWireCodec.encodePushAttemptsBody([record])
        let encodedText = try XCTUnwrap(String(data: encoded, encoding: .utf8))
        XCTAssertTrue(
            encodedText.contains(#""submitted_payload":{"clicks":[{"no":0,"point":{"x":0,"y":0.5}}]}"#),
            "submitted_payload must be spliced verbatim without mutating integer 0 to 0.0: \(encodedText)"
        )

        let pullWire = """
        {
          "attempts": [
            {
              "id": "\(record.id)",
              "user_id": "\(record.userId)",
              "card_id": "\(record.cardId)",
              "direction": "default",
              "session_mode": "practical_exam",
              "submitted_payload": {"clicks":[{"no":0,"point":{"x":0,"y":0.5}}]},
              "is_correct": true,
              "duration_ms": 1200,
              "confidence": "certain",
              "error_tags": [],
              "created_at": "2026-01-02T10:00:00.000000Z",
              "sync_cursor": 42,
              "affects_scheduling": true
            }
          ],
          "next_cursor": 42,
          "has_more": false
        }
        """
        let decoded = try SyncWireCodec.decodePullAttemptsResponse(from: Data(pullWire.utf8))
        XCTAssertEqual(decoded.nextCursor, 42)
        XCTAssertFalse(decoded.hasMore)
        XCTAssertEqual(decoded.attempts.count, 1)
        XCTAssertEqual(decoded.attempts[0].submittedPayloadJson, #"{"clicks":[{"no":0,"point":{"x":0,"y":0.5}}]}"#)
    }

    // MARK: - 4. Invite Login, Full Sync, Cookie-Only Auth, & Logout Cleanup

    @MainActor
    func testInviteLoginSyncAndLogoutClearsKeychainAndCookies() async throws {
        let (db, blobsURL, localUserId) = try makeDemoDatabase()
        let remoteUserId = "01a21714-c98d-7931-a48a-6a1d5cd348bd"
        let questionId = "01a21714-c98d-70f7-90ba-1bced11dd072"
        let cardId = "01a21714-c98d-716e-9b53-ead7c8ee810f"

        // Create an offline attempt + card meta before signing in
        let attemptId = newAttemptId()
        _ = try db.appendAttempt(record: AttemptLogRecord(
            id: attemptId,
            userId: localUserId,
            cardId: questionId,
            direction: .default,
            sessionMode: .dailyDue,
            submittedPayloadJson: #"{"keys":["A"]}"#,
            isCorrect: true,
            durationMs: 1400,
            confidence: .certain,
            errorTags: [],
            createdAt: "2026-01-02T10:00:00.000000Z",
            syncCursor: try db.nextAttemptCursor(),
            affectsScheduling: true
        ))
        _ = try db.saveUserCardMeta(
            userId: localUserId,
            cardId: cardId,
            starred: true,
            personalTags: ["exam"],
            personalNote: "Remember triple point"
        )

        let remoteAttemptId = newAttemptId()
        var logoutCalled = false
        var contentCallCount = 0
        var requestErrors: [String] = []

        StubURLProtocol.configure { [self] request in
            let url = request.url!
            let path = url.path

            if request.value(forHTTPHeaderField: "Authorization") != nil {
                requestErrors.append("Unexpected Authorization header on \(path)")
            }

            switch (request.httpMethod, path) {
            case ("GET", "/api/auth/status"):
                let json = #"{"auth_mode":"invite","needs_owner_setup":false,"authenticated":false,"role":null}"#
                return (makeResponse(url: url), Data(json.utf8))

            case ("POST", "/api/auth/login"):
                let headers = [
                    "Content-Type": "application/json",
                    "Set-Cookie": "reinforce_session=tok-abc-123; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800"
                ]
                let json = #"{"user_id":"\#(remoteUserId)","role":"learner"}"#
                return (makeResponse(url: url, headers: headers), Data(json.utf8))

            case ("POST", "/api/sync/attempts"):
                if request.value(forHTTPHeaderField: "Cookie") != "reinforce_session=tok-abc-123" {
                    requestErrors.append("Wrong cookie on POST /api/sync/attempts: \(request.value(forHTTPHeaderField: "Cookie") ?? "nil")")
                }
                let body = request.readBodyData() ?? Data()
                let text = String(data: body, encoding: .utf8) ?? ""
                if !text.contains(remoteUserId) {
                    requestErrors.append("Missing remoteUserId in push attempts body")
                }
                if !text.contains(attemptId) {
                    requestErrors.append("Missing attemptId in push attempts body")
                }
                let json = #"{"accepted":1,"duplicates":0,"cursor":10}"#
                return (makeResponse(url: url), Data(json.utf8))

            case ("PUT", "/api/sync/user-card-meta"):
                if request.value(forHTTPHeaderField: "Cookie") != "reinforce_session=tok-abc-123" {
                    requestErrors.append("Wrong cookie on PUT /api/sync/user-card-meta")
                }
                let json = #"{"applied":1,"kept":0}"#
                return (makeResponse(url: url), Data(json.utf8))

            case ("GET", "/api/sync/attempts"):
                if request.value(forHTTPHeaderField: "Cookie") != "reinforce_session=tok-abc-123" {
                    requestErrors.append("Wrong cookie on GET /api/sync/attempts")
                }
                let comps = URLComponents(url: url, resolvingAgainstBaseURL: false)
                let cursor = Int64(comps?.queryItems?.first(where: { $0.name == "cursor" })?.value ?? "0") ?? 0
                if cursor == 0 {
                    let json = """
                    {
                      "attempts": [{
                        "id": "\(remoteAttemptId)",
                        "user_id": "\(remoteUserId)",
                        "card_id": "\(questionId)",
                        "direction": "default",
                        "session_mode": "daily_due",
                        "submitted_payload": {"keys":["B"]},
                        "is_correct": false,
                        "duration_ms": 2100,
                        "confidence": "guessed",
                        "error_tags": ["concept_gap"],
                        "created_at": "2026-01-02T10:05:00.000000Z",
                        "sync_cursor": 11,
                        "affects_scheduling": true
                      }],
                      "next_cursor": 11,
                      "has_more": false
                    }
                    """
                    return (makeResponse(url: url), Data(json.utf8))
                } else {
                    let json = #"{"attempts":[],"next_cursor":\#(cursor),"has_more":false}"#
                    return (makeResponse(url: url), Data(json.utf8))
                }

            case ("GET", "/api/sync/user-card-meta"):
                if request.value(forHTTPHeaderField: "Cookie") != "reinforce_session=tok-abc-123" {
                    requestErrors.append("Wrong cookie on GET /api/sync/user-card-meta")
                }
                return (makeResponse(url: url), Data(#"{"user_card_meta":[]}"#.utf8))

            case ("GET", "/api/sync/content"):
                if request.value(forHTTPHeaderField: "Cookie") != "reinforce_session=tok-abc-123" {
                    requestErrors.append("Wrong cookie on GET /api/sync/content")
                }
                contentCallCount += 1
                let comps = URLComponents(url: url, resolvingAgainstBaseURL: false)
                let known = comps?.queryItems?.first(where: { $0.name == "known" })?.value
                if known == "sha256-demo-snap" {
                    return (makeResponse(url: url), Data(#"{"unchanged":true,"snapshot_sha256":"sha256-demo-snap"}"#.utf8))
                }
                return (makeResponse(url: url), Data(#"{"unchanged":false,"snapshot_sha256":"sha256-demo-snap","bundle_jsonl":"","assets":[]}"#.utf8))

            case ("POST", "/api/auth/logout"):
                logoutCalled = true
                return (makeResponse(url: url), Data(#"{"ok":true}"#.utf8))

            default:
                requestErrors.append("Unexpected request: \(request.httpMethod ?? "?") \(url.absoluteString)")
                return (makeResponse(url: url, status: 404), Data())
            }
        }

        let credStore = InMemoryCredentialStore()
        let netMonitor = MockNetworkMonitor(isOnline: true)
        let apiClient = makeStubClient()
        let manager = SyncManager(
            database: db,
            blobsURL: blobsURL,
            localUserId: localUserId,
            apiClient: apiClient,
            credentialStore: credStore,
            networkMonitor: netMonitor,
            userDefaults: testDefaults,
            retryBaseDelaySeconds: 0
        )

        try await manager.signIn(rawURL: "http://127.0.0.1:3000/", username: "learner", password: "password123")

        XCTAssertTrue(requestErrors.isEmpty, "Request errors: \(requestErrors)")
        XCTAssertTrue(manager.isSignedIn)
        XCTAssertEqual(manager.status, .idle)
        XCTAssertEqual(manager.pendingAttemptCount, 0)
        XCTAssertEqual(manager.pendingMetaCount, 0)
        XCTAssertEqual(try credStore.loadSession()?.sessionCookie, "tok-abc-123")
        XCTAssertEqual(try db.syncState(localUserId: localUserId)?.pullCursor, 11)

        // Second sync should short-circuit content via known=sha256-demo-snap
        let secondOK = await manager.syncNow(reason: .manual)
        XCTAssertTrue(secondOK, "Second sync failed: \(manager.lastSyncError ?? "nil")")
        XCTAssertEqual(contentCallCount, 2)
        XCTAssertEqual(manager.lastSyncSummary?.contentUnchanged, true)

        // Plant a cookie in HTTPCookieStorage.shared (off the main actor to avoid CFNetwork QoS inversion)
        await Task.detached(priority: .utility) {
            if let planted = HTTPCookie(properties: [
                .domain: "127.0.0.1",
                .path: "/",
                .name: CookieCleaner.sessionCookieName,
                .value: "tok-abc-123"
            ]) {
                HTTPCookieStorage.shared.setCookie(planted)
            }
        }.value

        await manager.signOut()
        let remainingSessionCookies = await Task.detached(priority: .utility) {
            (HTTPCookieStorage.shared.cookies ?? []).filter {
                $0.name == CookieCleaner.sessionCookieName
            }
        }.value
        XCTAssertTrue(logoutCalled)
        XCTAssertFalse(manager.isSignedIn)
        XCTAssertEqual(manager.status, .signedOut)
        XCTAssertNil(try credStore.loadSession())
        XCTAssertTrue(remainingSessionCookies.isEmpty)
    }

    // MARK: - 5. Offline Queue, Interrupted Retry, & Deduplication on Reconnect

    @MainActor
    func testOfflineAnsweringAndReconnectRetriesWithoutDuplicates() async throws {
        let (db, blobsURL, localUserId) = try makeDemoDatabase()
        let remoteUserId = "01a21714-c98d-7931-a48a-6a1d5cd348bd"
        let questionId = "01a21714-c98d-70f7-90ba-1bced11dd072"

        var serverStoredIds = Set<String>()
        var failFirstPushAfterStoring = true
        var pushCallCount = 0

        StubURLProtocol.configure { [self] request in
            let url = request.url!
            switch (request.httpMethod, url.path) {
            case ("GET", "/api/auth/status"):
                return (makeResponse(url: url), Data(#"{"auth_mode":"single","needs_owner_setup":false,"authenticated":true,"role":"owner"}"#.utf8))
            case ("GET", "/api/auth/session"):
                return (makeResponse(url: url), Data(#"{"user_id":"\#(remoteUserId)","role":"owner"}"#.utf8))
            case ("POST", "/api/sync/attempts"):
                pushCallCount += 1
                guard let body = request.readBodyData(),
                      let parsed = try JSONSerialization.jsonObject(with: body) as? [String: Any],
                      let attempts = parsed["attempts"] as? [[String: Any]] else {
                    return (makeResponse(url: url, status: 400), Data())
                }
                var accepted = 0
                var duplicates = 0
                for a in attempts {
                    guard let id = a["id"] as? String else { continue }
                    if serverStoredIds.contains(id) {
                        duplicates += 1
                    } else {
                        serverStoredIds.insert(id)
                        accepted += 1
                    }
                }
                if failFirstPushAfterStoring {
                    failFirstPushAfterStoring = false
                    // Simulate connection drop AFTER server persisted the rows
                    throw URLError(.networkConnectionLost)
                }
                let json = #"{"accepted":\#(accepted),"duplicates":\#(duplicates),"cursor":\#(serverStoredIds.count)}"#
                return (makeResponse(url: url), Data(json.utf8))
            case ("GET", "/api/sync/attempts"):
                return (makeResponse(url: url), Data(#"{"attempts":[],"next_cursor":0,"has_more":false}"#.utf8))
            case ("GET", "/api/sync/user-card-meta"):
                return (makeResponse(url: url), Data(#"{"user_card_meta":[]}"#.utf8))
            case ("GET", "/api/sync/content"):
                return (makeResponse(url: url), Data(#"{"unchanged":true,"snapshot_sha256":"snap-1"}"#.utf8))
            default:
                return (makeResponse(url: url, status: 404), Data())
            }
        }

        let credStore = InMemoryCredentialStore()
        let netMonitor = MockNetworkMonitor(isOnline: true)
        let manager = SyncManager(
            database: db,
            blobsURL: blobsURL,
            localUserId: localUserId,
            apiClient: makeStubClient(),
            credentialStore: credStore,
            networkMonitor: netMonitor,
            userDefaults: testDefaults,
            retryBaseDelaySeconds: 0
        )

        // Sign in (single-user mode, like `reinforce serve --demo`)
        try await manager.signIn(rawURL: "http://127.0.0.1:3000", username: "", password: "")
        XCTAssertTrue(manager.isSignedIn)

        // Go offline and answer 2 questions locally
        netMonitor.setOnline(false)
        let id1 = newAttemptId()
        let id2 = newAttemptId()
        for (idx, attemptId) in [id1, id2].enumerated() {
            _ = try db.appendAttempt(record: AttemptLogRecord(
                id: attemptId,
                userId: localUserId,
                cardId: questionId,
                direction: .default,
                sessionMode: .dailyDue,
                submittedPayloadJson: #"{"keys":["A"]}"#,
                isCorrect: idx == 0,
                durationMs: 1200,
                confidence: .certain,
                errorTags: [],
                createdAt: "2026-01-02T10:0\(idx):00.000000Z",
                syncCursor: try db.nextAttemptCursor(),
                affectsScheduling: true
            ))
        }
        manager.notifyLocalStudyChange()
        XCTAssertEqual(manager.status, .offlineQueued(pendingCount: 2))
        XCTAssertEqual(pushCallCount, 0, "Must not attempt upload while offline")

        // First online attempt drops mid-flight AFTER server stored both attempts
        netMonitor.setOnline(true)
        // Wait briefly for the async Task spawned by network restoration
        for _ in 0..<40 {
            if pushCallCount >= 1 && !manager.isSyncing { break }
            try await Task.sleep(for: .milliseconds(25))
        }
        XCTAssertEqual(pushCallCount, 1)
        XCTAssertEqual(serverStoredIds, [id1, id2])
        XCTAssertEqual(manager.pendingAttemptCount, 2, "Unconfirmed attempts remain queued for retry")

        // Subsequent retry uploads the same IDs; server deduplicates them cleanly
        let retryOK = await manager.syncNow(reason: .manual)
        XCTAssertTrue(retryOK)
        XCTAssertEqual(pushCallCount, 2)
        XCTAssertEqual(manager.pendingAttemptCount, 0)
        XCTAssertEqual(manager.lastSyncSummary?.pushedAttempts, 0)
        XCTAssertEqual(manager.lastSyncSummary?.duplicateAttempts, 2)
        XCTAssertEqual(serverStoredIds.count, 2, "Server must not duplicate retried attempts")

        // Triggering another sync does not re-upload already confirmed attempts
        let noopOK = await manager.syncNow(reason: .manual)
        XCTAssertTrue(noopOK)
        XCTAssertEqual(pushCallCount, 2)
    }

    // MARK: - 6. PDF Export Presets (standard / goodnotes / worksheet)

    @MainActor
    func testPDFExportAllThreePresetsWritesShareableFile() async throws {
        let (db, blobsURL, localUserId) = try makeDemoDatabase()
        let noteId = "01a21714-c98d-769f-be7a-6a1d5cd348bd"
        let fakePDF = Data("%PDF-1.7\n% Reinforce test study sheet\n%%EOF\n".utf8)
        var requestedPresets: [String] = []

        StubURLProtocol.configure { [self] request in
            let url = request.url!
            XCTAssertNil(request.value(forHTTPHeaderField: "Authorization"))
            if request.httpMethod == "POST" && url.path == "/api/export" {
                XCTAssertEqual(request.value(forHTTPHeaderField: "Cookie"), "reinforce_session=pdf-cookie")
                let body = try XCTUnwrap(request.readBodyData())
                let json = try XCTUnwrap(JSONSerialization.jsonObject(with: body) as? [String: Any])
                XCTAssertEqual(json["format"] as? String, "pdf")
                let preset = try XCTUnwrap(json["preset"] as? String)
                requestedPresets.append(preset)
                let scope = try XCTUnwrap(json["scope"] as? [String: Any])
                XCTAssertEqual(scope["kind"] as? String, "note")
                XCTAssertEqual(scope["note_id"] as? String, noteId)
                XCTAssertEqual(json["web_base_url"] as? String, "http://127.0.0.1:3000")

                let headers = [
                    "Content-Type": "application/pdf",
                    "Content-Disposition": "inline; filename=\"note-\(preset).pdf\""
                ]
                return (makeResponse(url: url, headers: headers), fakePDF)
            }
            return (makeResponse(url: url, status: 404), Data())
        }

        let credStore = InMemoryCredentialStore()
        try credStore.saveSession(StoredServerSession(
            serverURL: "http://127.0.0.1:3000",
            authMode: .invite,
            remoteUserId: "01a21714-c98d-7931-a48a-6a1d5cd348bd",
            role: "owner",
            username: "owner",
            sessionCookie: "pdf-cookie"
        ))

        let manager = SyncManager(
            database: db,
            blobsURL: blobsURL,
            localUserId: localUserId,
            apiClient: makeStubClient(),
            credentialStore: credStore,
            networkMonitor: MockNetworkMonitor(isOnline: true),
            userDefaults: testDefaults,
            retryBaseDelaySeconds: 0
        )

        for preset in PDFExportPreset.allCases {
            let fileURL = try await manager.exportPDF(preset: preset, scope: .note(noteId: noteId))
            XCTAssertTrue(FileManager.default.fileExists(atPath: fileURL.path))
            XCTAssertEqual(fileURL.lastPathComponent, "note-\(preset.rawValue).pdf")
            XCTAssertEqual(try Data(contentsOf: fileURL), fakePDF)
        }

        XCTAssertEqual(requestedPresets, ["standard", "goodnotes", "worksheet"])
    }

    // MARK: - 7. Multi-Page Pull Pagination, Content Snapshot with Blobs, Auth Expiry, & Foreground Trigger

    @MainActor
    func testMultiPagePullAndContentSnapshotImportWithBlobs() async throws {
        // Start from a completely empty device database so content snapshot populates everything
        let dbURL = tempDir.appendingPathComponent("empty-sync.db")
        let blobsURL = tempDir.appendingPathComponent("empty-blobs", isDirectory: true)
        let db = try LocalDatabase.openWithBlobs(databasePath: dbURL.path, blobsDir: blobsURL.path)
        let localUserId = try db.createUser(displayName: "Fresh Device")
        let remoteUserId = "01a21714-c98d-7931-a48a-6a1d5cd348bd"
        let questionId = "01a21714-c98d-70f7-90ba-1bced11dd072"

        let demoDir = try fixtureURL("demo")
        let bundleText = try String(contentsOf: demoDir.appendingPathComponent("bundle.jsonl"), encoding: .utf8)
        let baseSvgData = try Data(contentsOf: demoDir.appendingPathComponent("assets/phase-diagram.svg"))
        let overlaySvgData = try Data(contentsOf: demoDir.appendingPathComponent("assets/phase-labels.svg"))

        let baseResourceId = "01a21714-c98c-7bfe-8cf8-a60b84573f56"
        let baseSha = "22df5f119108ae55882dc11a03d09ba426cf4771ef92eaa418457f8db40cbe4a"
        let overlayResourceId = "01a21714-c98c-79c2-a42e-4f429d610443"
        let overlaySha = "b94c10a77bb4b4a7ec82de0314b3cc974b0ff71790c1db0f4d8e2df8b8e5c0a4"

        // Discover actual resource IDs and sha256 hashes from bundle.jsonl so the test never goes stale
        var assetMap: [(resourceId: String, sha256: String, data: Data)] = []
        for line in bundleText.split(separator: "\n") {
            guard let lineData = line.data(using: .utf8),
                  let obj = try? JSONSerialization.jsonObject(with: lineData) as? [String: Any],
                  obj["op"] as? String == "resource",
                  let dataObj = obj["data"] as? [String: Any],
                  let rid = dataObj["id"] as? String,
                  let sha = dataObj["sha256"] as? String else { continue }
            if rid == baseResourceId || sha == baseSha {
                assetMap.append((rid, sha, baseSvgData))
            } else if dataObj["title"] as? String == "相位圖標註覆蓋層" || rid == overlayResourceId || sha == overlaySha {
                assetMap.append((rid, sha, overlaySvgData))
            }
        }

        let attempt1 = newAttemptId()
        let attempt2 = newAttemptId()
        let attempt3 = newAttemptId()
        var pullCursorsSeen: [Int64] = []
        var downloadedResourceIds: [String] = []

        StubURLProtocol.configure { [self] request in
            let url = request.url!
            switch (request.httpMethod, url.path) {
            case ("GET", "/api/sync/attempts"):
                let comps = URLComponents(url: url, resolvingAgainstBaseURL: false)
                let cursor = Int64(comps?.queryItems?.first(where: { $0.name == "cursor" })?.value ?? "0") ?? 0
                pullCursorsSeen.append(cursor)
                if cursor == 0 {
                    // Page 1 returns 2 attempts with has_more=true
                    let json = """
                    {
                      "attempts": [
                        {
                          "id": "\(attempt1)",
                          "user_id": "\(remoteUserId)",
                          "card_id": "\(questionId)",
                          "direction": "default",
                          "session_mode": "daily_due",
                          "submitted_payload": {"keys":["A"]},
                          "is_correct": true,
                          "duration_ms": 1000,
                          "confidence": "certain",
                          "error_tags": [],
                          "created_at": "2026-01-02T10:00:00.000000Z",
                          "sync_cursor": 5,
                          "affects_scheduling": true
                        },
                        {
                          "id": "\(attempt2)",
                          "user_id": "\(remoteUserId)",
                          "card_id": "\(questionId)",
                          "direction": "default",
                          "session_mode": "daily_due",
                          "submitted_payload": {"keys":["B"]},
                          "is_correct": false,
                          "duration_ms": 1800,
                          "confidence": "split",
                          "error_tags": ["careless"],
                          "created_at": "2026-01-02T10:01:00.000000Z",
                          "sync_cursor": 5,
                          "affects_scheduling": true
                        }
                      ],
                      "next_cursor": 5,
                      "has_more": true
                    }
                    """
                    return (makeResponse(url: url), Data(json.utf8))
                } else {
                    // Page 2 finishes pagination
                    let json = """
                    {
                      "attempts": [
                        {
                          "id": "\(attempt3)",
                          "user_id": "\(remoteUserId)",
                          "card_id": "\(questionId)",
                          "direction": "default",
                          "session_mode": "daily_due",
                          "submitted_payload": {"keys":["A"]},
                          "is_correct": true,
                          "duration_ms": 900,
                          "confidence": "certain",
                          "error_tags": [],
                          "created_at": "2026-01-02T10:02:00.000000Z",
                          "sync_cursor": 9,
                          "affects_scheduling": true
                        }
                      ],
                      "next_cursor": 9,
                      "has_more": false
                    }
                    """
                    return (makeResponse(url: url), Data(json.utf8))
                }

            case ("GET", "/api/sync/user-card-meta"):
                return (makeResponse(url: url), Data(#"{"user_card_meta":[]}"#.utf8))

            case ("GET", "/api/sync/content"):
                let assetsJSON: [[String: Any]] = assetMap.map { item in
                    [
                        "resource_id": item.resourceId,
                        "sha256": item.sha256,
                        "byte_size": item.data.count
                    ]
                }
                let payload: [String: Any] = [
                    "unchanged": false,
                    "snapshot_sha256": "snap-full-demo",
                    "bundle_jsonl": bundleText,
                    "assets": assetsJSON
                ]
                let data = try JSONSerialization.data(withJSONObject: payload)
                return (makeResponse(url: url), data)

            case ("GET", _) where url.path.hasPrefix("/api/resources/") && url.path.hasSuffix("/blob"):
                let parts = url.path.split(separator: "/")
                let rid = parts.count >= 3 ? String(parts[2]) : ""
                downloadedResourceIds.append(rid)
                if let match = assetMap.first(where: { $0.resourceId == rid }) {
                    return (makeResponse(url: url, headers: ["Content-Type": "image/svg+xml"]), match.data)
                }
                return (makeResponse(url: url, status: 404), Data())

            default:
                return (makeResponse(url: url, status: 404), Data())
            }
        }

        let credStore = InMemoryCredentialStore()
        try credStore.saveSession(StoredServerSession(
            serverURL: "http://127.0.0.1:3000",
            authMode: .single,
            remoteUserId: remoteUserId,
            role: "owner",
            username: nil,
            sessionCookie: nil
        ))

        let manager = SyncManager(
            database: db,
            blobsURL: blobsURL,
            localUserId: localUserId,
            apiClient: makeStubClient(),
            credentialStore: credStore,
            networkMonitor: MockNetworkMonitor(isOnline: true),
            userDefaults: testDefaults,
            retryBaseDelaySeconds: 0
        )

        let ok = await manager.syncNow(reason: .foreground)
        XCTAssertTrue(ok, "Sync failed: \(manager.lastSyncError ?? "nil")")
        XCTAssertEqual(pullCursorsSeen, [0, 5])
        XCTAssertEqual(try db.syncState(localUserId: localUserId)?.pullCursor, 9)
        XCTAssertEqual(manager.lastSyncSummary?.pulledAttempts, 3)
        XCTAssertGreaterThan(manager.lastSyncSummary?.importedContentItems ?? 0, 0)
        XCTAssertEqual(downloadedResourceIds.count, assetMap.count)
        XCTAssertEqual(try db.notes(subject: nil, limit: 10).count, 1)
    }

    @MainActor
    func testServerURLNormalizationAndAuthExpiredHandling() async throws {
        XCTAssertEqual(
            try ReinforceAPIClient.normalizeServerURL("  HTTP://127.0.0.1:3000///  ").absoluteString,
            "http://127.0.0.1:3000"
        )
        XCTAssertEqual(
            try ReinforceAPIClient.normalizeServerURL("https://study.example.com/sub/?a=1#frag").absoluteString,
            "https://study.example.com/sub"
        )
        XCTAssertThrowsError(try ReinforceAPIClient.normalizeServerURL("ftp://127.0.0.1:3000"))
        XCTAssertThrowsError(try ReinforceAPIClient.normalizeServerURL("   "))

        let (db, blobsURL, localUserId) = try makeDemoDatabase()
        StubURLProtocol.configure { [self] request in
            let url = request.url!
            let errBody = #"{"error":{"code":"unauthorized","message":"session expired"}}"#
            return (makeResponse(url: url, status: 401), Data(errBody.utf8))
        }

        let credStore = InMemoryCredentialStore()
        try credStore.saveSession(StoredServerSession(
            serverURL: "http://127.0.0.1:3000",
            authMode: .invite,
            remoteUserId: "01a21714-c98d-7931-a48a-6a1d5cd348bd",
            role: "learner",
            username: "alice",
            sessionCookie: "expired-cookie"
        ))

        let manager = SyncManager(
            database: db,
            blobsURL: blobsURL,
            localUserId: localUserId,
            apiClient: makeStubClient(),
            credentialStore: credStore,
            networkMonitor: MockNetworkMonitor(isOnline: true),
            userDefaults: testDefaults,
            retryBaseDelaySeconds: 0
        )

        let ok = await manager.syncNow(reason: .manual)
        XCTAssertFalse(ok)
        XCTAssertEqual(manager.status, .authExpired(message: "session expired"))
    }
}
