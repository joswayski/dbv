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

    private static func run<T>(_ body: @escaping () throws -> T) async throws -> T {
        try await Task.detached(priority: .userInitiated, operation: body).value
    }

    private static func decode<T: Decodable>(_ response: String, as type: T.Type) throws -> T {
        guard let data = response.data(using: .utf8) else {
            throw BridgeError.message("the bridge returned invalid UTF-8")
        }
        let envelope = try JSONDecoder().decode(Envelope.self, from: data)
        if let error = envelope.error {
            throw BridgeError.message(error)
        }
        guard let value = envelope.ok, let payload = try? JSONSerialization.data(withJSONObject: value) else {
            throw BridgeError.message("the bridge returned an unexpected response")
        }
        return try JSONDecoder().decode(T.self, from: payload)
    }
}

/// `{"ok": …}` or `{"error": "…"}`.
private struct Envelope: Decodable {
    let ok: AnyJSON?
    let error: String?

    private enum CodingKeys: String, CodingKey {
        case ok, error
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        error = try container.decodeIfPresent(String.self, forKey: .error)
        ok = container.contains(.ok) ? try container.decode(AnyJSON.self, forKey: .ok) : nil
    }
}

/// Decodes any JSON value so it can be re-encoded into a concrete type.
enum AnyJSON: Decodable {
    case value(Any)

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            value = NSNull()
        } else if let bool = try? container.decode(Bool.self) {
            value = bool
        } else if let int = try? container.decode(Int.self) {
            value = int
        } else if let double = try? container.decode(Double.self) {
            value = double
        } else if let string = try? container.decode(String.self) {
            value = string
        } else if let array = try? container.decode([AnyJSON].self) {
            value = array.map(\.value)
        } else if let dictionary = try? container.decode([String: AnyJSON].self) {
            value = dictionary.mapValues(\.value)
        } else {
            throw BridgeError.message("the bridge returned an unsupported value")
        }
    }
}

extension Encodable {
    /// Encodes a value into a `JSONSerialization`-compatible object.
    func jsonObject() throws -> Any {
        let data = try JSONEncoder().encode(self)
        return try JSONSerialization.jsonObject(with: data)
    }
}
