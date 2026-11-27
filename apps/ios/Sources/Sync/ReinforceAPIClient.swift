import Foundation

enum ReinforceAPIError: LocalizedError, Equatable {
    case invalidServerURL
    case offline
    case unauthorized(String)
    case forbidden(String)
    case conflict(String)
    case missingSessionCookie
    case serverError(statusCode: Int, message: String)
    case decodingError(String)
    case networkError(String)

    var isRetryableNetworkFailure: Bool {
        switch self {
        case .offline, .networkError:
            return true
        case .serverError(let statusCode, _):
            return statusCode >= 500 || statusCode == 429
        default:
            return false
        }
    }

    var isUnauthorized: Bool {
        if case .unauthorized = self { return true }
        return false
    }

    var errorDescription: String? {
        switch self {
        case .invalidServerURL:
            return "Please enter a valid http:// or https:// server URL."
        case .offline:
            return "No network connection. Changes are queued locally."
        case .unauthorized(let message):
            return message.isEmpty ? "Authentication required. Please sign in again." : message
        case .forbidden(let message):
            return message.isEmpty ? "Access forbidden." : message
        case .conflict(let message):
            return message.isEmpty ? "Sync conflict detected." : message
        case .missingSessionCookie:
            return "Server login succeeded without returning a session cookie."
        case .serverError(let statusCode, let message):
            return "Server error (\(statusCode)): \(message)"
        case .decodingError(let message):
            return "Failed to parse server response: \(message)"
        case .networkError(let message):
            return message
        }
    }
}

final class ReinforceAPIClient: @unchecked Sendable {
    let session: URLSession
    let cookieStorage: HTTPCookieStorage?

    init(configuration: URLSessionConfiguration? = nil) {
        let config = configuration ?? {
            let c = URLSessionConfiguration.default
            c.timeoutIntervalForRequest = 30
            c.timeoutIntervalForResource = 60
            c.waitsForConnectivity = false
            return c
        }()
        self.cookieStorage = config.httpCookieStorage
        self.session = URLSession(configuration: config)
    }

    static func normalizeServerURL(_ raw: String) throws -> URL {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, var components = URLComponents(string: trimmed) else {
            throw ReinforceAPIError.invalidServerURL
        }
        guard let scheme = components.scheme?.lowercased(),
              (scheme == "http" || scheme == "https"),
              let host = components.host,
              !host.isEmpty else {
            throw ReinforceAPIError.invalidServerURL
        }
        components.scheme = scheme
        while components.path.hasSuffix("/") {
            components.path.removeLast()
        }
        components.query = nil
        components.fragment = nil
        guard let url = components.url else {
            throw ReinforceAPIError.invalidServerURL
        }
        return url
    }

    func clearStoredCookies(for serverURL: URL?) {
        let storage = cookieStorage
        DispatchQueue.global(qos: .utility).async {
            var storages: [HTTPCookieStorage] = [.shared]
            if let storage, storage !== HTTPCookieStorage.shared {
                storages.append(storage)
            }
            CookieCleaner.clearSessionCookies(for: serverURL, in: storages)
        }
    }

    func clearStoredCookiesAsync(for serverURL: URL?) async {
        let storage = cookieStorage
        await Task.detached(priority: .utility) {
            var storages: [HTTPCookieStorage] = [.shared]
            if let storage, storage !== HTTPCookieStorage.shared {
                storages.append(storage)
            }
            CookieCleaner.clearSessionCookies(for: serverURL, in: storages)
        }.value
    }

    // MARK: - Auth Endpoints

    func authStatus(serverURL: URL, sessionCookie: String? = nil) async throws -> AuthStatusResponse {
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/auth/status",
            method: "GET",
            sessionCookie: sessionCookie
        )
        let (data, _) = try await perform(request)
        do {
            return try JSONDecoder().decode(AuthStatusResponse.self, from: data)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
    }

    func currentSession(serverURL: URL, sessionCookie: String? = nil) async throws -> SessionInfoResponse {
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/auth/session",
            method: "GET",
            sessionCookie: sessionCookie
        )
        let (data, _) = try await perform(request)
        do {
            return try JSONDecoder().decode(SessionInfoResponse.self, from: data)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
    }

    func login(serverURL: URL, username: String, password: String) async throws -> LoginResponse {
        let body = try JSONEncoder().encode(["username": username, "password": password])
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/auth/login",
            method: "POST",
            body: body,
            sessionCookie: nil
        )
        let (data, http) = try await perform(request)
        guard let cookie = SyncWireCodec.extractSessionCookie(from: http, url: serverURL) else {
            throw ReinforceAPIError.missingSessionCookie
        }
        do {
            let info = try JSONDecoder().decode(SessionInfoResponse.self, from: data)
            return LoginResponse(userId: info.userId, role: info.role, sessionCookie: cookie)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
    }

    func logout(serverURL: URL, sessionCookie: String?) async {
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/auth/logout",
            method: "POST",
            sessionCookie: sessionCookie
        )
        _ = try? await perform(request)
    }

    // MARK: - Sync Endpoints (Session cookie only; never sends Authorization header)

    func pushAttempts(
        serverURL: URL,
        sessionCookie: String?,
        attempts: [AttemptLogRecord]
    ) async throws -> PushAttemptsResponse {
        let body: Data
        do {
            body = try SyncWireCodec.encodePushAttemptsBody(attempts)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/sync/attempts",
            method: "POST",
            body: body,
            sessionCookie: sessionCookie
        )
        let (data, _) = try await perform(request)
        do {
            return try JSONDecoder().decode(PushAttemptsResponse.self, from: data)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
    }

    func pullAttempts(
        serverURL: URL,
        sessionCookie: String?,
        cursor: Int64,
        limit: Int = 200
    ) async throws -> PullAttemptsResponse {
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/sync/attempts",
            queryItems: [
                URLQueryItem(name: "cursor", value: String(cursor)),
                URLQueryItem(name: "limit", value: String(limit))
            ],
            method: "GET",
            sessionCookie: sessionCookie
        )
        let (data, _) = try await perform(request)
        do {
            return try SyncWireCodec.decodePullAttemptsResponse(from: data)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
    }

    func pushUserCardMeta(
        serverURL: URL,
        sessionCookie: String?,
        rows: [UserCardMeta]
    ) async throws -> PushUserCardMetaResponse {
        let body: Data
        do {
            body = try SyncWireCodec.encodePushUserCardMetaBody(rows)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/sync/user-card-meta",
            method: "PUT",
            body: body,
            sessionCookie: sessionCookie
        )
        let (data, _) = try await perform(request)
        do {
            return try JSONDecoder().decode(PushUserCardMetaResponse.self, from: data)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
    }

    func pullUserCardMeta(
        serverURL: URL,
        sessionCookie: String?
    ) async throws -> [UserCardMeta] {
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/sync/user-card-meta",
            method: "GET",
            sessionCookie: sessionCookie
        )
        let (data, _) = try await perform(request)
        do {
            return try SyncWireCodec.decodePullUserCardMetaResponse(from: data)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
    }

    func fetchContentSnapshot(
        serverURL: URL,
        sessionCookie: String?,
        knownSha256: String?
    ) async throws -> ContentSnapshotResponse {
        var queryItems: [URLQueryItem] = []
        if let knownSha256, !knownSha256.isEmpty {
            queryItems.append(URLQueryItem(name: "known", value: knownSha256))
        }
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/sync/content",
            queryItems: queryItems,
            method: "GET",
            sessionCookie: sessionCookie
        )
        let (data, _) = try await perform(request)
        do {
            return try JSONDecoder().decode(ContentSnapshotResponse.self, from: data)
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
    }

    func downloadResourceBlob(
        serverURL: URL,
        sessionCookie: String?,
        resourceId: String
    ) async throws -> Data {
        let encodedId = resourceId.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? resourceId
        let request = makeRequest(
            serverURL: serverURL,
            path: "/api/resources/\(encodedId)/blob",
            method: "GET",
            sessionCookie: sessionCookie
        )
        let (data, _) = try await perform(request)
        return data
    }

    // MARK: - PDF Export

    func exportPDF(
        serverURL: URL,
        sessionCookie: String?,
        preset: PDFExportPreset,
        scope: PDFExportScope
    ) async throws -> ExportedPDFFile {
        let body: Data
        do {
            body = try SyncWireCodec.encodeExportPDFBody(
                preset: preset,
                scope: scope,
                webBaseURL: serverURL.absoluteString
            )
        } catch {
            throw ReinforceAPIError.decodingError(error.localizedDescription)
        }
        var request = makeRequest(
            serverURL: serverURL,
            path: "/api/export",
            method: "POST",
            body: body,
            sessionCookie: sessionCookie
        )
        request.setValue("application/pdf", forHTTPHeaderField: "Accept")
        request.timeoutInterval = 60

        let (data, http) = try await perform(request)
        let fallbackName = "reinforce-\(preset.rawValue).pdf"
        let suggested = SyncWireCodec.extractFilename(
            fromContentDisposition: http.value(forHTTPHeaderField: "Content-Disposition")
        ) ?? fallbackName
        return ExportedPDFFile(data: data, suggestedFilename: suggested)
    }

    // MARK: - Request Builder & Execution

    private func makeRequest(
        serverURL: URL,
        path: String,
        queryItems: [URLQueryItem] = [],
        method: String,
        body: Data? = nil,
        sessionCookie: String?
    ) -> URLRequest {
        var components = URLComponents(url: serverURL, resolvingAgainstBaseURL: false)!
        let basePath = components.path.hasSuffix("/") ? String(components.path.dropLast()) : components.path
        components.path = basePath + path
        if !queryItems.isEmpty {
            components.queryItems = queryItems
        }

        var request = URLRequest(url: components.url!)
        request.httpMethod = method
        request.httpShouldHandleCookies = false
        request.setValue("application/json", forHTTPHeaderField: "Accept")

        if let body {
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        if let sessionCookie, !sessionCookie.isEmpty {
            request.setValue("\(CookieCleaner.sessionCookieName)=\(sessionCookie)", forHTTPHeaderField: "Cookie")
        }
        // Explicitly never attach an Authorization header; device sync endpoints reject Bearer tokens with 403.
        request.setValue(nil, forHTTPHeaderField: "Authorization")
        return request
    }

    private func perform(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch let urlError as URLError {
            switch urlError.code {
            case .notConnectedToInternet, .networkConnectionLost, .dataNotAllowed, .internationalRoamingOff:
                throw ReinforceAPIError.offline
            default:
                throw ReinforceAPIError.networkError(urlError.localizedDescription)
            }
        } catch {
            throw ReinforceAPIError.networkError(error.localizedDescription)
        }

        guard let http = response as? HTTPURLResponse else {
            throw ReinforceAPIError.networkError("Invalid HTTP response.")
        }

        switch http.statusCode {
        case 200..<300:
            return (data, http)
        case 401:
            throw ReinforceAPIError.unauthorized(SyncWireCodec.parseAPIErrorMessage(from: data, statusCode: 401))
        case 403:
            throw ReinforceAPIError.forbidden(SyncWireCodec.parseAPIErrorMessage(from: data, statusCode: 403))
        case 409:
            throw ReinforceAPIError.conflict(SyncWireCodec.parseAPIErrorMessage(from: data, statusCode: 409))
        default:
            throw ReinforceAPIError.serverError(
                statusCode: http.statusCode,
                message: SyncWireCodec.parseAPIErrorMessage(from: data, statusCode: http.statusCode)
            )
        }
    }
}
