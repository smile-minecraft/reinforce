import Foundation
import Security

enum ServerAuthMode: String, Codable, Equatable, Sendable {
    case single
    case invite
}

struct StoredServerSession: Codable, Equatable, Sendable {
    let serverURL: String
    let authMode: ServerAuthMode
    let remoteUserId: String
    let role: String
    let username: String?
    let sessionCookie: String?

    var normalizedURL: URL? {
        URL(string: serverURL)
    }

    var displayAccountName: String {
        if let username, !username.isEmpty {
            return username
        }
        return role
    }
}

enum KeychainStoreError: LocalizedError, Equatable {
    case unexpectedStatus(OSStatus)
    case encodingFailed
    case decodingFailed

    var errorDescription: String? {
        switch self {
        case .unexpectedStatus(let status):
            if let message = SecCopyErrorMessageString(status, nil) as String? {
                return "Keychain error (\(status)): \(message)"
            }
            return "Keychain error (\(status))"
        case .encodingFailed:
            return "Failed to encode credentials for Keychain."
        case .decodingFailed:
            return "Failed to decode credentials from Keychain."
        }
    }
}

protocol CredentialStoreProtocol: Sendable {
    func loadSession() throws -> StoredServerSession?
    func saveSession(_ session: StoredServerSession) throws
    func deleteSession() throws
}

final class KeychainCredentialStore: CredentialStoreProtocol, @unchecked Sendable {
    static let defaultService = "app.reinforce.ios.sync"
    static let defaultAccount = "active_server_session"

    let service: String
    let account: String

    init(service: String = defaultService, account: String = defaultAccount) {
        self.service = service
        self.account = account
    }

    func loadSession() throws -> StoredServerSession? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecMatchLimit as String: kSecMatchLimitOne,
            kSecReturnData as String: true
        ]

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound {
            return nil
        }
        guard status == errSecSuccess else {
            throw KeychainStoreError.unexpectedStatus(status)
        }
        guard let data = item as? Data else {
            throw KeychainStoreError.decodingFailed
        }
        do {
            return try JSONDecoder().decode(StoredServerSession.self, from: data)
        } catch {
            throw KeychainStoreError.decodingFailed
        }
    }

    func saveSession(_ session: StoredServerSession) throws {
        let data: Data
        do {
            data = try JSONEncoder().encode(session)
        } catch {
            throw KeychainStoreError.encodingFailed
        }

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account
        ]

        let attributesToUpdate: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlock
        ]

        let updateStatus = SecItemUpdate(query as CFDictionary, attributesToUpdate as CFDictionary)
        if updateStatus == errSecSuccess {
            return
        }

        if updateStatus != errSecItemNotFound {
            throw KeychainStoreError.unexpectedStatus(updateStatus)
        }

        var addQuery = query
        addQuery[kSecValueData as String] = data
        addQuery[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock

        let addStatus = SecItemAdd(addQuery as CFDictionary, nil)
        guard addStatus == errSecSuccess else {
            throw KeychainStoreError.unexpectedStatus(addStatus)
        }
    }

    func deleteSession() throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account
        ]
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeychainStoreError.unexpectedStatus(status)
        }
    }
}

enum CookieCleaner {
    static let sessionCookieName = "reinforce_session"

    static func clearSessionCookies(
        for serverURL: URL?,
        in storages: [HTTPCookieStorage] = [.shared]
    ) {
        let targetHost = serverURL?.host?.lowercased()
        for storage in storages {
            guard let cookies = storage.cookies else { continue }
            for cookie in cookies {
                let matchesName = cookie.name == sessionCookieName
                let matchesHost: Bool = {
                    guard let targetHost else { return false }
                    let domain = cookie.domain.trimmingCharacters(in: CharacterSet(charactersIn: ".")).lowercased()
                    return domain == targetHost || targetHost.hasSuffix(".\(domain)")
                }()
                if matchesName || matchesHost {
                    storage.deleteCookie(cookie)
                }
            }
        }
    }
}
