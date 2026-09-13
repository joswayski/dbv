import AppKit
import SwiftUI

struct WorkbenchView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            TopBar()
            TabStrip()
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(Theme.bg)
    }

    @ViewBuilder
    private var content: some View {
        if let tab = model.tabs.first(where: { $0.id == model.activeTabId }) {
            switch tab.kind {
            case .table:
                TableTabView(tab: tab)
                    .id(tab.id)
            case .query:
                QueryTabView(tab: tab)
            }
        } else {
            WelcomeView()
        }
    }
}

struct TopBar: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 10) {
            if let profile = model.activeProfileId.flatMap({ model.profile($0) }) {
                Circle()
                    .fill(Color(hex: profile.displayColor))
                    .frame(width: 12, height: 12)
                    .shadow(color: Color(hex: profile.displayColor).opacity(0.9), radius: 6)
                VStack(alignment: .leading, spacing: 0) {
                    Text(profile.name).font(.system(size: 12.5, weight: .bold))
                    Text(identity(profile))
                        .font(.system(size: 11))
                        .foregroundStyle(Theme.muted)
                }
            } else {
                Text("No active connection")
                    .font(Theme.uiFont)
                    .foregroundStyle(Theme.muted)
            }
            Spacer()
        }
        .padding(.horizontal, 18)
        .frame(height: 68)
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.border).frame(height: 1) }
    }

    private func identity(_ profile: ConnectionProfile) -> String {
        let database = model.database(for: profile.id)
        return profile.username.isEmpty
            ? "\(profile.host):\(profile.port) / \(database)"
            : "\(profile.username)@\(profile.host):\(profile.port) / \(database)"
    }
}

struct TabStrip: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 0) {
                    ForEach(model.tabs) { tab in
                        WorkbenchTabButton(tab: tab)
                    }
                }
            }
            if let profileId = model.activeProfileId, model.isConnected(profileId) {
                Button("＋") { model.openQuery(profileId: profileId) }
                    .buttonStyle(DBMButtonStyle(kind: .link))
                    .padding(.horizontal, 6)
            }
            Spacer(minLength: 0)
        }
        .frame(height: 38)
        .background(Theme.sidebar)
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.border).frame(height: 1) }
    }

}

private struct WorkbenchTabButton: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var isHovering = false
    let tab: WorkbenchTab

    private var isActive: Bool { model.activeTabId == tab.id }
    private var color: Color {
        model.profile(tab.profileId).map { Color(hex: $0.displayColor) } ?? Theme.accent
    }

    var body: some View {
        HStack(spacing: 6) {
            Button(tab.title) { model.activeTabId = tab.id }
                .buttonStyle(.plain)
                .font(Theme.uiFont)
                .fontWeight(isActive ? .bold : .regular)
                .foregroundStyle(isActive ? Theme.text : Theme.muted)
                .shadow(color: isActive ? color.opacity(0.42) : .clear, radius: 6)
            Button("×") { model.closeTab(tab.id) }
                .buttonStyle(DBMButtonStyle(kind: .link))
                .foregroundStyle(Theme.muted)
        }
        .padding(.horizontal, 12)
        .frame(maxHeight: .infinity)
        .background(isActive ? color.opacity(isHovering ? 0.20 : 0.16) : (isHovering ? Theme.text.opacity(0.06) : .clear))
        .background(Theme.sidebar)
        .overlay(alignment: .bottom) {
            Rectangle().fill(isActive ? color : Color.clear).frame(height: 3)
        }
        .overlay(alignment: .top) {
            Rectangle().fill(isActive ? color.opacity(0.34) : Color.clear).frame(height: 1)
        }
        .overlay(alignment: .trailing) { Rectangle().fill(Theme.border).frame(width: 1) }
        .onHover { isHovering = $0 }
        .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: isHovering)
        .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: isActive)
    }
}

struct WelcomeView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 10) {
            Text(title)
                .font(Theme.titleFont)
            Text(detail)
                .font(Theme.uiFont)
                .foregroundStyle(Theme.muted)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var title: String {
        model.activeProfileId.flatMap { model.profile($0) }?.name ?? "No connection selected"
    }

    private var detail: String {
        guard let profileId = model.activeProfileId, let _ = model.profile(profileId) else {
            return model.profiles.isEmpty
                ? "Create a connection from the sidebar to get started."
                : "Select a saved connection from the sidebar to browse its data."
        }
        return model.isConnected(profileId)
            ? "Choose a table from the sidebar or open a new query with the plus button above."
            : "This connection is selected but not connected. Select it again to connect."
    }
}

/// The shared data grid used by table browsing and query results.
struct TableCellEditing: Equatable {
    let rowKey: String
    let column: Int
    var value: String
}

struct DataGrid: View {
    let columns: [(name: String, dataType: String)]
    let rows: [[JSONValue]]
    var sortColumn: String?
    var descending = false
    var onSort: ((String) -> Void)?
    var rowKeys: [String]? = nil
    var pendingRows: [String: PendingRow] = [:]
    var primaryKey: Set<String> = []
    var editable = false
    var interactionDisabled = false
    var selectedRows: Set<String> = []
    var tableEditing: Binding<TableCellEditing?>? = nil
    var onSelect: ((Int, String, NSEvent.ModifierFlags) -> Void)?
    var onEdit: ((String, Int, JSONValue) -> Void)?
    var onDiscard: ((String) -> Void)?

    @State private var previewKey: String?
    @FocusState private var focusedCell: GridCellAddress?

    private let headerHeight: CGFloat = 47
    private let rowHeight: CGFloat = 36

    var body: some View {
        GeometryReader { geometry in
            let fittedWidths = fittedWidths(availableWidth: geometry.size.width)
            ScrollView([.horizontal, .vertical]) {
                LazyVStack(alignment: .leading, spacing: 0) {
                    header(widths: fittedWidths)
                    ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                        rowView(row, index: index, widths: fittedWidths)
                    }
                }
            }
        }
        .background(Color(hex: "#0e1620"))
        .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(Theme.border))
        .clipShape(RoundedRectangle(cornerRadius: 7))
        .onChange(of: focusedCell) { focused in
            if focused == nil, tableEditing?.wrappedValue != nil { commitEditing() }
        }
    }

    private var widths: [CGFloat] {
        columns.map { Workbench.columnWidth($0.dataType) }
    }

    private func fittedWidths(availableWidth: CGFloat) -> [CGFloat] {
        let intrinsic = widths
        let total = intrinsic.reduce(0, +)
        guard total > 0, total < availableWidth else { return intrinsic }
        let extra = (availableWidth - total) / CGFloat(intrinsic.count)
        return intrinsic.map { $0 + extra }
    }

    private func header(widths: [CGFloat]) -> some View {
        HStack(spacing: 0) {
            ForEach(Array(columns.enumerated()), id: \.offset) { index, column in
                Button {
                    onSort?(column.name)
                } label: {
                    VStack(alignment: .leading, spacing: 3) {
                        HStack(spacing: 4) {
                            Text(marker(column.name))
                                .font(.system(size: 11, weight: .bold))
                                .foregroundStyle(Theme.text)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                        }
                        Text(column.dataType)
                            .font(.system(size: 8))
                            .foregroundStyle(Theme.subtle)
                            .lineLimit(1)
                    }
                    .padding(.horizontal, 10)
                    .frame(width: widths[index], height: headerHeight, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(interactionDisabled)
                .overlay(alignment: .trailing) { Rectangle().fill(Theme.border).frame(width: 1) }
            }
        }
        .frame(width: widths.reduce(0, +), height: headerHeight, alignment: .leading)
        .background(Color(hex: "#172332"))
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.borderStrong).frame(height: 1) }
    }

    private func marker(_ name: String) -> String {
        guard let sortColumn, sortColumn == name else { return name }
        return name + (descending ? " ↓" : " ↑")
    }

    private func rowView(_ row: [JSONValue], index: Int, widths: [CGFloat]) -> some View {
        let key = rowKeys?[safe: index] ?? "query-row:\(index)"
        let pending = pendingRows[key]
        let deleted = pending?.deleted == true
        return HStack(spacing: 0) {
            ForEach(Array(columns.enumerated()), id: \.offset) { columnIndex, column in
                let value = columnIndex < row.count ? row[columnIndex] : JSONValue.null
                let address = GridCellAddress(row: key, column: columnIndex)
                let isEditing = tableEditing?.wrappedValue.map {
                    $0.rowKey == key && $0.column == columnIndex
                } == true
                let changed = pending?.deleted == false
                    && pending?.original[safe: columnIndex] != pending?.changes[safe: columnIndex]
                Group {
                    if isEditing {
                        TextField("", text: editingText)
                            .textFieldStyle(.plain)
                            .font(Theme.uiFont)
                            .foregroundStyle(Theme.text)
                            .padding(.horizontal, 5)
                            .frame(height: 25)
                            .background(Theme.editor)
                            .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(Theme.accent))
                            .focused($focusedCell, equals: address)
                            .onSubmit { commitEditing() }
                            .onExitCommand {
                                tableEditing?.wrappedValue = nil
                                focusedCell = nil
                            }
                            .onAppear { focusedCell = address }
                    } else {
                        Text(value.display)
                            .font(Theme.uiFont)
                            .foregroundStyle(value.isNull ? Theme.subtle : Theme.text)
                            .strikethrough(deleted, color: Theme.danger.opacity(0.8))
                            .lineLimit(1)
                    }
                }
                .padding(.horizontal, isEditing ? 4 : 10)
                .frame(width: widths[columnIndex], height: rowHeight, alignment: .leading)
                .background(changed ? Theme.warning.opacity(0.20) : Color.clear)
                .contentShape(Rectangle())
                .onTapGesture(count: 2) {
                    guard editable,
                          !interactionDisabled,
                          !deleted,
                          !primaryKey.contains(column.name)
                    else { return }
                    commitEditing()
                    tableEditing?.wrappedValue = TableCellEditing(
                        rowKey: key,
                        column: columnIndex,
                        value: value.isNull ? "" : value.display
                    )
                }
            }
        }
        .frame(width: widths.reduce(0, +), height: rowHeight, alignment: .leading)
        .background(rowBackground(key: key, pending: pending))
        .overlay {
            if selectedRows.contains(key) {
                Rectangle().strokeBorder(Theme.accent.opacity(0.20))
            }
        }
        .overlay(alignment: .trailing) {
            if let pending {
                Button(pending.deleted ? "DELETE" : "EDIT") { previewKey = key }
                    .buttonStyle(.plain)
                    .disabled(interactionDisabled)
                    .font(.system(size: 8, weight: .bold))
                    .foregroundStyle(pending.deleted ? Theme.danger : Theme.warning)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 3)
                    .background(Theme.panelRaised.opacity(0.94))
                    .overlay(Capsule().strokeBorder((pending.deleted ? Theme.danger : Theme.warning).opacity(0.42)))
                    .clipShape(Capsule())
                    .padding(.trailing, 7)
                    .popover(isPresented: previewBinding(for: key), arrowEdge: .top) {
                        PendingChangePreview(
                            pending: pending,
                            columns: columns.map { $0.name },
                            onDiscard: {
                                previewKey = nil
                                onDiscard?(key)
                            }
                        )
                    }
            }
        }
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.border.opacity(0.55)).frame(height: 1) }
        .contentShape(Rectangle())
        .onTapGesture {
            guard rowKeys != nil, !interactionDisabled else { return }
            onSelect?(index, key, NSEvent.modifierFlags.intersection(.deviceIndependentFlagsMask))
        }
    }

    private var editingText: Binding<String> {
        Binding(
            get: { tableEditing?.wrappedValue?.value ?? "" },
            set: { value in
                guard var editing = tableEditing?.wrappedValue else { return }
                editing.value = value
                tableEditing?.wrappedValue = editing
            }
        )
    }

    private func commitEditing() {
        guard let editing = tableEditing?.wrappedValue else { return }
        onEdit?(editing.rowKey, editing.column, parseCellValue(editing.value))
        tableEditing?.wrappedValue = nil
        focusedCell = nil
    }

    private func rowBackground(key: String, pending: PendingRow?) -> Color {
        if pending?.deleted == true { return Theme.danger.opacity(0.09) }
        if pending != nil { return Theme.warning.opacity(0.11) }
        if selectedRows.contains(key) { return Theme.accent.opacity(0.08) }
        return .clear
    }

    private func previewBinding(for key: String) -> Binding<Bool> {
        Binding(
            get: { previewKey == key },
            set: { showing in previewKey = showing ? key : nil }
        )
    }
}

private struct GridCellAddress: Hashable {
    let row: String
    let column: Int
}

private struct PendingChangePreview: View {
    let pending: PendingRow
    let columns: [String]
    let onDiscard: () -> Void

    private var changes: [(column: String, before: String, after: String)] {
        columns.enumerated().compactMap { index, column in
            guard pending.original[safe: index] != pending.changes[safe: index] else { return nil }
            return (
                column,
                pending.original[safe: index]?.display ?? "NULL",
                pending.changes[safe: index]?.display ?? "NULL"
            )
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            HStack(spacing: 18) {
                Text(pending.deleted ? "Pending delete" : "Pending edit")
                    .font(Theme.smallFont.weight(.bold))
                    .foregroundStyle(pending.deleted ? Theme.danger : Theme.warning)
                Spacer()
                Button(pending.deleted ? "Undo delete" : "Discard edit", action: onDiscard)
                    .buttonStyle(DBMButtonStyle(kind: pending.deleted ? .danger : .secondary))
            }
            if pending.deleted {
                Text("This row will be deleted when changes are saved.")
                    .font(Theme.smallFont)
                    .foregroundStyle(Theme.text)
            } else {
                ForEach(Array(changes.enumerated()), id: \.offset) { _, change in
                    VStack(alignment: .leading, spacing: 5) {
                        Text(change.column)
                            .font(.system(size: 9, weight: .bold))
                            .foregroundStyle(Theme.warning)
                        HStack(alignment: .top, spacing: 7) {
                            DiffValue(label: "BEFORE", value: change.before, changed: false)
                            Text("→")
                                .font(Theme.smallFont)
                                .foregroundStyle(Theme.warning)
                                .padding(.top, 22)
                            DiffValue(label: "AFTER", value: change.after, changed: true)
                        }
                    }
                    .padding(.top, 7)
                    .overlay(alignment: .top) { Rectangle().fill(Theme.border.opacity(0.7)).frame(height: 1) }
                }
            }
        }
        .padding(12)
        .frame(minWidth: 420, maxWidth: 620, alignment: .leading)
        .background(pending.deleted ? Color(hex: "#21181d") : Color(hex: "#151b23"))
    }
}

private struct DiffValue: View {
    let label: String
    let value: String
    let changed: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label)
                .font(.system(size: 8, weight: .bold))
                .foregroundStyle(changed ? Theme.warning.opacity(0.8) : Theme.muted)
            Text(value)
                .font(.system(size: 10, design: .monospaced))
                .foregroundStyle(changed ? Color(hex: "#fef3c7") : Theme.text)
                .textSelection(.enabled)
                .padding(.horizontal, 7)
                .padding(.vertical, 6)
                .frame(minWidth: 165, maxWidth: 270, minHeight: 30, alignment: .topLeading)
                .background(changed ? Theme.warning.opacity(0.08) : Theme.editor)
                .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(changed ? Theme.warning.opacity(0.18) : Color.clear))
                .clipShape(RoundedRectangle(cornerRadius: 4))
        }
    }
}

func parseCellValue(_ value: String) -> JSONValue {
    if value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return .null }
    if value == "true" { return .bool(true) }
    if value == "false" { return .bool(false) }
    if let number = Double(value),
       number.isFinite,
       value.range(of: #"^-?\d+(\.\d+)?$"#, options: .regularExpression) != nil
    {
        return .number(number)
    }
    return .string(value)
}

private extension Collection {
    subscript(safe index: Index) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
