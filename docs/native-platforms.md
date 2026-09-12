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
crates/dbm-workbench          Shared presentation logic (no toolkit dependencies)
  connection-URL import, CSV, engine presets, statement targeting,
  schema-refresh summaries
        ▲
        │
experiments/linux-native      GTK4 presentation (Rust)
experiments/windows-native    Win32 + Direct2D/DirectWrite presentation (Rust)
experiments/macos-native      SwiftUI/AppKit presentation + Rust C-ABI bridge
```

- `crates/dbm-engine` is the single source of truth for profiles, credential
  storage, sessions, schema trees, table pages, queries, and mutations.
- `crates/dbm-workbench` holds the presentation logic that must behave the same
  everywhere: presets, connection-URL parsing, CSV output, destructive-statement
  detection, and statement/line targeting. Its tests run on every platform.
- The Tauri shell is a thin command layer over the engine; the React UI and its
  tests are unchanged.
- Native frontends link the engine in-process (the macOS app links it through a
  small JSON C ABI). They do not speak Tauri IPC, and they read and write the
  same local profile database and OS credential store as the Tauri app, so a
  profile created in one appears in the other.

## Status

| Platform | Presentation | State |
| --- | --- | --- |
| Linux | Rust + GTK4, custom CSS | Vertical slice implemented; exercised against live PostgreSQL 15 and Redis 7 under Xvfb |
| Windows | Rust + Win32 + Direct2D/DirectWrite | Vertical slice implemented; built and tested on Windows CI and exercised under Wine against live PostgreSQL and Redis. Not run on Windows by hand |
| macOS | Swift + SwiftUI/AppKit + Rust bridge | Vertical slice implemented; the bridge is tested on Linux and the app typechecks and builds on a macOS CI runner. Not launched yet |

All three are experiments: none is wired into installers, the updater, or
release artifacts yet. They are the direction of record for the next release,
with the Tauri app as the fallback until they reach parity and gain installers;
the README lists what is still missing.

## What the native slices implement

Shared behavior (identical across the three):

- **Connections:** the saved-profile list from the shared local store,
  per-profile colors, connect/disconnect, database or Redis index switching, and
  connection identity in the top bar.
- **Connection editor:** create, edit, test, and delete profiles, with engine
  presets, connection-URL import (`postgres://`, `postgresql://`, `mysql://`,
  `mariadb://`, `redis://`, `rediss://`, `valkey://`, `valkeys://`), TLS mode,
  CA certificate path, read-only switch, and password storage through the same
  OS credential store the Tauri app uses.
- **Schema tree:** database/schema/table/view nodes for PostgreSQL and MySQL,
  keyspace nodes grouped by Redis type, manual refresh with an added/removed
  summary.
- **Query tabs:** statement targeting ported from the React editor (including
  quoted strings, nested block comments, and PostgreSQL dollar quotes),
  keyboard run (Ctrl/⌘+Enter), confirmation before destructive statements, a
  10,000-row cap, per-profile-and-database history, and a results grid. A
  statement that resolves to exactly one table as `SELECT * FROM [schema.]table`
  opens the full table view instead, matching the Tauri workbench. Redis
  connections get a command workbench instead of SQL.
- **Table tabs:** paginated previews (200 rows), ordering, refresh, copy the
  current page as CSV, and full filtered CSV export with a large-export
  confirmation.
- **Chrome:** DBM's dark surfaces and cyan accent, custom confirmation windows
  for destructive actions, an error banner, and transient toasts. No stock
  GTK/AppKit/Win32 confirmation dialogs are used for DBM actions.

Platform notes:

- **Linux** keeps the editor in a plain GTK `TextView` with statement targeting
  from caret offsets, and offers a structured filter popover with the same
  thirteen operators as the Tauri UI.
- **Windows** paints everything with Direct2D, including a DirectWrite-backed
  text editor with caret, selection, and word movement, so no stock controls are
  involved. Column headers sort.
- **macOS** uses SwiftUI views over the bridge, with an AppKit `NSTextView`
  editor so statement targeting follows the caret or selection like the other
  frontends.

## Known gaps versus the Tauri app

- Staged inline edits and deletes are not built yet on any native frontend. The
  shared engine already supports mutations (including PostgreSQL `xmin`
  concurrency) and the Tauri UI implements them, so this is UI work. A
  `SELECT * FROM table` statement opens the full table view on every native
  frontend, matching the Tauri workbench, but that view is still read-only.
- Structured table filters exist on Linux only; Windows and macOS do ordering,
  paging, and CSV export.
- Query tabs cannot be renamed or collapsed; table columns cannot be collapsed
  or resized.
- Query history in one tab does not push a live update into other open tabs.
- There is no updater or installer integration for native builds, and none of
  them is signed or notarized.
- Windows has no draggable scrollbars and no mixed-DPI verification yet.

## Performance

The native frontends exist to remove the webview from the hot path, so the
project keeps a repeatable comparison. `scripts/bench-frontends.py` measures a
frontend under Xvfb: time to a mapped window, time to the first painted frame,
memory (RSS and PSS, summed over the process tree), CPU, and one connect +
query workload.

Recorded on an orb (Linux, x86_64, release builds, 1400x900, median of three
runs, same saved PostgreSQL profile). The Tauri column is the app from `main`,
built from a pristine worktree; the branch build measured the same within
run-to-run noise, and its React bundle is byte-identical.

| Metric | Tauri + WebKitGTK (main) | GTK4 native | Difference |
| --- | --- | --- | --- |
| Binary size | 30.9 MB | 11.8 MB | 2.6x smaller |
| Window mapped | 0.31 s | 0.30 s | same |
| First painted frame | 1.55 s | 0.83 s | 1.9x faster |
| Idle PSS / RSS | 304 MB / 492 MB | 126 MB / 166 MB | 2.4x / 3.0x less |
| After connect + query (PSS / RSS) | 355 MB / 548 MB | 159 MB / 202 MB | 2.2x / 2.7x less |
| Idle CPU | 0.25% | 0.00% | |

On Linux, Tauri renders the React UI in WebKitGTK (GTK3 + WebKit2GTK); on macOS
that webview is WKWebView and on Windows it is WebView2, so only the Linux
comparison has been measured here.

Two caveats matter when reading this:

- Xvfb has no GPU, and WebKit's compositor busy-waits there: the Tauri web
  process held ~61% of a core after a query result rendered, and dropped to
  1.2% with `WEBKIT_DISABLE_COMPOSITING_MODE=1`. That is an artifact of
  software rendering, not a claim about Tauri on a real desktop. The native
  frontend's post-query CPU settles back to 0%.
- Database work is engine-bound and identical by construction, so these numbers
  are about the presentation layer only. macOS and Windows have not been
  measured.

## Verification

Each native frontend is a separate Cargo workspace, so the root
`cargo test --workspace` stays toolkit-free on every platform.

```sh
# Engine + workbench logic + Tauri shell (root workspace)
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings

# Linux frontend
cd experiments/linux-native
cargo fmt --all -- --check && cargo test && cargo clippy --all-targets -- -D warnings

# Windows frontend (cross-checked from Linux, built and tested on Windows)
cd experiments/windows-native
cargo check --target x86_64-pc-windows-msvc
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
cargo test --target x86_64-pc-windows-gnu   # needs a Wine runner on Linux

# macOS bridge (runs anywhere) and app (macOS only)
cd experiments/macos-native
cargo test --manifest-path bridge/Cargo.toml
./build.sh check        # typechecks the Swift sources on macOS
```

Workflows: `native-linux.yml` (Ubuntu), `native-windows.yml` (Windows),
`native-macos.yml` (bridge on Ubuntu, SwiftUI app on macOS).

What has actually been verified:

- Root workspace: formatting, strict clippy, and 42 tests (engine, workbench
  logic, Tauri shell).
- Linux frontend: strict clippy, a release build, and a hand-run session against
  live PostgreSQL 15 and Redis 7 under Xvfb (connect, schema tree, query
  results, table paging, Redis keyspace and `PING`, connection editor, and
  `SELECT * FROM orders` opening the table view while `SELECT count(*) …` stays
  in the query tab).
- Windows frontend: `cargo check` for the MSVC target, strict clippy for the GNU
  target, a mingw release link, and unit tests run under Wine. A Wine 11 run
  exercised the whole workbench against live PostgreSQL 15 and Redis 7:
  connecting, the schema tree, typing in the DirectWrite editor, running SQL and
  Redis commands with Ctrl+Enter, the results grid, table pages, and
  `SELECT * FROM orders` opening the table view. Wine substitutes Segoe UI and
  drops a few glyphs in the 9 px eyebrow labels; everything else rendered.
  Nothing has been run on Windows itself.
- macOS bridge: tests for request validation, profile round-trip, and URL import,
  run on Linux. The SwiftUI layer typechecks and links into an app bundle on the
  macOS CI runner (macOS 14, arm64); it has not been launched.

The adapters decode PostgreSQL `numeric`, `uuid`, and `bytea` values directly:
`uuid` and `bytea` use tokio-postgres' built-in support, and `numeric` has its
own decoder because this dependency tree has no decimal feature for it. The
numeric decoder has unit tests, and all three were checked against live
PostgreSQL from the Linux and Windows frontends.

Compilation or a static screenshot is not proof of interaction parity. Treat
every "implemented" row above as "written and reviewed, pending a run on that
platform".

## Running the frontends

```sh
# Linux (GTK 4.6+ and a display server; .agents/setup installs libgtk-4-dev)
cd experiments/linux-native && cargo run

# Windows
cd experiments/windows-native && cargo run --release

# macOS (macOS 13+, Xcode command line tools)
cd experiments/macos-native && ./build.sh release && open "build/DBM Native.app"
```

All three open with the same profile list as the Tauri app. Passwords use the
operating system credential store, so a desktop keyring (GNOME Keyring, KWallet,
Windows Credential Manager, or the macOS Keychain) must be available for stored
passwords to resolve. Deleting a profile no longer fails when the credential
store is unavailable: the profile is removed and the stale entry, keyed by the
deleted profile id, is simply never read again. That behavior lives in
`AppState::delete_profile` so every frontend shares it.
