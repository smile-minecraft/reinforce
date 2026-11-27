import Foundation

enum PDFExportPreset: String, CaseIterable, Identifiable, Codable, Sendable {
    case standard
    case goodnotes
    case worksheet

    var id: String { rawValue }
}

enum PDFExportScope: Equatable, Sendable {
    case note(noteId: String)
    case subject(subject: String)
    case weakness(threshold: Double)
}

struct AuthStatusResponse: Codable, Equatable, Sendable {
    let authMode: ServerAuthMode
    let needsOwnerSetup: Bool
    let authenticated: Bool
    let role: String?

    enum CodingKeys: String, CodingKey {
        case authMode = "auth_mode"
        case needsOwnerSetup = "needs_owner_setup"
        case authenticated
        case role
    }
}

struct SessionInfoResponse: Codable, Equatable, Sendable {
    let userId: String
    let role: String

    enum CodingKeys: String, CodingKey {
        case userId = "user_id"
        case role
    }
}

struct LoginResponse: Equatable, Sendable {
    let userId: String
    let role: String
    let sessionCookie: String
}

struct PushAttemptsResponse: Codable, Equatable, Sendable {
    let accepted: Int
    let duplicates: Int
    let cursor: Int64
}

struct PullAttemptsResponse: Equatable {
    let attempts: [AttemptLogRecord]
    let nextCursor: Int64
    let hasMore: Bool
}

struct PushUserCardMetaResponse: Codable, Equatable, Sendable {
    let applied: Int
    let kept: Int
}

struct RemoteContentAsset: Codable, Equatable, Sendable {
    let resourceId: String
    let sha256: String
    let byteSize: Int64

    enum CodingKeys: String, CodingKey {
        case resourceId = "resource_id"
        case sha256
        case byteSize = "byte_size"
    }
}

struct ContentSnapshotResponse: Codable, Equatable, Sendable {
    let unchanged: Bool
    let snapshotSha256: String
    let bundleJsonl: String?
    let assets: [RemoteContentAsset]?

    enum CodingKeys: String, CodingKey {
        case unchanged
        case snapshotSha256 = "snapshot_sha256"
        case bundleJsonl = "bundle_jsonl"
        case assets
    }
}

struct ExportedPDFFile: Equatable, Sendable {
    let data: Data
    let suggestedFilename: String
}

enum SyncWireCodecError: LocalizedError, Equatable {
    case invalidSubmittedPayload(attemptId: String)
    case invalidResponseShape(String)

    var errorDescription: String? {
        switch self {
        case .invalidSubmittedPayload(let id):
            return "Invalid submitted_payload JSON on attempt \(id)."
        case .invalidResponseShape(let detail):
            return "Unexpected server response: \(detail)"
        }
    }
}

enum SyncWireCodec {
    private static let encoder = JSONEncoder()

    // MARK: - Enum wire mappings

    static func wireValue(for direction: AttemptDirection) -> String {
        switch direction {
        case .default: return "default"
        case .forward: return "forward"
        case .reverse: return "reverse"
        }
    }

    static func attemptDirection(from wire: String) throws -> AttemptDirection {
        switch wire {
        case "default": return .default
        case "forward": return .forward
        case "reverse": return .reverse
        default: throw SyncWireCodecError.invalidResponseShape("unknown direction \(wire)")
        }
    }

    static func wireValue(for mode: SessionMode) -> String {
        switch mode {
        case .dailyDue: return "daily_due"
        case .fullExam: return "full_exam"
        case .topicDrill: return "topic_drill"
        case .practicalExam: return "practical_exam"
        case .weaknessDrill: return "weakness_drill"
        }
    }

    static func sessionMode(from wire: String) throws -> SessionMode {
        switch wire {
        case "daily_due": return .dailyDue
        case "full_exam": return .fullExam
        case "topic_drill": return .topicDrill
        case "practical_exam": return .practicalExam
        case "weakness_drill": return .weaknessDrill
        default: throw SyncWireCodecError.invalidResponseShape("unknown session_mode \(wire)")
        }
    }

    static func wireValue(for confidence: Confidence) -> String {
        switch confidence {
        case .certain: return "certain"
        case .split: return "split"
        case .guessed: return "guessed"
        }
    }

    static func confidence(from wire: String) throws -> Confidence {
        switch wire {
        case "certain": return .certain
        case "split": return .split
        case "guessed": return .guessed
        default: throw SyncWireCodecError.invalidResponseShape("unknown confidence \(wire)")
        }
    }

    // MARK: - Attempts encoding (preserves submitted_payload verbatim)

    /// Encodes `{"attempts": [...]}` while splicing each record's `submittedPayloadJson`
    /// verbatim so numeric representations (e.g. `0` vs `0.0`) and key order never mutate
    /// across retries, preventing HTTP 409 payload mismatch conflicts.
    static func encodePushAttemptsBody(_ attempts: [AttemptLogRecord]) throws -> Data {
        var out = Data()
        out.append(contentsOf: #"{"attempts":["#.utf8)

        for (index, record) in attempts.enumerated() {
            if index > 0 {
                out.append(contentsOf: ",".utf8)
            }
            let rawPayload = record.submittedPayloadJson.trimmingCharacters(in: .whitespacesAndNewlines)
            guard let payloadBytes = rawPayload.data(using: .utf8),
                  (try? JSONSerialization.jsonObject(with: payloadBytes, options: [.fragmentsAllowed])) != nil else {
                throw SyncWireCodecError.invalidSubmittedPayload(attemptId: record.id)
            }

            out.append(contentsOf: "{".utf8)
            try appendJSONField("id", value: record.id, to: &out, first: true)
            try appendJSONField("user_id", value: record.userId, to: &out, first: false)
            try appendJSONField("card_id", value: record.cardId, to: &out, first: false)
            try appendJSONField("direction", value: wireValue(for: record.direction), to: &out, first: false)
            try appendJSONField("session_mode", value: wireValue(for: record.sessionMode), to: &out, first: false)

            out.append(contentsOf: #","submitted_payload":"#.utf8)
            out.append(payloadBytes)

            try appendJSONField("is_correct", value: record.isCorrect, to: &out, first: false)
            try appendJSONField("duration_ms", value: record.durationMs, to: &out, first: false)
            try appendJSONField("confidence", value: wireValue(for: record.confidence), to: &out, first: false)
            try appendJSONField("error_tags", value: record.errorTags, to: &out, first: false)
            try appendJSONField("created_at", value: record.createdAt, to: &out, first: false)
            try appendJSONField("sync_cursor", value: record.syncCursor, to: &out, first: false)
            try appendJSONField("affects_scheduling", value: record.affectsScheduling, to: &out, first: false)
            out.append(contentsOf: "}".utf8)
        }

        out.append(contentsOf: "]}".utf8)
        return out
    }

    private static func appendJSONField<T: Encodable>(
        _ key: String,
        value: T,
        to buffer: inout Data,
        first: Bool
    ) throws {
        if !first {
            buffer.append(contentsOf: ",".utf8)
        }
        buffer.append(contentsOf: "\"\(key)\":".utf8)
        buffer.append(try encoder.encode(value))
    }

    // MARK: - Attempts decoding (extracts raw submitted_payload JSON)

    static func decodePullAttemptsResponse(from data: Data) throws -> PullAttemptsResponse {
        guard let root = try JSONSerialization.jsonObject(with: data, options: []) as? [String: Any] else {
            throw SyncWireCodecError.invalidResponseShape("expected object root")
        }
        guard let rawAttempts = root["attempts"] as? [[String: Any]] else {
            throw SyncWireCodecError.invalidResponseShape("missing attempts array")
        }
        guard let nextCursorNumber = root["next_cursor"] as? NSNumber else {
            throw SyncWireCodecError.invalidResponseShape("missing next_cursor")
        }
        guard let hasMore = root["has_more"] as? Bool else {
            throw SyncWireCodecError.invalidResponseShape("missing has_more")
        }

        // Extract raw submitted_payload strings from the original bytes in array order so
        // numbers like 0 never get widened into 0.0 when stored locally.
        let rawSubmittedPayloads = extractRawSubmittedPayloads(from: data)

        var records: [AttemptLogRecord] = []
        records.reserveCapacity(rawAttempts.count)

        for (index, item) in rawAttempts.enumerated() {
            guard let id = item["id"] as? String,
                  let userId = item["user_id"] as? String,
                  let cardId = item["card_id"] as? String,
                  let directionStr = item["direction"] as? String,
                  let modeStr = item["session_mode"] as? String,
                  let isCorrect = item["is_correct"] as? Bool,
                  let durationMs = (item["duration_ms"] as? NSNumber)?.int64Value,
                  let confidenceStr = item["confidence"] as? String,
                  let errorTags = item["error_tags"] as? [String],
                  let createdAt = item["created_at"] as? String,
                  let syncCursor = (item["sync_cursor"] as? NSNumber)?.int64Value,
                  let affectsScheduling = item["affects_scheduling"] as? Bool else {
                throw SyncWireCodecError.invalidResponseShape("incomplete attempt record at index \(index)")
            }

            let payloadJSON: String
            if let rawList = rawSubmittedPayloads, index < rawList.count {
                payloadJSON = rawList[index]
            } else if let payloadObj = item["submitted_payload"],
                      let serialized = try? JSONSerialization.data(withJSONObject: payloadObj, options: [.fragmentsAllowed]),
                      let text = String(data: serialized, encoding: .utf8) {
                payloadJSON = text
            } else {
                throw SyncWireCodecError.invalidResponseShape("missing submitted_payload at index \(index)")
            }

            records.append(
                AttemptLogRecord(
                    id: id,
                    userId: userId,
                    cardId: cardId,
                    direction: try attemptDirection(from: directionStr),
                    sessionMode: try sessionMode(from: modeStr),
                    submittedPayloadJson: payloadJSON,
                    isCorrect: isCorrect,
                    durationMs: durationMs,
                    confidence: try confidence(from: confidenceStr),
                    errorTags: errorTags,
                    createdAt: createdAt,
                    syncCursor: syncCursor,
                    affectsScheduling: affectsScheduling
                )
            )
        }

        return PullAttemptsResponse(
            attempts: records,
            nextCursor: nextCursorNumber.int64Value,
            hasMore: hasMore
        )
    }

    // MARK: - UserCardMeta wire encoding/decoding

    private struct WireUserCardMeta: Codable {
        let userId: String
        let cardId: String
        let starred: Bool
        let personalTags: [String]
        let personalNote: String?
        let updatedAt: String

        enum CodingKeys: String, CodingKey {
            case userId = "user_id"
            case cardId = "card_id"
            case starred
            case personalTags = "personal_tags"
            case personalNote = "personal_note"
            case updatedAt = "updated_at"
        }

        init(from record: UserCardMeta) {
            self.userId = record.userId
            self.cardId = record.cardId
            self.starred = record.starred
            self.personalTags = record.personalTags
            self.personalNote = record.personalNote
            self.updatedAt = record.updatedAt
        }

        var asRecord: UserCardMeta {
            UserCardMeta(
                userId: userId,
                cardId: cardId,
                starred: starred,
                personalTags: personalTags,
                personalNote: personalNote,
                updatedAt: updatedAt
            )
        }
    }

    private struct WireUserCardMetaEnvelope: Codable {
        let userCardMeta: [WireUserCardMeta]

        enum CodingKeys: String, CodingKey {
            case userCardMeta = "user_card_meta"
        }
    }

    static func encodePushUserCardMetaBody(_ rows: [UserCardMeta]) throws -> Data {
        let envelope = WireUserCardMetaEnvelope(userCardMeta: rows.map(WireUserCardMeta.init))
        return try encoder.encode(envelope)
    }

    static func decodePullUserCardMetaResponse(from data: Data) throws -> [UserCardMeta] {
        let envelope = try JSONDecoder().decode(WireUserCardMetaEnvelope.self, from: data)
        return envelope.userCardMeta.map(\.asRecord)
    }

    // MARK: - Export PDF request

    static func encodeExportPDFBody(
        preset: PDFExportPreset,
        scope: PDFExportScope,
        webBaseURL: String?
    ) throws -> Data {
        var body: [String: Any] = [
            "format": "pdf",
            "preset": preset.rawValue
        ]
        switch scope {
        case .note(let noteId):
            body["scope"] = ["kind": "note", "note_id": noteId]
        case .subject(let subject):
            body["scope"] = ["kind": "subject", "subject": subject]
        case .weakness(let threshold):
            body["scope"] = ["kind": "weakness", "threshold": threshold]
        }
        if let webBaseURL, (webBaseURL.hasPrefix("http://") || webBaseURL.hasPrefix("https://")) {
            body["web_base_url"] = webBaseURL
        }
        return try JSONSerialization.data(withJSONObject: body, options: [])
    }

    // MARK: - Header & Error helpers

    static func extractSessionCookie(
        from response: HTTPURLResponse,
        url: URL,
        cookieName: String = CookieCleaner.sessionCookieName
    ) -> String? {
        var headerFields: [String: String] = [:]
        for (key, value) in response.allHeaderFields {
            if let k = key as? String, let v = value as? String {
                headerFields[k] = v
            }
        }
        let cookies = HTTPCookie.cookies(withResponseHeaderFields: headerFields, for: url)
        if let matched = cookies.first(where: { $0.name == cookieName }), !matched.value.isEmpty {
            return matched.value
        }

        // Fallback raw parse in case URLSession did not parse Set-Cookie for a custom host/stub
        for (key, value) in headerFields where key.caseInsensitiveCompare("Set-Cookie") == .orderedSame {
            for segment in value.split(separator: ";") {
                let trimmed = segment.trimmingCharacters(in: .whitespacesAndNewlines)
                if let eqIndex = trimmed.firstIndex(of: "=") {
                    let name = String(trimmed[..<eqIndex]).trimmingCharacters(in: .whitespaces)
                    let val = String(trimmed[trimmed.index(after: eqIndex)...]).trimmingCharacters(in: .whitespaces)
                    if name == cookieName && !val.isEmpty {
                        return val
                    }
                }
            }
        }
        return nil
    }

    static func extractFilename(fromContentDisposition header: String?) -> String? {
        guard let header else { return nil }
        let parts = header.split(separator: ";")
        for part in parts {
            let trimmed = part.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.lowercased().hasPrefix("filename=") {
                var raw = String(trimmed.dropFirst("filename=".count))
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                if raw.hasPrefix("\"") && raw.hasSuffix("\"") && raw.count >= 2 {
                    raw = String(raw.dropFirst().dropLast())
                }
                if !raw.isEmpty {
                    return raw
                }
            }
        }
        return nil
    }

    static func parseAPIErrorMessage(from data: Data, statusCode: Int) -> String {
        if let json = try? JSONSerialization.jsonObject(with: data, options: []) as? [String: Any],
           let err = json["error"] as? [String: Any],
           let message = err["message"] as? String,
           !message.isEmpty {
            return message
        }
        if let text = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
           !text.isEmpty,
           text.count < 300 {
            return text
        }
        return HTTPURLResponse.localizedString(forStatusCode: statusCode)
    }

    // MARK: - Raw JSON scanner for submitted_payload preservation

    /// Scans the `attempts` array in a `GET /api/sync/attempts` JSON body and returns the
    /// exact verbatim JSON substring of each attempt's `"submitted_payload"` value.
    static func extractRawSubmittedPayloads(from data: Data) -> [String]? {
        let bytes = Array(data)
        var scanner = RawJSONScanner(bytes: bytes)
        scanner.skipWhitespace()
        guard scanner.consume(UInt8(ascii: "{")) else { return nil }

        while true {
            scanner.skipWhitespace()
            if scanner.consume(UInt8(ascii: "}")) {
                return nil
            }
            guard let key = scanner.readString() else { return nil }
            scanner.skipWhitespace()
            guard scanner.consume(UInt8(ascii: ":")) else { return nil }
            scanner.skipWhitespace()

            if key == "attempts" {
                return scanner.readAttemptsSubmittedPayloads()
            } else {
                guard scanner.skipValue() else { return nil }
            }

            scanner.skipWhitespace()
            if scanner.consume(UInt8(ascii: ",")) {
                continue
            } else if scanner.consume(UInt8(ascii: "}")) {
                return nil
            } else {
                return nil
            }
        }
    }
}

private struct RawJSONScanner {
    let bytes: [UInt8]
    var idx: Int = 0

    var isAtEnd: Bool { idx >= bytes.count }

    mutating func skipWhitespace() {
        while idx < bytes.count {
            let b = bytes[idx]
            if b == 0x20 || b == 0x09 || b == 0x0A || b == 0x0D {
                idx += 1
            } else {
                break
            }
        }
    }

    mutating func consume(_ expected: UInt8) -> Bool {
        guard idx < bytes.count, bytes[idx] == expected else { return false }
        idx += 1
        return true
    }

    mutating func readString() -> String? {
        guard idx < bytes.count, bytes[idx] == UInt8(ascii: "\"") else { return nil }
        let start = idx
        idx += 1
        while idx < bytes.count {
            let b = bytes[idx]
            if b == UInt8(ascii: "\\") {
                idx += 2
                continue
            }
            if b == UInt8(ascii: "\"") {
                idx += 1
                let slice = Data(bytes[start..<idx])
                return try? JSONDecoder().decode(String.self, from: slice)
            }
            idx += 1
        }
        return nil
    }

    mutating func skipString() -> Bool {
        guard idx < bytes.count, bytes[idx] == UInt8(ascii: "\"") else { return false }
        idx += 1
        while idx < bytes.count {
            let b = bytes[idx]
            if b == UInt8(ascii: "\\") {
                idx += 2
                continue
            }
            if b == UInt8(ascii: "\"") {
                idx += 1
                return true
            }
            idx += 1
        }
        return false
    }

    mutating func skipValue() -> Bool {
        skipWhitespace()
        guard idx < bytes.count else { return false }
        let b = bytes[idx]
        switch b {
        case UInt8(ascii: "\""):
            return skipString()
        case UInt8(ascii: "{"):
            idx += 1
            skipWhitespace()
            if consume(UInt8(ascii: "}")) { return true }
            while true {
                skipWhitespace()
                guard skipString() else { return false }
                skipWhitespace()
                guard consume(UInt8(ascii: ":")) else { return false }
                guard skipValue() else { return false }
                skipWhitespace()
                if consume(UInt8(ascii: ",")) {
                    continue
                }
                return consume(UInt8(ascii: "}"))
            }
        case UInt8(ascii: "["):
            idx += 1
            skipWhitespace()
            if consume(UInt8(ascii: "]")) { return true }
            while true {
                guard skipValue() else { return false }
                skipWhitespace()
                if consume(UInt8(ascii: ",")) {
                    continue
                }
                return consume(UInt8(ascii: "]"))
            }
        default:
            while idx < bytes.count {
                let c = bytes[idx]
                if c == UInt8(ascii: ",") || c == UInt8(ascii: "}") || c == UInt8(ascii: "]")
                    || c == 0x20 || c == 0x09 || c == 0x0A || c == 0x0D {
                    break
                }
                idx += 1
            }
            return true
        }
    }

    mutating func readAttemptsSubmittedPayloads() -> [String]? {
        guard consume(UInt8(ascii: "[")) else { return nil }
        var results: [String] = []
        skipWhitespace()
        if consume(UInt8(ascii: "]")) {
            return results
        }
        while true {
            skipWhitespace()
            guard let payload = readSubmittedPayloadFromObject() else { return nil }
            results.append(payload)
            skipWhitespace()
            if consume(UInt8(ascii: ",")) {
                continue
            }
            guard consume(UInt8(ascii: "]")) else { return nil }
            return results
        }
    }

    private mutating func readSubmittedPayloadFromObject() -> String? {
        guard consume(UInt8(ascii: "{")) else { return nil }
        var found: String?
        skipWhitespace()
        if consume(UInt8(ascii: "}")) {
            return found
        }
        while true {
            skipWhitespace()
            guard let key = readString() else { return nil }
            skipWhitespace()
            guard consume(UInt8(ascii: ":")) else { return nil }
            skipWhitespace()
            let valueStart = idx
            guard skipValue() else { return nil }
            let valueEnd = idx
            if key == "submitted_payload" {
                found = String(bytes: bytes[valueStart..<valueEnd], encoding: .utf8)
            }
            skipWhitespace()
            if consume(UInt8(ascii: ",")) {
                continue
            }
            guard consume(UInt8(ascii: "}")) else { return nil }
            return found
        }
    }
}
