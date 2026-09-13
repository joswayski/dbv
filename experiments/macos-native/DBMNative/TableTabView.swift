import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct TableTabView: View {
    @EnvironmentObject private var model: AppModel
    @State private var selectedRows: Set<String> = []
    @State private var selectionAnchor: Int?
    @State private var editing: TableCellEditing?
    let tab: WorkbenchTab

    private var schema: String {
        if case let .table(schema, _) = tab.kind { return schema }
        return ""
    }

    private var table: String {
        if case let .table(_, table) = tab.kind { return table }
        return ""
    }

    private var page: TablePage? { model.pages[tab.id] }
    private var pending: [String: PendingRow] { model.pendingRows[tab.id] ?? [:] }
    private var pendingCount: Int { pending.count }
    private var pendingDeleteCount: Int { pending.values.filter(\.deleted).count }
    private var pendingEditCount: Int { pendingCount - pendingDeleteCount }
    private var saving: Bool { model.savingTables.contains(tab.id) }
    private var loading: Bool { model.loadingTables.contains(tab.id) }
    private var busy: Bool { saving || loading }
    private var editable: Bool {
        page?.metadata.primaryKey.isEmpty == false && model.profile(tab.profileId)?.readOnly == false
    }

    private var entries: [(key: String, row: [JSONValue], fallbackIndex: Int)] {
        guard let page else { return [] }
        return page.rows.enumerated().map { index, row in
            let fallbackIndex = page.offset + index
            return (
                model.rowKey(tabId: tab.id, row: row, fallbackIndex: fallbackIndex),
                row,
                fallbackIndex
            )
        }
    }

    private var selectedEntries: [(key: String, row: [JSONValue], fallbackIndex: Int)] {
        entries.filter { selectedRows.contains($0.key) }
    }

    private var allSelectedDeleted: Bool {
        !selectedEntries.isEmpty && selectedEntries.allSatisfy { pending[$0.key]?.deleted == true }
    }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            statusLine
            if pendingCount > 0 { pendingBanner }
            grid
            pagination
        }
        .padding(.horizontal, 22)
        .padding(.top, 18)
        .padding(.bottom, 24)
        .onChange(of: page?.offset) { _ in
            editing = nil
            selectedRows = []
            selectionAnchor = nil
        }
        .onChange(of: busy) { isBusy in
            if isBusy { editing = nil }
        }
        .onDisappear { commitEditing() }
    }

    private var toolbar: some View {
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 0) {
                Text("TABLE VIEWER")
                    .font(Theme.eyebrowFont)
                    .foregroundStyle(Theme.muted)
                Text("\(schema).\(table)")
                    .font(Theme.titleFont)
            }
            Spacer()
            if let order = model.orders[tab.id] {
                Text("Ordered by \(order.column)\(order.descending ? " ↓" : " ↑")")
                    .font(Theme.smallFont)
                    .foregroundStyle(Theme.muted)
            }
            if !selectedEntries.isEmpty, editable {
                Button(selectionActionLabel) {
                    commitEditing()
                    model.setRowsDeleted(
                        tab.id,
                        rows: selectedEntries.map { ($0.row, $0.fallbackIndex) },
                        deleted: !allSelectedDeleted
                    )
                }
                .buttonStyle(DBMButtonStyle(kind: allSelectedDeleted ? .secondary : .danger))
                .disabled(busy)
            }
            Button("Copy visible (\(copyableRowCount))") {
                commitEditing()
                model.copyCSV(tab.id)
            }
                .buttonStyle(DBMButtonStyle(kind: .secondary))
                .disabled(page == nil || busy)
            Button(exportLabel) {
                commitEditing()
                exportCSV()
            }
                .buttonStyle(DBMButtonStyle(kind: .secondary))
                .disabled(page == nil || busy || pendingCount > 0)
            Button("Refresh") {
                commitEditing()
                guard pendingCount == 0 else { return }
                Task { await model.loadPage(tab.id) }
            }
                .buttonStyle(DBMButtonStyle(kind: .secondary))
                .disabled(busy || pendingCount > 0)
            if pendingCount > 0 {
                Button(saving ? "Saving…" : "Save changes (\(pendingCount))") {
                    commitEditing()
                    Task {
                        if await model.savePendingRows(tab.id) {
                            selectedRows = []
                            selectionAnchor = nil
                        }
                    }
                }
                .buttonStyle(DBMButtonStyle(kind: .primary))
                .disabled(busy)
            }
        }
        .padding(.horizontal, 0)
        .padding(.vertical, 9)
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.border).frame(height: 1) }
    }

    private var statusLine: some View {
        HStack(spacing: 8) {
            Text(model.tableStatus[tab.id] ?? "Loading…")
                .font(Theme.smallFont)
                .foregroundStyle(Theme.muted)
            if model.profile(tab.profileId)?.readOnly == true {
                Text("READ ONLY")
                    .font(Theme.eyebrowFont)
                    .foregroundStyle(Theme.muted)
            } else if page?.metadata.primaryKey.isEmpty == true {
                Text("A primary key is required for editing")
                    .font(Theme.smallFont)
                    .foregroundStyle(Theme.muted)
            }
            Spacer()
        }
        .padding(.horizontal, 0)
        .padding(.vertical, 6)
    }

    private var pendingBanner: some View {
        HStack(spacing: 8) {
            Text("\(pendingCount) pending \(pendingCount == 1 ? "change" : "changes")")
                .font(Theme.smallFont.weight(.bold))
                .foregroundStyle(Theme.warning)
            if pendingEditCount > 0 {
                PendingCountBadge(text: "\(pendingEditCount) \(pendingEditCount == 1 ? "edited row" : "edited rows")", danger: false)
            }
            if pendingDeleteCount > 0 {
                PendingCountBadge(text: "\(pendingDeleteCount) \(pendingDeleteCount == 1 ? "deletion" : "deletions")", danger: true)
            }
            Spacer()
            Button("Discard changes") {
                editing = nil
                model.discardPendingRows(tab.id)
            }
                .buttonStyle(DBMButtonStyle(kind: .secondary))
                .disabled(busy)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(Theme.warning.opacity(0.06))
        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Theme.warning.opacity(0.28)))
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .padding(.bottom, 10)
    }

    @ViewBuilder
    private var grid: some View {
        if let page {
            DataGrid(
                columns: page.metadata.columns.map { (name: $0.name, dataType: $0.dataType) },
                rows: entries.map { model.displayValues(tab.id, row: $0.row, fallbackIndex: $0.fallbackIndex) },
                sortColumn: model.orders[tab.id]?.column,
                descending: model.orders[tab.id]?.descending ?? false,
                onSort: { column in
                    commitEditing()
                    selectedRows = []
                    selectionAnchor = nil
                    Task { await model.sort(tab.id, column: column) }
                },
                rowKeys: entries.map { $0.key },
                pendingRows: pending,
                primaryKey: Set(page.metadata.primaryKey),
                editable: editable,
                interactionDisabled: busy,
                selectedRows: selectedRows,
                tableEditing: $editing,
                onSelect: selectRow,
                onEdit: { rowKey, columnIndex, value in
                    guard let entry = entries.first(where: { $0.key == rowKey }) else { return }
                    model.stageCell(
                        tab.id,
                        row: entry.row,
                        fallbackIndex: entry.fallbackIndex,
                        column: columnIndex,
                        value: value
                    )
                },
                onDiscard: { key in
                    editing = nil
                    model.discardPendingRow(tab.id, key: key)
                }
            )
        } else {
            Text(model.loadingTables.contains(tab.id) ? "Loading…" : "No rows match this view.")
                .font(Theme.uiFont)
                .foregroundStyle(Theme.muted)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var pagination: some View {
        let index = model.pageIndex[tab.id] ?? 0
        let hasMore = page?.hasMore ?? false
        return HStack(spacing: 8) {
            if index > 0 {
                Button("← Previous") {
                    commitEditing()
                    Task { await model.changePage(tab.id, delta: -1) }
                }
                    .buttonStyle(DBMButtonStyle(kind: .secondary))
                    .disabled(busy)
            }
            Text("Page \(index + 1)")
                .font(Theme.smallFont)
                .foregroundStyle(Theme.muted)
            if hasMore {
                Button("Next →") {
                    commitEditing()
                    Task { await model.changePage(tab.id, delta: 1) }
                }
                    .buttonStyle(DBMButtonStyle(kind: .secondary))
                    .disabled(busy)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
        .overlay(alignment: .top) { Rectangle().fill(Theme.border).frame(height: 1) }
    }

    private var copyableRowCount: Int {
        entries.filter { pending[$0.key]?.deleted != true }.count
    }

    private var exportLabel: String {
        guard let total = page?.totalRows else { return "Export all" }
        return "Export all (\(total.formatted()))"
    }

    private var selectionActionLabel: String {
        if allSelectedDeleted { return "Undo staged deletion" }
        return selectedEntries.count == 1 ? "Stage row for deletion" : "Stage \(selectedEntries.count) rows for deletion"
    }

    private func selectRow(_ rowIndex: Int, _ key: String, _ modifiers: NSEvent.ModifierFlags) {
        let command = modifiers.contains(.command) || modifiers.contains(.control)
        if modifiers.contains(.shift), let selectionAnchor {
            var next = command ? selectedRows : []
            let lower = min(selectionAnchor, rowIndex)
            let upper = max(selectionAnchor, rowIndex)
            for entry in entries[lower...upper] { next.insert(entry.key) }
            selectedRows = next
        } else if command {
            if selectedRows.contains(key) { selectedRows.remove(key) } else { selectedRows.insert(key) }
        } else {
            selectedRows = [key]
        }
        selectionAnchor = rowIndex
    }

    private func commitEditing() {
        guard let editing else { return }
        defer { self.editing = nil }
        guard let entry = entries.first(where: { $0.key == editing.rowKey }) else { return }
        model.stageCell(
            tab.id,
            row: entry.row,
            fallbackIndex: entry.fallbackIndex,
            column: editing.column,
            value: parseCellValue(editing.value)
        )
    }

    private func exportCSV() {
        guard pendingCount == 0 else { return }
        let panel = NSSavePanel()
        panel.title = "Export CSV"
        panel.nameFieldStringValue = "\(Workbench.safeFileName("\(schema).\(table)")).csv"
        panel.allowedContentTypes = [UTType.commaSeparatedText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await model.exportCSV(tab.id, path: url.path) }
    }
}

private struct PendingCountBadge: View {
    let text: String
    let danger: Bool

    var body: some View {
        Text(text)
            .font(.system(size: 9, weight: .medium))
            .foregroundStyle(danger ? Theme.danger : Theme.warning)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background((danger ? Theme.danger : Theme.warning).opacity(0.08))
            .overlay(Capsule().strokeBorder((danger ? Theme.danger : Theme.warning).opacity(0.34)))
            .clipShape(Capsule())
    }
}
