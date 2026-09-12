import Foundation

// MARK: - C ABI

@_silgen_name("dbm_init")
private func dbm_init_raw() -> UnsafeMutablePointer<CChar>?

@_silgen_name("dbm_call")
private func dbm_call_raw(_ request: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?

@_silgen_name("dbm_free")
private func dbm_free_raw(_ pointer: UnsafeMutablePointer<CChar>?)

@_silgen_name("dbm_shutdown")
private func dbm_shutdown_raw()

// MARK: - Errors

enum BridgeError: LocalizedError {
    case message(String)

    var errorDescription: String? {
        switch self {
        case let .message(message): return message
        }
    }
}

// MARK: - Bridge

/// The Swift side of the Rust bridge: one JSON request in, one JSON response
/// out. Every call blocks, so callers must be off the main actor.
enum Bridge {
    /// Initializes the engine and returns the saved profiles.
    static func initialize() async throws -> [ProfileSummary] {
        try await run {
            guard let pointer = dbm_init_raw() else {
                throw BridgeError.message("the bridge did not return a response")
            }
            defer { dbm_free_raw(pointer) }
            let response = String(cString: pointer)
            return try decode(response, as: [ProfileSummary].self)
        }
    }

    /// Performs one operation.
    static func call<T: Decodable>(_ request: [String: Any], as type: T.Type) async throws -> T {
        let data = try JSONSerialization.data(withJSONObject: request)
        let payload = String(decoding: data, as: UTF8.self)
        return try await run {
            let response = try callRaw(payload)
            return try decode(response, as: T.self)
        }
    }

    /// Performs one operation that returns no payload.
    static func call(_ request: [String: Any]) async throws {
        let data = try JSONSerialization.data(withJSONObject: request)
        let payload = String(decoding: data, as: UTF8.self)
        _ = try await run { try callRaw(payload) }
    }

    static func shutdown() {
        dbm_shutdown_raw()
    }

    private static func callRaw(_ request: String) throws -> String {
        let response = request.withCString { pointer in
            dbm_call_raw(pointer).map { String(cString: $0) }
        }
        guard let response else {
            throw BridgeError.message("the bridge did not return a response")
        }
        // The Rust side owns the pointer until we free it; `dbm_call_raw`
        // returns an owned pointer, so release it here.
        return response
    }

    private static func run<T>(_ body: @escaping @Sendable () throws -> T) async throws -> T {
        try await Task.detached(priority: .userInitiated, operation: body).value
    }

    private static func decode<T: Decodable>(_ response: String, as type: T.Type) throws -> T {
        guard let data = response.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            throw BridgeError.message("the bridge returned invalid JSON")
        }
        if let error = object["error"] as? String {
            throw BridgeError.message(error)
        }
        guard let value = object["ok"], !(value is NSNull) else {
            throw BridgeError.message("the bridge returned an unexpected response")
        }
        let payload = try JSONSerialization.data(withJSONObject: value)
        return try JSONDecoder().decode(T.self, from: payload)
    }
}

extension Encodable {
    /// Encodes a value into a `JSONSerialization`-compatible object.
    func jsonObject() throws -> Any {
        let data = try JSONEncoder().encode(self)
        return try JSONSerialization.jsonObject(with: data)
    }
}
