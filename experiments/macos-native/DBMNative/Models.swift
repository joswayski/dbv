import Foundation

// MARK: - Engines

enum DatabaseEngine: String, Codable, CaseIterable, Identifiable {
    case postgres
    case mysql
    case redis

    var id: String { rawValue }

    var label: String {
        switch self {
        case .postgres: return "PostgreSQL"
        case .mysql: return "MySQL"
        case .redis: return "Redis"
        }
    }

    var presetName: String {
        switch self {
        case .postgres: return "Local PostgreSQL"
        case .mysql: return "Local MySQL"
        case .redis: return "Local Redis"
        }
    }

    var defaultPort: Int {
        switch self {
        case .postgres: return 5432
        case .mysql: return 3306
        case .redis: return 6379
        }
    }

    var defaultUsername: String {
        switch self {
        case .postgres: return "postgres"
        case .mysql: return "root"
        case .redis: return "default"
        }
    }

    var defaultDatabase: String {
        switch self {
        case .postgres: return "postgres"
        case .mysql: return "mysql"
        case .redis: return "0"
        }
    }

    var urlPlaceholder: String {
        switch self {
        case .postgres: return "postgresql://user:password@host:5432/database"
        case .mysql: return "mysql://user:password@host:3306/database"
        case .redis: return "redis://default:password@host:6379/0"
        }
    }

    var isRedis: Bool { self == .redis }
}

enum TlsMode: String, Codable, CaseIterable, Identifiable {
    case disabled
    case preferred
    case required

    var id: String { rawValue }

    var label: String {
        switch self {
        case .disabled: return "Disabled"
        case .preferred: return "Preferred"
        case .required: return "Required"
        }
    }
}

// MARK: - Profiles

struct ConnectionProfile: Codable, Identifiable, Hashable {
    var id: String
    var name: String
    var color: String?
    var engine: DatabaseEngine
    var host: String
    var port: Int
    var username: String
    var defaultDatabase: String
    var tlsMode: TlsMode
    var caCertPath: String?
    var readOnly: Bool
    var createdAt: String?
    var updatedAt: String?

    var displayColor: String { color ?? Theme.defaultConnectionColor }
}

struct ProfileSummary: Codable {
    var profile: ConnectionProfile
}

struct SaveProfileInput: Codable {
    var id: String?
    var name: String
    var color: String?
    var engine: DatabaseEngine
    var host: String
    var port: Int
    var username: String
    var defaultDatabase: String
    var tlsMode: TlsMode
    var caCertPath: String?
    var readOnly: Bool
    var password: String?

    static func draft(for profile: ConnectionProfile?) -> SaveProfileInput {
        let engine = profile?.engine ?? .postgres
        return SaveProfileInput(
            id: profile?.id,
            name: profile?.name ?? engine.presetName,
            color: profile?.color ?? Theme.defaultConnectionColor,
            engine: engine,
            host: profile?.host ?? "localhost",
            port: profile?.port ?? engine.defaultPort,
            username: profile?.username ?? engine.defaultUsername,
            defaultDatabase: profile?.defaultDatabase ?? engine.defaultDatabase,
            tlsMode: profile?.tlsMode ?? .preferred,
            caCertPath: profile?.caCertPath,
            readOnly: profile?.readOnly ?? false,
            password: nil
        )
    }

    /// Mirrors `applyEngineDefaults` in the React UI: switching engines only
    /// replaces fields that still hold the previous engine's preset.
    mutating func applyEngineDefaults(_ engine: DatabaseEngine) {
        let previous = self.engine
        if name == previous.presetName { name = engine.presetName }
        if port == previous.defaultPort { port = engine.defaultPort }
        if username == previous.defaultUsername { username = engine.defaultUsername }
        if defaultDatabase == previous.defaultDatabase { defaultDatabase = engine.defaultDatabase }
        self.engine = engine
    }
}

struct ImportedConnection: Codable {
    var engine: DatabaseEngine
    var host: String
    var port: Int
    var username: String
    var defaultDatabase: String
    var tlsMode: TlsMode
    var password: String?
    var suggestedName: String
}

// MARK: - Schema

struct DatabaseRef: Codable, Hashable {
    var name: String
    var isTemplate: Bool
    var isConnectable: Bool
}

struct SchemaNode: Codable, Identifiable, Hashable {
    var name: String
    var kind: String
    var schema: String?
    var table: String?
    var children: [SchemaNode]

    var id: String { "\(kind)|\(schema ?? "")|\(table ?? "")|\(name)" }

    var isTable: Bool { table != nil && schema != nil }

    var badge: String {
        switch kind {
        case "table": return "T"
        case "key": return "K"
        case "view": return "V"
        default: return "S"
        }
    }
}

struct WorkspaceInfo: Codable {
    var profile: ConnectionProfile
    var databases: [DatabaseRef]
    var schema: [SchemaNode]
}

// MARK: - Tables

struct TableColumn: Codable, Hashable {
    var name: String
    var dataType: String
    var nullable: Bool
    var defaultValue: String?
    var ordinal: Int
}

struct TableMetadata: Codable, Hashable {
    var schema: String
    var table: String
    var columns: [TableColumn]
    var primaryKey: [String]
    var hasXmin: Bool
}

struct OrderSpec: Codable, Hashable {
    var column: String
    var descending: Bool
}

struct TablePageRequest: Codable {
    var profileId: String
    var schema: String
    var table: String
    var offset: Int
    var limit: Int
    var filters: [FilterCondition]
    var orderBy: OrderSpec?
    var includeTotal: Bool
}

struct FilterCondition: Codable, Hashable {
    var column: String
    var `operator`: String
    var value: String?
}

struct TablePage: Codable {
    var metadata: TableMetadata
    var columns: [String]
    var rows: [[JSONValue]]
    var totalRows: Int?
    var offset: Int
    var limit: Int
    var hasMore: Bool
}

// MARK: - Queries

struct QueryColumn: Codable, Hashable {
    var name: String
    var dataType: String
}

struct QueryResponse: Codable {
    var columns: [QueryColumn]
    var rows: [[JSONValue]]
    var rowCount: Int
    var affectedRows: Int?
    var durationMs: Double
    var truncated: Bool
    var notices: [String]
}

struct QueryHistoryEntry: Codable, Identifiable, Hashable {
    var id: String
    var profileId: String
    var database: String
    var sql: String
    var executedAt: String
    var durationMs: Double
    var success: Bool
}

// MARK: - Values

/// Arbitrary JSON, because database rows contain anything.
enum JSONValue: Codable, Hashable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else if let value = try? container.decode([String: JSONValue].self) {
            self = .object(value)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "unsupported JSON value")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case let .bool(value): try container.encode(value)
        case let .number(value): try container.encode(value)
        case let .string(value): try container.encode(value)
        case let .array(value): try container.encode(value)
        case let .object(value): try container.encode(value)
        }
    }

    var isNull: Bool {
        if case .null = self { return true }
        return false
    }

    /// Matches `toDisplayValue` in the React UI and the native frontends.
    var display: String {
        switch self {
        case .null: return "NULL"
        case let .bool(value): return value ? "true" : "false"
        case let .number(value):
            if value.rounded() == value, abs(value) < 1e15 {
                return String(Int64(value))
            }
            return String(value)
        case let .string(value): return value
        case let .array(value):
            return (try? Self.encodeJSON(value)) ?? "[]"
        case let .object(value):
            return (try? Self.encodeJSON(value)) ?? "{}"
        }
    }

    private static func encodeJSON(_ value: some Encodable) throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.withoutEscapingSlashes]
        let data = try encoder.encode(value)
        return String(decoding: data, as: UTF8.self)
    }
}

/// CSV output, mirroring `csv_document` in the shared workbench crate.
enum CSV {
    static func document(columns: [String], rows: [[JSONValue]]) -> String {
        var lines = [line(columns.map { .string($0) })]
        lines.append(contentsOf: rows.map(line))
        return lines.joined(separator: "\n")
    }

    static func line(_ values: [JSONValue]) -> String {
        values
            .map { value in
                let text = value.display
                if text.contains("\"") || text.contains(",") || text.contains("\n") || text.contains("\r") {
                    return "\"" + text.replacingOccurrences(of: "\"", with: "\"\"") + "\""
                }
                return text
            }
            .joined(separator: ",")
    }
}
