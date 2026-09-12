import SwiftUI

struct QueryTabView: View {
    @EnvironmentObject private var model: AppModel
    let tab: WorkbenchTab

    private var database: String {
        if case let .query(database) = tab.kind { return database }
        return ""
    }

    private var engine: DatabaseEngine { model.engine(for: tab.profileId) }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            HStack(spacing: 0) {
                editor
                Rectangle().fill(Theme.border).frame(width: 1)
                history
            }
            Rectangle().fill(Theme.border).frame(height: 1)
            results
        }
    }

    private var toolbar: some View {
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 0) {
                Text(engine.isRedis ? "REDIS WORKBENCH" : "SQL WORKBENCH")
                    .font(Theme.eyebrowFont)
                    .foregroundStyle(Theme.muted)
                Text("\(tab.title)  ·  \(database)")
                    .font(Theme.titleFont)
            }
            Spacer()
            if model.runningQueries.contains(tab.id) {
                Text("Running…")
                    .font(Theme.smallFont)
                    .foregroundStyle(Theme.muted)
            }
            Button("Refresh") { Task { await model.runQuery(tab.id) } }
                .buttonStyle(DBMButtonStyle(kind: .secondary))
                .disabled(model.runningQueries.contains(tab.id))
            Button(engine.isRedis ? "Run command" : "Run statement") {
                Task { await model.runQuery(tab.id) }
            }
            .buttonStyle(DBMButtonStyle(kind: .primary))
            .keyboardShortcut(.return, modifiers: .command)
            .disabled(model.runningQueries.contains(tab.id))
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.border).frame(height: 1) }
    }

    private var editor: some View {
        VStack(alignment: .leading, spacing: 0) {
            TextEditor(text: sqlBinding)
                .font(Theme.monoFont)
                .foregroundStyle(Theme.text)
                .scrollContentBackground(.hidden)
                .background(Theme.editor)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            Text(
                engine.isRedis
                    ? "The command under the cursor runs · Command+Return · results capped at 10,000 rows"
                    : "The statement under the cursor runs · Command+Return · results capped at 10,000 rows"
            )
            .font(Theme.smallFont)
            .foregroundStyle(Theme.muted)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
        }
    }

    private var history: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("HISTORY")
                .font(Theme.eyebrowFont)
                .foregroundStyle(Theme.muted)
                .padding(.horizontal, 10)
                .padding(.top, 6)
                .padding(.bottom, 4)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    if (model.histories[tab.id] ?? []).isEmpty {
                        Text("Run a query to start history.")
                            .font(Theme.smallFont)
                            .foregroundStyle(Theme.muted)
                            .padding(.horizontal, 10)
                    }
                    ForEach(model.histories[tab.id] ?? []) { entry in
                        Button {
                            model.useHistory(tab.id, entry: entry)
                        } label: {
                            VStack(alignment: .leading, spacing: 1) {
                                Text("\(entry.success ? "✓" : "!") \(Workbench.compact(entry.sql))")
                                    .font(Theme.smallFont)
                                    .lineLimit(1)
                                Text(shortTime(entry.executedAt))
                                    .font(.system(size: 10))
                                    .foregroundStyle(Theme.muted)
                            }
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 4)
            }
        }
        .frame(width: 230)
        .background(Theme.sidebar)
    }

    private var results: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(model.queryMeta[tab.id] ?? "Results will appear here.")
                    .font(Theme.smallFont)
                    .foregroundStyle(model.queryErrors[tab.id] == nil ? Theme.muted : Theme.danger)
                Spacer()
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 5)
            if let error = model.queryErrors[tab.id] {
                Text(error)
                    .font(Theme.smallFont)
                    .foregroundStyle(Theme.danger)
                    .padding(.horizontal, 14)
                    .padding(.bottom, 4)
                    .lineLimit(2)
            }
            if let response = model.queries[tab.id], !response.columns.isEmpty {
                DataGrid(
                    columns: response.columns.map { (name: $0.name, dataType: $0.dataType) },
                    rows: response.rows
                )
            } else {
                Spacer()
            }
        }
        .frame(minHeight: 160, idealHeight: 240)
    }

    private var sqlBinding: Binding<String> {
        Binding(
            get: { model.sqlText[tab.id] ?? "" },
            set: { model.sqlText[tab.id] = $0 }
        )
    }

    private func shortTime(_ value: String) -> String {
        // The engine writes RFC 3339 timestamps; show the clock portion.
        guard let time = value.split(separator: "T").last?.prefix(8) else { return value }
        return String(time)
    }
}
