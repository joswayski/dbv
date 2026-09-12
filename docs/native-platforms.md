# Platform-native frontends

DBM's shipping desktop app is Tauri 2 with a React UI. This document describes
the in-development alternative: platform-native, custom-rendered presentation
layers that link the same Rust database engines directly, without a webview.

Nothing here replaces the Tauri app yet. Tauri remains the shipped baseline for
macOS, Windows, and Linux until a native frontend proves equivalent behavior and
measured value.

## Why

Tauri renders every window in a system webview. For a database workbench that
means a full browser engine (and, on Linux, a second process) resident next to
the Rust engines, plus a presentation layer that can never quite match the
platform's native menus, focus, and input behavior. The native frontends keep
the engines and rebuild only the presentation layer, so connection handling,
schema browsing, table pages, query execution, and local storage stay identical
by construction.

"Native" here does **not** mean stock GTK/AppKit/Win32 widgets. DBM keeps its own
dark workbench chrome, per-connection colors, and custom confirmations. Only
genuinely system-owned surfaces (permission prompts, file pickers) are left to
the OS.

## Architecture

```
apps/desktop/src-tauri        Tauri shell: IPC commands + updater (shipping)
        │
        ▼
crates/dbm-engine             UI-independent engines (no toolkit dependencies)
  models, error, storage, keyring_store, state, session,
  postgres, mysql, redis
        ▲
        │
experiments/linux-native      GTK4 presentation (this repository)
experiments/macos-native      SwiftUI/AppKit presentation (planned)
experiments/windows-native    Win32 + DirectComposition presentation (planned)
```

- `crates/dbm-engine` is the single source of truth for profiles, credential
  storage, sessions, schema trees, table pages, queries, and mutations. It
  depends on no presentation toolkit, so every frontend shares one behavior.
- The Tauri shell (`apps/desktop/src-tauri`) is now a thin command layer over
  the engine; the React UI and its tests are unchanged.
- Native frontends link the engine crate in-process. They do not speak Tauri
  IPC, and they read and write the same local profile database and OS credential
  store as the Tauri app, so a profile created in one appears in the other.

## Status

| Platform | Presentation | State |
| --- | --- | --- |
| Linux | Rust + GTK4, custom CSS | Vertical slice implemented and exercised against live PostgreSQL and Redis in an orb |
| macOS | Swift + SwiftUI/AppKit + Rust bridge | Planned; not started in this repository |
| Windows | Rust + Win32 + DirectComposition/Direct3D/Direct2D/DirectWrite | Planned; not started in this repository |

The Linux slice is an experiment: it builds and runs, but it is not wired into
installers, the updater, or release artifacts.

## What the Linux slice implements

- **Connections:** the saved-profile list from the shared local store, per-profile
  colors, connect/disconnect, database or Redis index switching, and connection
  identity in the top bar.
- **Connection editor:** create, edit, test, and delete profiles, with engine
  presets, connection-URL import (`postgres://`, `postgresql://`, `mysql://`,
  `mariadb://`, `redis://`, `rediss://`, `valkey://`, `valkeys://`), a color
  picker, TLS mode, CA certificate path, read-only switch, and password storage
  through the same OS credential store the Tauri app uses.
- **Schema tree:** database/schema/table/view nodes for PostgreSQL and MySQL,
  keyspace nodes grouped by Redis type, manual refresh with an added/removed
  summary.
- **Query tabs:** statement-under-cursor or explicit selection targeting ported
  from the React editor (including quoted strings, nested block comments, and
  PostgreSQL dollar quotes), Ctrl+Enter, confirmation before destructive
  statements, a 10,000-row cap, per-profile-and-database history, and a results
  grid. Redis connections get a command workbench instead of SQL.
- **Table tabs:** paginated previews (200 rows), structured filters with the same
  thirteen operators as the Tauri UI, ordering, refresh, copy the current page
  as CSV, and full filtered CSV export with a large-export confirmation.
- **Chrome:** DBM's dark surfaces and cyan accent, custom confirmation windows
  for destructive actions, an error banner, and transient toasts. No stock GTK
  confirmation dialogs are used for DBM actions.

## Known gaps versus the Tauri app

- Staged inline edits and deletes, and the editable table viewer that opens for a
  simple `SELECT * FROM table`, are not built yet. The shared engine already
  supports mutations (including PostgreSQL `xmin` concurrency), so this is UI
  work, not engine work.
- Query tabs cannot be renamed or collapsed, and table columns cannot be
  collapsed, resized, or sorted by clicking a header (ordering uses a toolbar
  control instead).
- Query history in one tab does not push a live update into other open tabs; each
  tab refreshes its own history after it runs a statement.
- There is no updater or installer integration for native builds yet.
- macOS and Windows frontends do not exist yet.

## Verification

The native workspace is a separate Cargo workspace (`experiments/linux-native`)
so the root `cargo test --workspace` stays toolkit-free on macOS and Windows.

```sh
# Engine + Tauri shell (root workspace)
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings

# Linux native frontend
cd experiments/linux-native
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

`npm run check` still covers the React UI. `.github/workflows/native-linux.yml`
runs the native checks on Ubuntu with GTK 4 development packages installed.

Unit tests cover the ported behavior that must match the React UI: CSV escaping,
destructive-statement detection, engine presets, connection-URL parsing, and
statement/line targeting. Live behavior was verified by hand in an orb against
PostgreSQL 15 and Redis 7 with a real profile store, a session keyring, and a
headless X server: connect, schema tree, query execution with results, table
paging, Redis keyspace and `PING`, and the connection editor.

Behavior parity with the Tauri UI is asserted only where tests exist. Compilation
or a static screenshot is not proof of interaction parity, and none of the
macOS/Windows claims above are verified on those operating systems.

## Running the Linux frontend

```sh
cd experiments/linux-native
cargo run
```

Requirements: GTK 4.6 or newer development packages, and a display server.
`.agents/setup` installs `libgtk-4-dev` for Amp orbs; on Debian/Ubuntu:

```sh
sudo apt-get install libgtk-4-dev
```

The app opens with the same profile list as the Tauri app. Passwords use the
operating system credential store, so a desktop keyring (GNOME Keyring, KWallet,
or similar) must be available for stored passwords to resolve.
