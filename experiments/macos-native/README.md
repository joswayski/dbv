# macOS native frontend (in development)

A SwiftUI/AppKit frontend for DBM that links the shared Rust engines through a
small C-ABI bridge, so there is no webview and no Tauri IPC.

This is an experiment: it is not wired into installers, signing, notarization,
or the updater. The shipping macOS app is still Tauri.

## Layout

```
bridge/          Rust static library: JSON-in/JSON-out C ABI over dbm-engine
DBMNative/       SwiftUI app (models, bridge wrapper, views)
build.sh         builds the bridge and compiles the .app bundle
Info.plist       bundle metadata for the local test app
```

The bridge exposes four C functions:

| Function | Purpose |
| --- | --- |
| `dbm_init()` | initializes the engine, returns the saved profiles as JSON |
| `dbm_call(request)` | performs one operation from a JSON request, returns JSON |
| `dbm_free(pointer)` | releases a string returned by the bridge |
| `dbm_shutdown()` | closes every open session |

Requests are documented at the top of `bridge/src/lib.rs`; every response is
`{"ok": …}` or `{"error": "…"}`. Calls block, and the Swift side runs them on a
background task, which keeps the ABI small and avoids callback lifetimes.

## Building

```sh
./build.sh          # debug bridge + "build/DBM Native.app"
./build.sh release  # release bridge + app bundle
./build.sh check    # typecheck the Swift sources without building the app
open "build/DBM Native.app"
```

Requirements: macOS 13 or newer, Xcode command line tools, and the Rust
toolchain pinned in `rust-toolchain.toml`. The Rust bridge itself builds and
tests on any platform (`cargo test` inside `bridge/`), so CI can check it
without macOS.

## What this slice implements

- Saved connections from the same local profile database and OS credential
  store as the Tauri app, with per-connection colors and connect/disconnect.
- Connection editor: create, edit, test, and delete profiles, with engine
  presets, connection-URL import, TLS, CA path, read-only switch, and colors.
- Schema/keyspace browsing with database switching and refresh.
- Query tabs: statement-under-cursor targeting, ⌘↩ to run, destructive-statement
  confirmation, 10,000-row cap, per-profile history, and a results grid.
- Table tabs: paginated previews, ordering from column headers, CSV copy, and
  full filtered CSV export.

## Known gaps

- Statement targeting runs the statement at the end of the document: SwiftUI's
  `TextEditor` does not expose the caret or selection, so "statement under the
  cursor" is not available yet. SQL selection support needs an `NSTextView`
  wrapper.
- Structured table filters are not implemented yet (ordering, paging, and CSV
  export are).
- No staged inline edits, no tab rename, and no updater or installer
  integration.
- The app is built ad-hoc for local testing; it is not signed or notarized.

## Verification status

The Rust bridge is covered by tests that run anywhere (`cargo test` in
`bridge/`): request validation, profile round-trip, and connection-URL import.
The SwiftUI layer has **not** been compiled in this repository's development
environment; `.github/workflows/native-macos.yml` typechecks and builds it on a
macOS runner, and `./build.sh check` does the same locally. Treat the UI as
unverified until it has been run on a Mac.
