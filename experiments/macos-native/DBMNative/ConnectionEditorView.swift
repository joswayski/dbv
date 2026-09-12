import SwiftUI

struct ConnectionEditorView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss

    let profile: ConnectionProfile?

    @State private var input: SaveProfileInput
    @State private var url = ""
    @State private var feedback: (success: Bool, message: String)?
    @State private var busy = false

    init(profile: ConnectionProfile?) {
        self.profile = profile
        _input = State(initialValue: SaveProfileInput.draft(for: profile))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(profile == nil ? "New connection" : "Edit connection")
                .font(Theme.titleFont)

            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 9) {
                GridRow {
                    label("Database engine")
                    enginePicker
                }
                GridRow {
                    label("Connection URL")
                    HStack(spacing: 6) {
                        TextField(input.engine.urlPlaceholder, text: $url)
                            .textFieldStyle(.roundedBorder)
                            .font(Theme.uiFont)
                            .onSubmit { Task { await importURL() } }
                        Button("Import URL") { Task { await importURL() } }
                            .buttonStyle(DBMButtonStyle(kind: .secondary))
                            .disabled(url.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                }
                GridRow {
                    label("Name")
                    field($input.name)
                }
                GridRow {
                    label("Connection color")
                    colorPicker
                }
                GridRow {
                    label("Host")
                    field($input.host)
                }
                GridRow {
                    label("Port")
                    TextField("", value: $input.port, format: .number)
                        .textFieldStyle(.roundedBorder)
                        .font(Theme.uiFont)
                        .frame(width: 110)
                }
                GridRow {
                    label(input.engine.isRedis ? "Username (ACL, optional)" : "Username")
                    field($input.username)
                }
                GridRow {
                    label(input.engine.isRedis ? "Database index" : "Database")
                    field($input.defaultDatabase)
                }
                GridRow {
                    label("Password")
                    SecureField(
                        profile == nil ? "Stored in OS credential store" : "Leave blank to keep saved password",
                        text: passwordBinding
                    )
                    .textFieldStyle(.roundedBorder)
                    .font(Theme.uiFont)
                }
                GridRow {
                    label("TLS")
                    Picker("", selection: $input.tlsMode) {
                        ForEach(TlsMode.allCases) { mode in
                            Text(mode.label).tag(mode)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(width: 160)
                }
                GridRow {
                    label("CA certificate path (optional)")
                    field(caBinding)
                }
                GridRow {
                    label("")
                    Toggle("Read-only profile (blocks GUI edits and mutations)", isOn: $input.readOnly)
                        .toggleStyle(.checkbox)
                        .font(Theme.smallFont)
                }
            }

            if let feedback {
                Text(feedback.message)
                    .font(Theme.smallFont)
                    .foregroundStyle(feedback.success ? Theme.success : Theme.danger)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Text("Passwords are stored in your operating system credential manager and are never written to DBM's profile database.")
                .font(Theme.smallFont)
                .foregroundStyle(Theme.muted)
                .fixedSize(horizontal: false, vertical: true)

            HStack(spacing: 8) {
                if let profile {
                    Button("Delete") {
                        model.confirm = ConfirmRequest(
                            title: "Delete connection",
                            body: "Delete connection “\(profile.name)”? Saved password and query history for this profile will be removed.",
                            confirmLabel: "Delete",
                            danger: true,
                            action: .deleteProfile(profile)
                        )
                        dismiss()
                    }
                    .buttonStyle(DBMButtonStyle(kind: .danger))
                }
                Spacer()
                Button("Cancel") { dismiss() }
                    .buttonStyle(DBMButtonStyle(kind: .secondary))
                Button(busy ? "Working…" : "Test connection") { Task { await test() } }
                    .buttonStyle(DBMButtonStyle(kind: .secondary))
                    .disabled(busy)
                Button(busy ? "Working…" : "Save & connect") { Task { await save() } }
                    .buttonStyle(DBMButtonStyle(kind: .primary))
                    .disabled(busy)
            }
        }
        .padding(20)
        .frame(width: 640)
        .background(Theme.bg)
    }

    private func label(_ text: String) -> some View {
        Text(text)
            .font(Theme.smallFont)
            .foregroundStyle(Theme.muted)
            .frame(width: 170, alignment: .leading)
    }

    private func field(_ binding: Binding<String>) -> some View {
        TextField("", text: binding)
            .textFieldStyle(.roundedBorder)
            .font(Theme.uiFont)
    }

    private var enginePicker: some View {
        HStack(spacing: 6) {
            ForEach(DatabaseEngine.allCases) { engine in
                Button(engine.label) {
                    input.applyEngineDefaults(engine)
                }
                .buttonStyle(DBMButtonStyle(kind: input.engine == engine ? .primary : .secondary))
            }
        }
    }

    private var colorPicker: some View {
        HStack(spacing: 6) {
            ForEach(Theme.connectionColors, id: \.self) { color in
                Button {
                    input.color = color
                } label: {
                    Circle()
                        .fill(Color(hex: color))
                        .frame(width: 20, height: 20)
                        .overlay(
                            Circle().strokeBorder(
                                input.color == color ? Theme.text : Color.clear,
                                lineWidth: 2
                            )
                        )
                }
                .buttonStyle(.plain)
            }
        }
    }

    private var passwordBinding: Binding<String> {
        Binding(
            get: { input.password ?? "" },
            set: { input.password = $0.isEmpty ? nil : $0 }
        )
    }

    private var caBinding: Binding<String> {
        Binding(
            get: { input.caCertPath ?? "" },
            set: { input.caCertPath = $0.isEmpty ? nil : $0 }
        )
    }

    private func importURL() async {
        guard let imported = await model.importURL(url) else { return }
        input.engine = imported.engine
        input.host = imported.host
        input.port = imported.port
        input.username = imported.username
        input.defaultDatabase = imported.defaultDatabase
        input.tlsMode = imported.tlsMode
        if let password = imported.password { input.password = password }
        if input.name.isEmpty || input.name == input.engine.presetName {
            input.name = imported.suggestedName
        }
        feedback = (true, "Connection URL imported. Review the details, then save and connect.")
    }

    private func test() async {
        busy = true
        defer { busy = false }
        feedback = (true, "Testing connection…")
        if let error = await model.testProfile(input) {
            feedback = (false, error)
        } else {
            feedback = (true, "Connection successful.")
        }
    }

    private func save() async {
        busy = true
        defer { busy = false }
        feedback = (true, "Testing connection before saving…")
        if await model.saveProfile(input) {
            dismiss()
        } else {
            feedback = (false, model.errorMessage ?? "Could not save this connection.")
            model.errorMessage = nil
        }
    }
}
