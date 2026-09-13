# Windows native frontend (in development)

A Win32 + Direct2D/DirectWrite frontend for DBM that links the shared Rust
engines directly, so there is no webview and no Tauri IPC. DBM's own chrome is
painted by a custom renderer; no stock Win32 controls are used for the
workbench.

This is an experiment: it is not wired into installers or the updater. The
shipping Windows app is still Tauri.

## Layout

```
src/main.rs        Win32 window class, message loop, input translation
src/render.rs      Direct2D + DirectWrite renderer (shapes, text, clipping)
src/theme.rs       DBM's palette and metrics
src/bridge.rs      tokio runtime + engine results posted to the window thread
src/platform.rs    clipboard and the system "Save as" dialog
src/ui/            state, custom-painted views, dialogs, text editing
tools/             helper for running the binary under Wine (see below)
```

The workbench is a single window. Layout is computed while painting and the
resulting hit regions are used for input, so there is no retained widget tree.
The connection editor and confirmations are in-window modals rather than
separate dialogs, and only genuinely system-owned surfaces (the CSV save
dialog) use Win32 dialogs.

## Building

On Windows:

```sh
cargo run --release
```

From Linux (cross-check only, no runtime):

```sh
rustup target add x86_64-pc-windows-gnu
cargo check --target x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu   # needs mingw-w64
```

`cargo check --target x86_64-pc-windows-msvc` also works from Linux and matches
the toolchain Microsoft ships, because checking does not link.

## What this slice implements

- Saved connections from the same local profile database and OS credential
  store as the Tauri app, with per-connection colors, connect/disconnect, and
  database or Redis index switching.
- Connection editor: create, edit, test, and delete profiles, with engine
  presets, connection-URL import, TLS, CA path, read-only switch, and the DBM
  color palette.
- Schema/keyspace tree with expand/collapse and refresh.
- Query tabs: statement-under-cursor targeting, Ctrl+Enter to run,
  destructive-statement confirmation, 10,000-row cap, per-profile history, and
  a scrollable results grid. A `SELECT * FROM table` statement opens the full
  table view instead.
- Table tabs: 200-row pages, ordering from column headers, refresh, CSV copy to
  the clipboard, and full filtered CSV export through the system save dialog.
  Non-PK cells support double-click edits; selected rows support staged deletion
  and undo. Amber/red pending states, before/after previews, and Save/Discard
  keep changes local until Save. Read-only and PK-less tables stay non-editable.
  Visible CSV copies include drafts and omit deleted rows; refresh and full
  export require saving or discarding pending changes first.
- Text editing (caret, selection, word movement, clipboard) implemented against
  DirectWrite, because the editor is a custom-painted surface.

## Known gaps

- Structured table filters are not implemented yet (ordering, paging, and CSV
  export are).
- No tab rename and no updater or installer integration.
- Scrollbars are wheel/keyboard driven; there are no draggable scrollbars yet.
- Per-monitor DPI changes update the render target, but mixed-DPI multi-monitor
  behavior has not been exercised.

## Verification status

`cargo check --target x86_64-pc-windows-msvc`, `cargo clippy --target
x86_64-pc-windows-gnu -D warnings`, `cargo test` (on Windows), and a mingw
release link all pass, which catches API misuse at the type level. On Linux the
unit tests run under Wine with:

```sh
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine \
  cargo test --target x86_64-pc-windows-gnu
```

For a smoke test without a Windows machine, the mingw build runs under Wine.
Wine 11 renders the chrome, layout, and text; it substitutes Segoe UI and drops
some glyphs in the 9 px uppercase labels, so treat text rendering as
unverified. Wine also predates the `bcryptprimitives.dll` API set that Rust's
standard library imports, so `tools/bcryptprimitives_stub.c` is needed first:

```sh
x86_64-w64-mingw32-gcc -shared -O2 -o bcryptprimitives.dll \
  tools/bcryptprimitives_stub.c -lbcrypt
wine dbm-native.exe
```

The visual-parity pass also ran on Wine 8.0. In a disposable Wine prefix,
missing Consolas caused an invisible editor even though query execution worked.
Mapping the missing system families to installed Linux fonts restored editor
text (restart the Wine app afterward):

```sh
wine reg add 'HKCU\Software\Wine\Fonts\Replacements' /v 'Consolas' /t REG_MULTI_SZ /d 'DejaVu Sans Mono' /f
wine reg add 'HKCU\Software\Wine\Fonts\Replacements' /v 'Segoe UI' /t REG_MULTI_SZ /d 'DejaVu Sans' /f
```

This is a Wine-only test setup, not an application requirement or evidence of
Windows font parity. Wine still renders UI text heavier than the Tauri
reference. The pass checked typed full-width table/query grids, purple active
tabs, SQL execution, and descending header sorting against local PostgreSQL.

A Wine run with live PostgreSQL 15 and Redis 7 exercised connecting, the schema
tree, typing in the DirectWrite editor, running SQL and Redis commands with
Ctrl+Enter, the results grid, and table pages.

Nothing here has been run on Windows itself. Capture behavior, keychain
integration, DPI, and text rendering still need a real Windows run.
The new staged-edit workflow is cross-compiled and has unit coverage, but still
requires hands-on Windows verification (including double-click, blur/Enter/
Escape, Ctrl/Shift selection, and Save/Discard).
