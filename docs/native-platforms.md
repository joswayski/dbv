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
  detection, statement/line targeting, and Rust table draft state. Its tests run
  on every platform; the Swift frontend mirrors the draft contract.
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
| macOS | Swift + SwiftUI/AppKit + Rust bridge | Vertical slice implemented; the bridge is tested on Linux. Earlier slice built on macOS CI; new staged-edit Swift changes need macOS compilation and runtime verification |

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
  confirmation. Double-click non-PK cells to stage edits; select rows to stage
  deletion. Amber edits/red deleted rows, before/after previews, undo-delete,
  pending counts, and Save/Discard use the shared engine's existing mutations.
  Drafts stay local across paging/sorting until Save. Undo-delete retains prior
  edits; read-only and PK-less tables cannot be changed. Refresh and full export
  require saving/discarding drafts; visible CSV copies include drafts and omit
  deleted rows. CSV headers and values exclude PostgreSQL's hidden `xmin`.
- **Chrome:** DBM's dark surfaces and cyan accent, custom confirmation windows
  for destructive actions, an error banner, and transient toasts. No stock
  GTK/AppKit/Win32 confirmation dialogs are used for DBM actions.

Platform notes:

- **Linux** keeps the editor in a plain GTK `TextView` with statement targeting
  from caret offsets, and offers a structured filter panel with the same
  thirteen operators as the Tauri UI.
- **Windows** paints everything with Direct2D, including a DirectWrite-backed
  text editor with caret, selection, and word movement, so no stock controls are
  involved. Column headers sort.
- **macOS** uses SwiftUI views over the bridge, with an AppKit `NSTextView`
  editor so statement targeting follows the caret or selection like the other
  frontends.

## Typography and design

The React UI is the design reference, not a claim of completed native parity.
The native frontends now use profile-tinted active tabs (16% over the sidebar,
3px underline and a subtle top highlight), typed 47px table headers, 36px rows,
full-width columns when space permits, and dark inset grid surfaces. Linux also
has the inline filter panel, row hover, 120ms tab crossfade and a 180ms loading
overlay that retains existing rows during a refresh. GTK honors its system
animation setting; SwiftUI button/tab transitions honor Reduce Motion.
Windows hover/press feedback is event-driven, not animated.

Satoshi is fetched with `npm run fonts` into the git-ignored `assets/fonts`;
the native Rust build scripts resolve that directory from their crate roots.
Editors retain platform monospace fonts. On Windows Satoshi uses a private
DirectWrite collection, with an explicit Segoe UI fallback if the loader is
unavailable (including Wine); monospace text uses the system collection. The
macOS bundle registers Satoshi with `ATSApplicationFontsPath`.

The GTK visual regression test renders real widgets under Xvfb and samples
pixels to catch Adwaita backgrounds, unstyled GtkBox tabs, lost profile color,
headers that fail to fill the grid, and missing/incorrect edit and delete
cell tints. Run it with:

```sh
xvfb-run -a env GSK_RENDERER=cairo cargo test --manifest-path experiments/linux-native/Cargo.toml -- --ignored
```

The first visual pass was exercised against disposable PostgreSQL data in GTK
and Wine. The staged-edit pass additionally exercised GTK Save, failed writes
with draft retention, undo/delete, and PostgreSQL partial conflicts (successful
rows commit; conflicting rows refresh without overwriting newer values).
Windows staged edits are cross-compiled, with seven tests passing under Wine
and a Wine smoke test of inline edits, row deletion, and before/after previews.
macOS Swift changes still require compilation and visual review on a Mac.
Linux and Wine screenshots do not establish macOS or real-Windows parity.

## Known gaps versus the Tauri app

- Structured table filters exist on Linux only; Windows and macOS do ordering,
  paging, and CSV export. Staged edits/deletes are implemented on all three but
  the Windows/macOS editing interactions still need native runtime verification.
- Query tabs cannot be renamed or collapsed. Table columns cannot be collapsed;
  GTK supports resizing, while Windows and macOS do not yet.
- Full visual/animation parity remains open: editor syntax highlighting,
  loading skeletons/blur, matching context menus, and uniform hover/focus
  treatment. Windows has no timed transitions and macOS has not been rendered.
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

With WebKit's DMA-BUF renderer disabled so both sides render in software (see
below), which is the fair comparison on a GPU-less display:

| Metric | Tauri + WebKitGTK (main) | GTK4 native | Difference |
| --- | --- | --- | --- |
| Binary size | 30.9 MB | 11.8 MB | 2.6x smaller |
| Window mapped | 0.28 s | 0.30 s | same |
| First painted frame | 1.29 s | 0.83 s | 1.6x faster |
| Idle PSS / RSS | 217 MB / 396 MB | 126 MB / 166 MB | 1.7x / 2.4x less |
| After connect + query (PSS / RSS) | 247 MB / 431 MB | 159 MB / 202 MB | 1.6x / 2.1x less |
| CPU after a query | 1.25% | 0.00% | |

On Linux, Tauri renders the React UI in WebKitGTK (GTK3 + WebKit2GTK); on macOS
that webview is WKWebView and on Windows it is WebView2, so only the Linux
comparison has been measured here.

Two caveats matter when reading this:

- Without `WEBKIT_DISABLE_DMABUF_RENDERER=1`, WebKit picks its DMA-BUF
  renderer, which has no real GPU buffers to import under Xvfb. The compositor
  then busy-waits at ~61% of a core after a query result first promotes
  content to a composited layer, and holds ~110 MB more in graphics buffers
  (idle PSS 304 MB, load 355 MB). `WEBKIT_DISABLE_COMPOSITING_MODE=1` has the
  same effect. That is software-rendering behavior, not a claim about Tauri on
  a GPU desktop, but it does mean the webview has a rendering path the native
  frontends simply do not have.
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
  plus mutation dispatch and hidden-column CSV handling, run on Linux. The
  earlier SwiftUI slice typechecked and linked on the macOS CI runner (macOS 14,
  arm64); the staged-edit changes have not been compiled or launched on macOS.

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
