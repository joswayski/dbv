import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct TableTabView: View {
    @EnvironmentObject private var model: AppModel
    let tab: WorkbenchTab

    private var schema: String {
        if case let .table(schema, _) = tab.kind { return schema }
        return ""
    }

    private var table: String {
        if case let .table(_, table) = tab.kind { return table }
        return ""
    }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            statusLine
            grid
            pagination
        }
        .padding(.horizontal, 22)
        .padding(.top, 18)
        .padding(.bottom, 24)
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
            Button("Copy CSV") { model.copyCSV(tab.id) }
                .buttonStyle(DBMButtonStyle(kind: .secondary))
            Button("Export CSV") { exportCSV() }
                .buttonStyle(DBMButtonStyle(kind: .secondary))
            Button("Refresh") { Task { await model.loadPage(tab.id) } }
                .buttonStyle(DBMButtonStyle(kind: .secondary))
        }
        .padding(.horizontal, 0)
        .padding(.vertical, 9)
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.border).frame(height: 1) }
    }

    private var statusLine: some View {
        HStack {
            Text(model.tableStatus[tab.id] ?? "Loading…")
                .font(Theme.smallFont)
                .foregroundStyle(Theme.muted)
            Spacer()
        }
        .padding(.horizontal, 0)
        .padding(.vertical, 6)
    }

    @ViewBuilder
    private var grid: some View {
        if let page = model.pages[tab.id] {
            DataGrid(
                columns: page.metadata.columns.map { (name: $0.name, dataType: $0.dataType) },
                rows: page.rows,
                sortColumn: model.orders[tab.id]?.column,
                descending: model.orders[tab.id]?.descending ?? false,
                onSort: { column in Task { await model.sort(tab.id, column: column) } }
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
        let hasMore = model.pages[tab.id]?.hasMore ?? false
        return HStack(spacing: 8) {
            if index > 0 {
                Button("← Previous") { Task { await model.changePage(tab.id, delta: -1) } }
                    .buttonStyle(DBMButtonStyle(kind: .secondary))
            }
            Text("Page \(index + 1)")
                .font(Theme.smallFont)
                .foregroundStyle(Theme.muted)
            if hasMore {
                Button("Next →") { Task { await model.changePage(tab.id, delta: 1) } }
                    .buttonStyle(DBMButtonStyle(kind: .secondary))
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
        .overlay(alignment: .top) { Rectangle().fill(Theme.border).frame(height: 1) }
    }

    private func exportCSV() {
        let panel = NSSavePanel()
        panel.title = "Export CSV"
        panel.nameFieldStringValue = "\(Workbench.safeFileName("\(schema).\(table)")).csv"
        panel.allowedContentTypes = [UTType.commaSeparatedText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await model.exportCSV(tab.id, path: url.path) }
    }
}
