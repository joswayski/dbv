import Foundation
import SwiftUI

/// A workbench tab, mirroring the Tauri UI's `Tab`.
struct WorkbenchTab: Identifiable, Hashable {
    enum Kind: Hashable {
        case table(schema: String, table: String)
        case query(database: String)
    }

    let id: String
    var title: String
    let kind: Kind
    let profileId: String
}

/// A pending confirmation, rendered as DBM's own overlay instead of a system
/// alert.
struct ConfirmRequest: Identifiable {
    enum Action {
        case deleteProfile(ConnectionProfile)
        case runQuery(tabId: String, sql: String)
    }

    let id = UUID()
    let title: String
    let body: String
    let confirmLabel: String
    let danger: Bool
    let action: Action
}

/// The workbench state: profiles, connections, tabs, and per-tab view state.
@MainActor
final class AppModel: ObservableObject {
    @Published var profiles: [ConnectionProfile] = []
    @Published var workspaces: [String: WorkspaceInfo] = [:]
    @Published var schemas: [String: [SchemaNode]] = [:]
    @Published var tabs: [WorkbenchTab] = []
    @Published var activeTabId: String?
    @Published var activeProfileId: String?
    @Published var errorMessage: String?
    @Published var toast: String?
    @Published var refreshingSchema: String?
    @Published var confirm: ConfirmRequest?

    @Published var pages: [String: TablePage] = [:]
    @Published var pageIndex: [String: Int] = [:]
    @Published var orders: [String: OrderSpec] = [:]
    @Published var tableStatus: [String: String] = [:]
    @Published var loadingTables: Set<String> = []
    @Published var pendingRows: [String: [String: PendingRow]] = [:]
    @Published var savingTables: Set<String> = []

    @Published var sqlText: [String: String] = [:]
    /// Caret and selection per query tab, reported by the AppKit editor.
    @Published var selections: [String: NSRange] = [:]
    @Published var queries: [String: QueryResponse] = [:]
    @Published var queryMeta: [String: String] = [:]
    @Published var queryErrors: [String: String] = [:]
    @Published var histories: [String: [QueryHistoryEntry]] = [:]
    @Published var runningQueries: Set<String> = []

    private var didStart = false

    // MARK: - Lifecycle

    func start() async {
        guard !didStart else { return }
        didStart = true
        do {
            let summaries = try await Bridge.initialize()
            profiles = summaries.map(\.profile)
        } catch {
            show(error)
        }
    }

    // MARK: - Lookups

    func profile(_ id: String) -> ConnectionProfile? {
        workspaces[id]?.profile ?? profiles.first { $0.id == id }
    }

    func engine(for id: String) -> DatabaseEngine {
        profile(id)?.engine ?? .postgres
    }

    func database(for id: String) -> String {
        workspaces[id]?.profile.defaultDatabase ?? engine(for: id).defaultDatabase
    }

    func isConnected(_ id: String) -> Bool { workspaces[id] != nil }

    // MARK: - Connections

    func connect(_ id: String) async {
        do {
            let workspace: WorkspaceInfo = try await Bridge.call(
                ["op": "connect", "profileId": id],
                as: WorkspaceInfo.self
            )
            apply(workspace)
            openQuery(profileId: id)
        } catch {
            show(error)
        }
    }

    func disconnect(_ id: String) async {
        do {
            try await Bridge.call(["op": "disconnect", "profileId": id])
        } catch {
            show(error)
        }
        workspaces.removeValue(forKey: id)
        schemas.removeValue(forKey: id)
        closeTabs(for: id)
        if activeProfileId == id { activeProfileId = nil }
    }

    func select(_ profile: ConnectionProfile) async {
        activeProfileId = profile.id
        if isConnected(profile.id) {
            if let tab = tabs.last(where: { $0.profileId == profile.id }) {
                activeTabId = tab.id
            } else {
                openQuery(profileId: profile.id)
            }
        } else {
            await connect(profile.id)
        }
    }

    func switchDatabase(_ id: String, to database: String) async {
        do {
            let workspace: WorkspaceInfo = try await Bridge.call(
                ["op": "connect_database", "profileId": id, "database": database],
                as: WorkspaceInfo.self
            )
            apply(workspace)
        } catch {
            show(error)
        }
    }

    func refreshSchema(_ id: String) async {
        refreshingSchema = id
        defer { refreshingSchema = nil }
        do {
            let tree: [SchemaNode] = try await Bridge.call(
                ["op": "schema_tree", "profileId": id],
                as: [SchemaNode].self
            )
            let kind = engine(for: id).isRedis ? "Keyspace" : "Schema"
            toast = describeSchemaRefresh(previous: schemas[id] ?? [], next: tree, kind: kind)
            schemas[id] = tree
        } catch {
            show(error)
        }
    }

    private func apply(_ workspace: WorkspaceInfo) {
        let id = workspace.profile.id
        workspaces[id] = workspace
        schemas[id] = workspace.schema
        activeProfileId = id
    }

    // MARK: - Tabs

    func openTable(profileId: String, schema: String, table: String) {
        if let existing = tabs.first(where: {
            $0.profileId == profileId && $0.kind == .table(schema: schema, table: table)
        }) {
            activeTabId = existing.id
            return
        }
        let tab = WorkbenchTab(
            id: UUID().uuidString,
            title: "\(schema).\(table)",
            kind: .table(schema: schema, table: table),
            profileId: profileId
        )
        tabs.append(tab)
        activeTabId = tab.id
        activeProfileId = profileId
        pageIndex[tab.id] = 0
        Task { await loadPage(tab.id) }
    }

    func openQuery(profileId: String) {
        let engine = engine(for: profileId)
        let existingTitles = Set(tabs.filter { $0.profileId == profileId && isQuery($0) }.map(\.title))
        var number = 1
        while existingTitles.contains("Query \(number)") { number += 1 }
        let tab = WorkbenchTab(
            id: UUID().uuidString,
            title: "Query \(number)",
            kind: .query(database: database(for: profileId)),
            profileId: profileId
        )
        tabs.append(tab)
        activeTabId = tab.id
        activeProfileId = profileId
        sqlText[tab.id] = engine.isRedis ? "PING" : "SELECT now();"
        Task { await loadHistory(tab.id) }
    }

    func closeTab(_ id: String) {
        guard let index = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs.remove(at: index)
        pages.removeValue(forKey: id)
        pageIndex.removeValue(forKey: id)
        orders.removeValue(forKey: id)
        tableStatus.removeValue(forKey: id)
        pendingRows.removeValue(forKey: id)
        savingTables.remove(id)
        sqlText.removeValue(forKey: id)
        selections.removeValue(forKey: id)
        queries.removeValue(forKey: id)
        queryMeta.removeValue(forKey: id)
        queryErrors.removeValue(forKey: id)
        histories.removeValue(forKey: id)
        if activeTabId == id {
            let next = tabs.indices.contains(index) ? tabs[index] : tabs.last
            activeTabId = next?.id
            if let next { activeProfileId = next.profileId }
        }
    }

    private func closeTabs(for profileId: String) {
        for tab in tabs.filter({ $0.profileId == profileId }) {
            closeTab(tab.id)
        }
    }

    private func isQuery(_ tab: WorkbenchTab) -> Bool {
        if case .query = tab.kind { return true }
        return false
    }

    // MARK: - Table pages

    func loadPage(_ tabId: String, whileSaving: Bool = false) async {
        guard let tab = tabs.first(where: { $0.id == tabId }),
              case let .table(schema, table) = tab.kind
        else { return }
        guard !loadingTables.contains(tabId), whileSaving || !savingTables.contains(tabId) else { return }
        loadingTables.insert(tabId)
        tableStatus[tabId] = "Loading…"
        defer { loadingTables.remove(tabId) }
        let index = pageIndex[tabId] ?? 0
        let request = TablePageRequest(
            profileId: tab.profileId,
            schema: schema,
            table: table,
            offset: index * 200,
            limit: 200,
            filters: [],
            orderBy: orders[tabId],
            includeTotal: true
        )
        do {
            let page: TablePage = try await Bridge.call(
                ["op": "table_page", "request": try request.jsonObject()],
                as: TablePage.self
            )
            pages[tabId] = page
            let total = page.totalRows.map { "\($0) rows" } ?? "\(page.rows.count) rows on this page"
            let first = page.rows.isEmpty ? 0 : page.offset + 1
            tableStatus[tabId] = "\(total) · showing rows \(first)–\(page.offset + page.rows.count)"
        } catch {
            tableStatus[tabId] = "Load failed."
            show(error)
        }
    }

    func changePage(_ tabId: String, delta: Int) async {
        guard !loadingTables.contains(tabId), !savingTables.contains(tabId) else { return }
        let current = pageIndex[tabId] ?? 0
        pageIndex[tabId] = max(0, current + delta)
        await loadPage(tabId)
    }

    func sort(_ tabId: String, column: String) async {
        guard !loadingTables.contains(tabId), !savingTables.contains(tabId) else { return }
        let existing = orders[tabId]
        let descending = existing?.column == column && existing?.descending == false
        orders[tabId] = OrderSpec(column: column, descending: descending)
        pageIndex[tabId] = 0
        await loadPage(tabId)
    }

    func rowKey(tabId: String, row: [JSONValue], fallbackIndex: Int) -> String {
        guard let metadata = pages[tabId]?.metadata, !metadata.primaryKey.isEmpty else {
            return "row:\(fallbackIndex):\(encodedKey(row))"
        }
        let values = metadata.primaryKey.map { key -> JSONValue in
            guard let index = metadata.columns.firstIndex(where: { $0.name == key }), index < row.count else {
                return .null
            }
            return row[index]
        }
        return "pk:\(encodedKey(values))"
    }

    func displayValues(_ tabId: String, row: [JSONValue], fallbackIndex: Int) -> [JSONValue] {
        let visibleCount = pages[tabId]?.metadata.columns.count ?? row.count
        let key = rowKey(tabId: tabId, row: row, fallbackIndex: fallbackIndex)
        return pendingRows[tabId]?[key]?.changes ?? Array(row.prefix(visibleCount))
    }

    func stageCell(_ tabId: String, row: [JSONValue], fallbackIndex: Int, column: Int, value: JSONValue) {
        guard let page = pages[tabId],
              !loadingTables.contains(tabId),
              !savingTables.contains(tabId),
              !page.metadata.primaryKey.isEmpty,
              column < page.metadata.columns.count,
              !page.metadata.primaryKey.contains(page.metadata.columns[column].name),
              profile(tabs.first(where: { $0.id == tabId })?.profileId ?? "")?.readOnly == false
        else { return }
        let key = rowKey(tabId: tabId, row: row, fallbackIndex: fallbackIndex)
        var tabPending = pendingRows[tabId] ?? [:]
        guard tabPending[key]?.deleted != true else { return }
        let basePending: PendingRow
        if let existing = tabPending[key] {
            basePending = existing
        } else if let created = pendingFromRow(page: page, row: row) {
            basePending = created
        } else {
            errorMessage = "This PostgreSQL row is missing its concurrency value; refresh before editing."
            return
        }
        var pending = basePending
        guard !page.metadata.hasXmin || pending.xmin != nil else {
            errorMessage = "This PostgreSQL row is missing its concurrency value; refresh before editing."
            return
        }
        pending.changes[column] = value
        if !pending.deleted, pending.changes == pending.original {
            tabPending.removeValue(forKey: key)
        } else {
            tabPending[key] = pending
        }
        pendingRows[tabId] = tabPending
    }

    func setRowsDeleted(_ tabId: String, rows: [(row: [JSONValue], fallbackIndex: Int)], deleted: Bool) {
        guard let page = pages[tabId],
              !loadingTables.contains(tabId),
              !savingTables.contains(tabId),
              !page.metadata.primaryKey.isEmpty,
              profile(tabs.first(where: { $0.id == tabId })?.profileId ?? "")?.readOnly == false
        else { return }
        var tabPending = pendingRows[tabId] ?? [:]
        for entry in rows {
            let key = rowKey(tabId: tabId, row: entry.row, fallbackIndex: entry.fallbackIndex)
            let pending: PendingRow?
            if let existing = tabPending[key] {
                pending = existing
            } else {
                pending = pendingFromRow(page: page, row: entry.row)
            }
            guard var pending else {
                errorMessage = "This PostgreSQL row is missing its concurrency value; refresh before deleting."
                continue
            }
            guard !page.metadata.hasXmin || pending.xmin != nil else {
                errorMessage = "This PostgreSQL row is missing its concurrency value; refresh before deleting."
                continue
            }
            pending.deleted = deleted
            if !deleted, pending.changes == pending.original {
                tabPending.removeValue(forKey: key)
            } else {
                tabPending[key] = pending
            }
        }
        pendingRows[tabId] = tabPending
    }

    func discardPendingRow(_ tabId: String, key: String) {
        guard !loadingTables.contains(tabId), !savingTables.contains(tabId) else { return }
        guard var tabPending = pendingRows[tabId], let pending = tabPending[key] else { return }
        if pending.deleted, pending.changes != pending.original {
            tabPending[key]?.deleted = false
        } else {
            tabPending.removeValue(forKey: key)
        }
        pendingRows[tabId] = tabPending
    }

    func discardPendingRows(_ tabId: String) {
        guard !loadingTables.contains(tabId), !savingTables.contains(tabId) else { return }
        pendingRows[tabId] = [:]
    }

    func savePendingRows(_ tabId: String) async -> Bool {
        guard let tab = tabs.first(where: { $0.id == tabId }),
              case let .table(schema, table) = tab.kind,
              !loadingTables.contains(tabId),
              !savingTables.contains(tabId),
              let pending = pendingRows[tabId],
              !pending.isEmpty
        else { return false }
        if pages[tabId]?.metadata.hasXmin == true, pending.values.contains(where: { $0.xmin == nil }) {
            errorMessage = "A pending PostgreSQL row is missing its concurrency value; discard and refresh before saving."
            return false
        }
        let batch = MutationBatch(
            profileId: tab.profileId,
            schema: schema,
            table: table,
            mutations: pending.values.map {
                RowMutation(
                    original: $0.original,
                    changes: $0.changes,
                    primaryKey: $0.primaryKey,
                    xmin: $0.xmin,
                    deleted: $0.deleted
                )
            }
        )
        savingTables.insert(tabId)
        defer { savingTables.remove(tabId) }
        do {
            let result: MutationResult = try await Bridge.call(
                ["op": "apply_mutations", "batch": try batch.jsonObject()],
                as: MutationResult.self
            )
            pendingRows[tabId] = [:]
            await loadPage(tabId, whileSaving: true)
            if result.conflicts.isEmpty {
                toast = "\(result.applied) \(result.applied == 1 ? "change" : "changes") saved."
            } else {
                errorMessage = "\(result.conflicts.count) row conflict(s); the table was refreshed."
            }
            return true
        } catch {
            // Keep every draft after transport, validation, or database errors.
            show(error)
            return false
        }
    }

    func copyCSV(_ tabId: String) {
        guard let page = pages[tabId],
              !loadingTables.contains(tabId),
              !savingTables.contains(tabId)
        else { return }
        let rows = page.rows.enumerated().compactMap { index, row -> [JSONValue]? in
            let fallbackIndex = page.offset + index
            let key = rowKey(tabId: tabId, row: row, fallbackIndex: fallbackIndex)
            if pendingRows[tabId]?[key]?.deleted == true { return nil }
            return displayValues(tabId, row: row, fallbackIndex: fallbackIndex)
        }
        let document = CSV.document(columns: page.metadata.columns.map(\.name), rows: rows)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(document, forType: .string)
        toast = "Copied \(rows.count) visible \(rows.count == 1 ? "row" : "rows") as CSV."
    }

    func exportCSV(_ tabId: String, path: String) async {
        guard !loadingTables.contains(tabId), !savingTables.contains(tabId) else { return }
        guard pendingRows[tabId]?.isEmpty != false else {
            errorMessage = "Save or discard pending row changes before exporting."
            return
        }
        guard let tab = tabs.first(where: { $0.id == tabId }),
              case let .table(schema, table) = tab.kind
        else { return }
        let request = TablePageRequest(
            profileId: tab.profileId,
            schema: schema,
            table: table,
            offset: 0,
            limit: 200,
            filters: [],
            orderBy: orders[tabId],
            includeTotal: false
        )
        do {
            let rows: Int = try await Bridge.call(
                ["op": "export_csv", "request": try request.jsonObject(), "path": path],
                as: Int.self
            )
            toast = "Exported \(rows) rows."
        } catch {
            show(error)
        }
    }

    // MARK: - Queries

    func runQuery(_ tabId: String) async {
        guard let tab = tabs.first(where: { $0.id == tabId }),
              case let .query(database) = tab.kind
        else { return }
        let text = sqlText[tabId] ?? ""
        let engine = engine(for: tab.profileId)
        let sql = selectedOrCurrentStatement(text, tabId: tabId, engine: engine)
        guard !sql.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        if requiresConfirmation(sql, engine: engine) {
            confirm = ConfirmRequest(
                title: "Run destructive statement",
                body: "This query may change or remove many rows. Run it anyway?",
                confirmLabel: "Run",
                danger: true,
                action: .runQuery(tabId: tabId, sql: sql)
            )
            return
        }
        let target = await resolveTableSelect(sql, profileId: tab.profileId, engine: engine)
        await execute(tabId: tabId, sql: sql, database: database)
        // `SELECT * FROM table` opens the full table view, matching the Tauri
        // workbench, so the result is browsable and filterable.
        if let target, queryErrors[tabId] == nil {
            openTable(profileId: tab.profileId, schema: target.schema, table: target.table)
        }
    }

    /// Asks the shared workbench crate whether the statement names one table.
    private func resolveTableSelect(
        _ sql: String,
        profileId: String,
        engine: DatabaseEngine
    ) async -> (schema: String, table: String)? {
        guard !engine.isRedis else { return nil }
        var tables: [[String: String]] = []
        func visit(_ nodes: [SchemaNode]) {
            for node in nodes {
                if node.kind == "table", let schema = node.schema, let table = node.table {
                    tables.append(["schema": schema, "table": table])
                }
                visit(node.children)
            }
        }
        visit(schemas[profileId] ?? [])
        guard !tables.isEmpty else { return nil }
        return try? await Bridge.resolveTableSelect(sql: sql, tables: tables)
    }

    func execute(tabId: String, sql: String, database: String) async {
        guard let tab = tabs.first(where: { $0.id == tabId }) else { return }
        runningQueries.insert(tabId)
        queryErrors[tabId] = nil
        queryMeta[tabId] = "Running…"
        defer { runningQueries.remove(tabId) }
        do {
            let response: QueryResponse = try await Bridge.call(
                [
                    "op": "run_query",
                    "profileId": tab.profileId,
                    "sql": sql,
                    "maxRows": 10_000,
                ],
                as: QueryResponse.self
            )
            queries[tabId] = response
            var meta = "\(response.rowCount) rows"
            if let affected = response.affectedRows { meta += " · \(affected) affected" }
            meta += " · \(Int(response.durationMs)) ms"
            if response.truncated { meta += " · truncated" }
            queryMeta[tabId] = meta
        } catch {
            queryMeta[tabId] = "Statement failed."
            queryErrors[tabId] = error.localizedDescription
        }
        await loadHistory(tabId)
    }

    func loadHistory(_ tabId: String) async {
        guard let tab = tabs.first(where: { $0.id == tabId }),
              case let .query(database) = tab.kind
        else { return }
        do {
            let entries: [QueryHistoryEntry] = try await Bridge.call(
                ["op": "history", "profileId": tab.profileId, "database": database, "limit": 100],
                as: [QueryHistoryEntry].self
            )
            histories[tabId] = entries
        } catch {
            // History is best effort; a failure must not interrupt the workbench.
        }
    }

    func useHistory(_ tabId: String, entry: QueryHistoryEntry) {
        sqlText[tabId] = entry.sql
        selections[tabId] = NSRange(location: (entry.sql as NSString).length, length: 0)
    }

    /// The AppKit editor reports the caret and selection here.
    func setSelection(_ tabId: String, _ range: NSRange) {
        selections[tabId] = range
    }

    // MARK: - Profiles

    func testProfile(_ input: SaveProfileInput) async -> String? {
        do {
            var request: [String: Any] = ["op": "test_profile", "input": try input.jsonObject()]
            if let password = input.password { request["password"] = password }
            try await Bridge.call(request)
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    func saveProfile(_ input: SaveProfileInput) async -> Bool {
        do {
            var request: [String: Any] = ["op": "save_profile", "input": try input.jsonObject()]
            if let password = input.password { request["password"] = password }
            let saved: ConnectionProfile = try await Bridge.call(request, as: ConnectionProfile.self)
            await reloadProfiles()
            await connect(saved.id)
            return true
        } catch {
            show(error)
            return false
        }
    }

    func deleteProfile(_ profile: ConnectionProfile) async {
        do {
            try await Bridge.call(["op": "delete_profile", "profileId": profile.id])
        } catch {
            show(error)
        }
        workspaces.removeValue(forKey: profile.id)
        schemas.removeValue(forKey: profile.id)
        closeTabs(for: profile.id)
        if activeProfileId == profile.id { activeProfileId = nil }
        await reloadProfiles()
        toast = "Connection deleted."
    }

    func importURL(_ url: String) async -> ImportedConnection? {
        do {
            return try await Bridge.call(["op": "import_url", "url": url], as: ImportedConnection.self)
        } catch {
            show(error)
            return nil
        }
    }

    private func reloadProfiles() async {
        do {
            let summaries = try await Bridge.initialize()
            profiles = summaries.map(\.profile)
        } catch {
            show(error)
        }
    }

    // MARK: - Confirmation

    func perform(_ request: ConfirmRequest) async {
        confirm = nil
        switch request.action {
        case let .deleteProfile(profile):
            await deleteProfile(profile)
        case let .runQuery(tabId, sql):
            guard let tab = tabs.first(where: { $0.id == tabId }),
                  case let .query(database) = tab.kind
            else { return }
            let target = await resolveTableSelect(
                sql,
                profileId: tab.profileId,
                engine: engine(for: tab.profileId)
            )
            await execute(tabId: tabId, sql: sql, database: database)
            if let target, queryErrors[tabId] == nil {
                openTable(profileId: tab.profileId, schema: target.schema, table: target.table)
            }
        }
    }

    // MARK: - Helpers

    func show(_ error: Error) {
        errorMessage = error.localizedDescription
    }

    private func pendingFromRow(page: TablePage, row: [JSONValue]) -> PendingRow? {
        let original = Array(row.prefix(page.metadata.columns.count))
        let primaryKey = page.metadata.primaryKey.map { key -> JSONValue in
            guard let index = page.metadata.columns.firstIndex(where: { $0.name == key }), index < original.count else {
                return .null
            }
            return original[index]
        }
        let xmin: String?
        if page.metadata.hasXmin, row.count > page.metadata.columns.count {
            switch row[page.metadata.columns.count] {
            case .null: xmin = nil
            default: xmin = row[page.metadata.columns.count].display
            }
        } else {
            xmin = nil
        }
        guard !page.metadata.hasXmin || xmin != nil else { return nil }
        return PendingRow(
            original: original,
            changes: original,
            primaryKey: primaryKey,
            xmin: xmin,
            deleted: false
        )
    }

    private func encodedKey(_ values: [JSONValue]) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        guard let data = try? encoder.encode(values) else { return values.map(\.display).joined(separator: "|") }
        return data.base64EncodedString()
    }

    /// Statement targeting, mirroring `sql_target.rs` from the shared crate.
    private func selectedOrCurrentStatement(_ text: String, tabId: String, engine: DatabaseEngine) -> String {
        let selection = selections[tabId] ?? NSRange(location: (text as NSString).length, length: 0)
        if selection.length > 0, let range = Range(selection, in: text) {
            let selected = String(text[range]).trimmingCharacters(in: .whitespacesAndNewlines)
            if !selected.isEmpty { return selected }
        }
        let cursor = Range(selection, in: text).map { text.distance(from: text.startIndex, to: $0.lowerBound) }
            ?? text.count
        let target = engine.isRedis
            ? lineExecutionTarget(text, cursor: cursor)
            : statementExecutionTarget(text, cursor: cursor)
        return target ?? text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

// MARK: - Statement targeting

/// Ports the statement splitter from `crates/dbm-workbench/src/sql_target.rs`.
func statementExecutionTarget(_ text: String, cursor: Int) -> String? {
    let characters = Array(text)
    let ranges = statementRanges(characters)
    guard !ranges.isEmpty else { return nil }
    let clamped = max(0, min(cursor, characters.count))
    var index = ranges.firstIndex { clamped < $0.upperBound } ?? ranges.count - 1
    if clamped > 0, characters[clamped - 1] == ";", index > 0 { index -= 1 }
    if let trimmed = trimRange(characters, ranges[index]) { return String(trimmed) }
    for range in ranges[(index + 1)...] {
        if let trimmed = trimRange(characters, range) { return String(trimmed) }
    }
    return nil
}

/// Redis commands run per line.
func lineExecutionTarget(_ text: String, cursor: Int) -> String? {
    let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
    guard !lines.isEmpty else { return nil }
    var index = lines.count - 1
    var offset = 0
    for (lineIndex, line) in lines.enumerated() {
        offset += line.count + 1
        if cursor < offset {
            index = lineIndex
            break
        }
    }
    for candidate in [index] + Array((0..<index).reversed()) + Array((index + 1)..<lines.count) {
        let line = lines[candidate].trimmingCharacters(in: .whitespaces)
        if !line.isEmpty { return line }
    }
    return nil
}

private func statementRanges(_ characters: [Character]) -> [Range<Int>] {
    var ranges: [Range<Int>] = []
    var start = 0
    var index = 0
    var singleQuoted = false
    var doubleQuoted = false
    var lineComment = false
    var blockDepth = 0
    var dollarQuote: String?

    while index < characters.count {
        let character = characters[index]
        let next = index + 1 < characters.count ? characters[index + 1] : nil

        if lineComment {
            if character == "\n" { lineComment = false }
            index += 1
            continue
        }
        if blockDepth > 0 {
            if character == "/", next == "*" { blockDepth += 1; index += 2 }
            else if character == "*", next == "/" { blockDepth -= 1; index += 2 }
            else { index += 1 }
            continue
        }
        if let delimiter = dollarQuote {
            if startsWith(characters, at: index, needle: delimiter) {
                index += delimiter.count
                dollarQuote = nil
            } else {
                index += 1
            }
            continue
        }
        if singleQuoted {
            if character == "\\" || (character == "'" && next == "'") { index += 2 }
            else {
                if character == "'" { singleQuoted = false }
                index += 1
            }
            continue
        }
        if doubleQuoted {
            if character == "\"", next == "\"" { index += 2 }
            else {
                if character == "\"" { doubleQuoted = false }
                index += 1
            }
            continue
        }
        if character == "-", next == "-" { lineComment = true; index += 2 }
        else if character == "/", next == "*" { blockDepth = 1; index += 2 }
        else if character == "'" { singleQuoted = true; index += 1 }
        else if character == "\"" { doubleQuoted = true; index += 1 }
        else if character == "$" {
            if let delimiter = dollarDelimiter(characters, at: index) {
                index += delimiter.count
                dollarQuote = delimiter
            } else {
                index += 1
            }
        } else if character == ";" {
            ranges.append(start..<(index + 1))
            start = index + 1
            index += 1
        } else {
            index += 1
        }
    }
    ranges.append(start..<characters.count)
    return ranges
}

private func startsWith(_ characters: [Character], at index: Int, needle: String) -> Bool {
    let needleCharacters = Array(needle)
    guard index + needleCharacters.count <= characters.count else { return false }
    return Array(characters[index..<(index + needleCharacters.count)]) == needleCharacters
}

private func dollarDelimiter(_ characters: [Character], at index: Int) -> String? {
    guard let end = characters[(index + 1)...].firstIndex(of: "$") else { return nil }
    let tag = String(characters[(index + 1)..<end])
    let valid = tag.isEmpty
        || (tag.first.map { $0.isLetter || $0 == "_" } == true
            && tag.allSatisfy { $0.isLetter || $0.isNumber || $0 == "_" })
    guard valid else { return nil }
    return String(characters[index...end])
}

private func trimRange(_ characters: [Character], _ range: Range<Int>) -> ArraySlice<Character>? {
    var lower = range.lowerBound
    var upper = range.upperBound
    while lower < upper, characters[lower].isWhitespace { lower += 1 }
    while upper > lower, characters[upper - 1].isWhitespace { upper -= 1 }
    return lower < upper ? characters[lower..<upper] : nil
}

/// Destructive statements ask before running, matching the other frontends.
func requiresConfirmation(_ sql: String, engine: DatabaseEngine) -> Bool {
    if engine.isRedis {
        let first = sql.split(whereSeparator: \.isWhitespace).first?.lowercased()
        return first == "flushall" || first == "flushdb"
    }
    let stripped = stripComments(sql).lowercased()
    let tokens = stripped.split(whereSeparator: { !$0.isLetter && !$0.isNumber && $0 != "_" })
    func has(_ word: String) -> Bool { tokens.contains(where: { $0 == word }) }
    return has("drop") || has("truncate") || ((has("delete") || has("update")) && !has("where"))
}

private func stripComments(_ sql: String) -> String {
    var result = ""
    let characters = Array(sql)
    var index = 0
    var singleQuoted = false
    var doubleQuoted = false
    while index < characters.count {
        let character = characters[index]
        let next = index + 1 < characters.count ? characters[index + 1] : nil
        if singleQuoted {
            result.append(character)
            if character == "\\", let next { result.append(next); index += 2; continue }
            if character == "'", next == "'" { result.append("'"); index += 2; continue }
            if character == "'" { singleQuoted = false }
            index += 1
            continue
        }
        if doubleQuoted {
            result.append(character)
            if character == "\"", next == "\"" { result.append("\""); index += 2; continue }
            if character == "\"" { doubleQuoted = false }
            index += 1
            continue
        }
        if character == "-", next == "-" {
            while index < characters.count, characters[index] != "\n" { index += 1 }
            continue
        }
        if character == "/", next == "*" {
            index += 2
            while index < characters.count {
                if characters[index] == "*", index + 1 < characters.count, characters[index + 1] == "/" {
                    index += 2
                    break
                }
                index += 1
            }
            result.append(" ")
            continue
        }
        if character == "'" { singleQuoted = true }
        if character == "\"" { doubleQuoted = true }
        result.append(character)
        index += 1
    }
    return result
}

/// Summarizes what a schema refresh changed, matching `describe_schema_refresh`.
func describeSchemaRefresh(previous: [SchemaNode], next: [SchemaNode], kind: String) -> String {
    let previousObjects = schemaObjects(previous)
    let nextObjects = schemaObjects(next)
    let previousKeys = Set(previousObjects.map(\.key))
    let nextKeys = Set(nextObjects.map(\.key))
    let added = nextObjects.filter { !previousKeys.contains($0.key) }.map(\.label)
    let removed = previousObjects.filter { !nextKeys.contains($0.key) }.map(\.label)
    if added.isEmpty, removed.isEmpty { return "\(kind) is already up to date." }
    var changes: [String] = []
    if !added.isEmpty { changes.append("Added \(summarize(added))") }
    if !removed.isEmpty { changes.append("Removed \(summarize(removed))") }
    return "\(kind) refreshed · " + changes.joined(separator: " · ") + "."
}

private func schemaObjects(_ nodes: [SchemaNode]) -> [(key: String, label: String)] {
    var objects: [(key: String, label: String)] = []
    func visit(_ node: SchemaNode) {
        let qualified = node.schema != nil && node.table != nil
            ? "\(node.schema!).\(node.table!)"
            : node.name
        objects.append((key: "\(node.kind):\(qualified)", label: "\(node.kind) \(qualified)"))
        node.children.forEach(visit)
    }
    nodes.forEach(visit)
    return objects.sorted { $0.key < $1.key }
}

private func summarize(_ labels: [String]) -> String {
    if labels.count == 1 { return labels[0] }
    let visible = labels.prefix(3).joined(separator: ", ")
    let remainder = labels.count - min(3, labels.count)
    return remainder > 0
        ? "\(labels.count) objects: \(visible), and \(remainder) more"
        : "\(labels.count) objects: \(visible)"
}
