import SwiftUI

struct SidebarView: View {
    @EnvironmentObject private var model: AppModel
    @State private var editingProfile: ConnectionProfile?
    @State private var creatingProfile = false

    var body: some View {
        VStack(spacing: 0) {
            header
            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(model.profiles) { profile in
                        ConnectionGroup(profile: profile) { editing in
                            editingProfile = editing
                        }
                    }
                    if model.profiles.isEmpty {
                        Text("No saved connections.")
                            .font(Theme.smallFont)
                            .foregroundStyle(Theme.muted)
                            .padding(12)
                    }
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
            }
            footer
        }
        .background(Theme.sidebar)
        .sheet(item: $editingProfile) { profile in
            ConnectionEditorView(profile: profile)
        }
        .sheet(isPresented: $creatingProfile) {
            ConnectionEditorView(profile: nil)
        }
        .onReceive(NotificationCenter.default.publisher(for: .dbmNewConnection)) { _ in
            creatingProfile = true
        }
    }

    private var header: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text("DB")
                    .font(Theme.eyebrowFont)
                    .foregroundStyle(Theme.accent)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 5)
                    .background(Theme.panelRaised)
                    .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(Theme.border))
                    .clipShape(RoundedRectangle(cornerRadius: 7))
                VStack(alignment: .leading, spacing: 0) {
                    Text("DBM").font(.system(size: 14, weight: .bold))
                    Text("database manager")
                        .font(.system(size: 10))
                        .foregroundStyle(Theme.muted)
                }
                Spacer()
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            Divider().overlay(Theme.border)

            HStack {
                Text("CONNECTIONS")
                    .font(Theme.eyebrowFont)
                    .foregroundStyle(Theme.muted)
                Spacer()
                Button("New connection") { creatingProfile = true }
                    .buttonStyle(DBMButtonStyle(kind: .primary))
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
        }
    }

    private var footer: some View {
        HStack {
            Text("LOCAL ONLY")
                .font(Theme.eyebrowFont)
                .foregroundStyle(Theme.success)
                .padding(.horizontal, 5)
                .padding(.vertical, 3)
                .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Theme.success.opacity(0.35)))
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(Theme.bg.opacity(0.25))
        .overlay(alignment: .top) { Rectangle().fill(Theme.border).frame(height: 1) }
    }
}

struct ConnectionGroup: View {
    @EnvironmentObject private var model: AppModel
    let profile: ConnectionProfile
    let onEdit: (ConnectionProfile) -> Void

    @State private var expanded = true

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            row
            if isActive, model.isConnected(profile.id), expanded {
                WorkspacePanel(profile: profile)
            }
        }
    }

    private var isActive: Bool { model.activeProfileId == profile.id }

    private var row: some View {
        HStack(spacing: 8) {
            Button {
                Task { await model.select(profile) }
            } label: {
                HStack(spacing: 8) {
                    Circle()
                        .fill(Color(hex: profile.displayColor))
                        .frame(width: 10, height: 10)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(profile.name)
                            .font(.system(size: 12.5, weight: .semibold))
                            .lineLimit(1)
                        Text(subtitle)
                            .font(.system(size: 11))
                            .foregroundStyle(Theme.muted)
                            .lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if model.isConnected(profile.id) {
                Button(expanded ? "⌃" : "⌄") { expanded.toggle() }
                    .buttonStyle(DBMButtonStyle(kind: .link))
                    .foregroundStyle(Theme.muted)
            }
            Menu {
                Button("Edit connection") { onEdit(profile) }
                if model.isConnected(profile.id) {
                    Button("Disconnect") { Task { await model.disconnect(profile.id) } }
                }
            } label: {
                Text("⋯").foregroundStyle(Theme.muted)
            }
            .menuStyle(.borderlessButton)
            .frame(width: 24)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .background(isActive ? Theme.panelRaised : Color.clear)
        .overlay(alignment: .leading) {
            if isActive {
                Rectangle()
                    .fill(Color(hex: profile.displayColor))
                    .frame(width: 3)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 7))
    }

    private var subtitle: String {
        let engine = profile.engine.label
        return profile.username.isEmpty
            ? "\(engine) · \(profile.host)"
            : "\(engine) · \(profile.username)@\(profile.host)"
    }
}

struct WorkspacePanel: View {
    @EnvironmentObject private var model: AppModel
    let profile: ConnectionProfile

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(profile.engine.isRedis ? "DATABASE INDEX" : "DATABASE")
                .font(Theme.eyebrowFont)
                .foregroundStyle(Theme.muted)
            Picker("", selection: databaseBinding) {
                ForEach(model.workspaces[profile.id]?.databases ?? [], id: \.name) { database in
                    Text(database.name).tag(database.name)
                }
            }
            .labelsHidden()
            .pickerStyle(.menu)

            HStack {
                Text(profile.engine.isRedis ? "KEYSPACE" : "SCHEMA")
                    .font(Theme.eyebrowFont)
                    .foregroundStyle(Theme.muted)
                Spacer()
                Button(model.refreshingSchema == profile.id ? "Refreshing…" : "Refresh") {
                    Task { await model.refreshSchema(profile.id) }
                }
                .buttonStyle(DBMButtonStyle(kind: .link))
                .disabled(model.refreshingSchema == profile.id)
            }

            VStack(alignment: .leading, spacing: 1) {
                ForEach(model.schemas[profile.id] ?? []) { node in
                    SchemaNodeView(profileId: profile.id, node: node, depth: 0, selected: selectedTable)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(10)
        .background(Theme.bg.opacity(0.35))
        .overlay(alignment: .top) { Rectangle().fill(Theme.border).frame(height: 1) }
    }

    private var databaseBinding: Binding<String> {
        Binding(
            get: { model.database(for: profile.id) },
            set: { value in Task { await model.switchDatabase(profile.id, to: value) } }
        )
    }

    private var selectedTable: (String, String)? {
        guard let tab = model.tabs.first(where: { $0.id == model.activeTabId }),
              case let .table(schema, table) = tab.kind
        else { return nil }
        return (schema, table)
    }
}

struct SchemaNodeView: View {
    @EnvironmentObject private var model: AppModel
    let profileId: String
    let node: SchemaNode
    let depth: Int
    let selected: (String, String)?

    @State private var expanded = true

    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            Button {
                if node.isTable, let schema = node.schema, let table = node.table {
                    model.openTable(profileId: profileId, schema: schema, table: table)
                } else {
                    expanded.toggle()
                }
            } label: {
                HStack(spacing: 6) {
                    Text(node.isTable ? "▧" : (expanded ? "⌄" : "›"))
                        .font(.system(size: 10))
                        .foregroundStyle(Theme.muted)
                        .frame(width: 12)
                    Text(node.badge)
                        .font(Theme.eyebrowFont)
                        .foregroundStyle(badgeColor)
                        .padding(.horizontal, 3)
                        .padding(.vertical, 1)
                        .background(badgeColor.opacity(0.15))
                        .clipShape(RoundedRectangle(cornerRadius: 3))
                    Text(node.name)
                        .font(Theme.uiFont)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 3)
                .padding(.horizontal, 5)
                .background(isSelected ? Theme.accent.opacity(0.15) : Color.clear)
                .clipShape(RoundedRectangle(cornerRadius: 4))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .padding(.leading, CGFloat(depth) * 12)

            if !node.isTable, expanded {
                ForEach(node.children) { child in
                    SchemaNodeView(profileId: profileId, node: child, depth: depth + 1, selected: selected)
                }
            }
        }
    }

    private var isSelected: Bool {
        guard let selected else { return false }
        return node.schema == selected.0 && node.table == selected.1
    }

    private var badgeColor: Color {
        switch node.kind {
        case "table": return Theme.accent
        case "key": return Color(hex: "#a78bfa")
        case "view": return Theme.success
        default: return Theme.muted
        }
    }
}
