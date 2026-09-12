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
        .frame(height: 48)
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
                        tabButton(tab)
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

    private func tabButton(_ tab: WorkbenchTab) -> some View {
        let isActive = model.activeTabId == tab.id
        let color = model.profile(tab.profileId).map { Color(hex: $0.displayColor) } ?? Theme.accent
        return HStack(spacing: 6) {
            Button(tab.title) { model.activeTabId = tab.id }
                .buttonStyle(.plain)
                .font(Theme.uiFont)
                .foregroundStyle(isActive ? Theme.text : Theme.muted)
            Button("×") { model.closeTab(tab.id) }
                .buttonStyle(DBMButtonStyle(kind: .link))
                .foregroundStyle(Theme.muted)
        }
        .padding(.horizontal, 12)
        .frame(maxHeight: .infinity)
        .background(isActive ? Theme.panelRaised : Color.clear)
        .overlay(alignment: .bottom) {
            Rectangle().fill(isActive ? color : Color.clear).frame(height: 2)
        }
        .overlay(alignment: .trailing) { Rectangle().fill(Theme.border).frame(width: 1) }
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
struct DataGrid: View {
    let columns: [(name: String, dataType: String)]
    let rows: [[JSONValue]]
    var sortColumn: String?
    var descending = false
    var onSort: ((String) -> Void)?

    private let headerHeight: CGFloat = 26
    private let rowHeight: CGFloat = 24

    var body: some View {
        ScrollView([.horizontal, .vertical]) {
            LazyVStack(alignment: .leading, spacing: 0) {
                header
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    rowView(row)
                }
            }
        }
        .background(Theme.bg)
    }

    private var widths: [CGFloat] {
        columns.map { Workbench.columnWidth($0.dataType) }
    }

    private var totalWidth: CGFloat {
        widths.reduce(0, +)
    }

    private var header: some View {
        HStack(spacing: 0) {
            ForEach(Array(columns.enumerated()), id: \.offset) { index, column in
                Button {
                    onSort?(column.name)
                } label: {
                    HStack(spacing: 4) {
                        Text(marker(column.name))
                            .font(.system(size: 11, weight: .bold))
                            .foregroundStyle(Theme.muted)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 6)
                    .frame(width: widths[index], height: headerHeight, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .overlay(alignment: .trailing) { Rectangle().fill(Theme.border).frame(width: 1) }
            }
        }
        .frame(width: totalWidth, height: headerHeight, alignment: .leading)
        .background(Theme.panel)
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.border).frame(height: 1) }
    }

    private func marker(_ name: String) -> String {
        guard let sortColumn, sortColumn == name else { return name }
        return name + (descending ? " ↓" : " ↑")
    }

    private func rowView(_ row: [JSONValue]) -> some View {
        HStack(spacing: 0) {
            ForEach(Array(columns.enumerated()), id: \.offset) { index, _ in
                let value = index < row.count ? row[index] : JSONValue.null
                Text(value.display)
                    .font(Theme.uiFont)
                    .foregroundStyle(value.isNull ? Theme.subtle : Theme.text)
                    .lineLimit(1)
                    .padding(.horizontal, 6)
                    .frame(width: widths[index], height: rowHeight, alignment: .leading)
            }
        }
        .frame(width: totalWidth, height: rowHeight, alignment: .leading)
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.border).frame(height: 1) }
    }
}
