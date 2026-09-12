import SwiftUI

@main
struct DBMNativeApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
                .frame(minWidth: 960, minHeight: 640)
                .preferredColorScheme(.dark)
                .task { await model.start() }
        }
        .windowResizability(.contentMinSize)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Connection") {
                    NotificationCenter.default.post(name: .dbmNewConnection, object: nil)
                }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            }
        }
    }
}

extension Notification.Name {
    static let dbmNewConnection = Notification.Name("dbm.newConnection")
}

struct ContentView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationSplitView {
            SidebarView()
                .navigationSplitViewColumnWidth(min: 260, ideal: 320, max: 420)
        } detail: {
            WorkbenchView()
        }
        .background(Theme.bg)
        .overlay(alignment: .top) {
            if let message = model.errorMessage {
                ErrorBanner(message: message) { model.errorMessage = nil }
                    .transition(.move(edge: .top))
            }
        }
        .overlay(alignment: .bottomTrailing) {
            if let toast = model.toast {
                ToastView(message: toast)
            }
        }
        .overlay {
            if let confirm = model.confirm {
                ConfirmOverlay(request: confirm)
            }
        }
        .animation(.easeOut(duration: 0.15), value: model.errorMessage)
        .animation(.easeOut(duration: 0.15), value: model.toast)
    }
}

struct ErrorBanner: View {
    let message: String
    let dismiss: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Text(message)
                .font(Theme.smallFont)
                .foregroundStyle(Theme.danger)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button("×", action: dismiss)
                .buttonStyle(DBMButtonStyle(kind: .link))
                .foregroundStyle(Theme.danger)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background(Theme.danger.opacity(0.12))
        .overlay(alignment: .bottom) {
            Rectangle().fill(Theme.danger.opacity(0.4)).frame(height: 1)
        }
        .task {
            try? await Task.sleep(nanoseconds: 10_000_000_000)
            dismiss()
        }
    }
}

struct ToastView: View {
    let message: String

    var body: some View {
        Text(message)
            .font(Theme.smallFont)
            .foregroundStyle(Theme.text)
            .padding(.horizontal, 12)
            .padding(.vertical, 9)
            .background(Theme.panelRaised)
            .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(Theme.borderStrong))
            .clipShape(RoundedRectangle(cornerRadius: 7))
            .shadow(color: .black.opacity(0.45), radius: 14, y: 6)
            .padding(18)
    }
}

struct ConfirmOverlay: View {
    @EnvironmentObject private var model: AppModel
    let request: ConfirmRequest

    var body: some View {
        ZStack {
            Theme.bg.opacity(0.65)
                .ignoresSafeArea()
            VStack(alignment: .leading, spacing: 12) {
                Text(request.title)
                    .font(Theme.titleFont)
                Text(request.body)
                    .font(Theme.smallFont)
                    .foregroundStyle(Theme.muted)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 8) {
                    Spacer()
                    Button("Cancel") { model.confirm = nil }
                        .buttonStyle(DBMButtonStyle(kind: .secondary))
                    Button(request.confirmLabel) {
                        Task { await model.perform(request) }
                    }
                    .buttonStyle(DBMButtonStyle(kind: request.danger ? .danger : .primary))
                }
            }
            .padding(20)
            .frame(width: 440)
            .background(Theme.bg)
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Theme.borderStrong))
            .clipShape(RoundedRectangle(cornerRadius: 10))
        }
    }
}
