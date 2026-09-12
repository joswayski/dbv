# DBM

DBM is a local-first desktop database manager for macOS, Windows, and Linux.
It currently supports PostgreSQL, MySQL (including MariaDB-compatible servers),
and Redis: saved connections, database and schema or keyspace exploration,
paginated table and key browsing, SQL or Redis command execution, and safe
local profile storage.

## Development

Prerequisites:

- Node.js 24 and npm 11
- Rust 1.94 with `rustfmt` and `clippy`
- Tauri's native dependencies for the operating system

```sh
npm install
cargo test --workspace
npm run check
npm run dev
```

### Amp orbs

Amp orbs run [`.agents/setup`](.agents/setup) to prepare a fresh machine: it installs Tauri's
Linux build dependencies, `redis-server` (the live Redis tests skip themselves without it),
Node.js 24 with npm 11, the Rust toolchain pinned in `rust-toolchain.toml`, and the locked npm and
Cargo dependencies. [`.agents/resume`](.agents/resume) only checks that the environment is still
intact when an orb wakes.

The Vite browser preview is declared in [`.amp/services.yaml`](.amp/services.yaml). Inside an orb,
`amp orb services ensure` starts it supervised and prints its portal URL.

## Build and install

`npm run build` creates a native build for the operating system where the
command runs. It prints the absolute paths to the unpackaged executable and
every installer or app bundle it creates.

On macOS, a successful build also:

1. Quits any running DBM instance.
2. Replaces `/Applications/DBM.app` with the new build.
3. Launches the newly installed app.

The generated app bundle and DMG remain under `target/release/bundle`.
Local builds use an installed Apple Development signing identity when one is
available and otherwise use an ad-hoc signature.

An ad-hoc signature changes whenever DBM is rebuilt. Because DBM keeps database
passwords in macOS Keychain, macOS may ask for the login keychain password when
a newly built copy first reads an existing password. This is a macOS system
prompt—DBM never receives the login keychain password. A stable Apple
Development signing identity avoids that repeated approval.

```sh
# Build + install + launch (default on macOS)
npm run build

# Build only, without changing /Applications
DBM_SKIP_INSTALL=1 npm run build

# Install without launching
DBM_OPEN_AFTER_INSTALL=0 npm run build
```

On Windows, the build creates an NSIS installer under
`target/release/bundle/nsis` and an unpackaged executable at
`target/release/dbm.exe`. If that exact unpackaged executable is already
running, the build stops it first so it can be replaced.

On Linux, the build creates `.deb` and AppImage packages under
`target/release/bundle`, plus the unpackaged executable at
`target/release/dbm`.

One local build only targets the current operating system. Pushing a version
tag runs the release workflow on macOS, Windows, and Linux and creates a draft
GitHub release with all three platforms' installers, signed updater artifacts,
and a validated `latest.json` manifest. Official builds check that manifest
from the top bar and install authenticated updates in place where the platform
supports it.

Creating installers is not the same as preparing a public release. Public
publishing also requires Developer ID signing and notarization on macOS,
Authenticode signing on Windows, and checksums plus build-provenance
attestations for every downloadable artifact. The required account setup,
workflow gates, and clean-machine acceptance checks are documented in
[docs/releases.md](docs/releases.md).

DBM never uploads connection profiles, query history, or database results.
Passwords are stored in the operating system credential store when available.

## What is implemented

- PostgreSQL, MySQL, and Redis direct connections with disabled, preferred, or required TLS.
- Local connection profiles and query history in an application SQLite database.
- Passwords through the macOS Keychain, Windows Credential Manager, or Linux
  secret service via `keyring`.
- Signed in-app updates from published GitHub Releases.
- Database list, schemas, tables/views, configurable previews up to 200 rows,
  structured multi-filtering, ordering, visible-page CSV copy, and full filtered
  CSV export. Redis connections show numbered databases, a SCAN-backed key
  index, and per-type key folders (strings, hashes, lists, sets, sorted sets,
  streams).
- Resizable sidebars and columns, collapsible wide fields, and multi-row
  selection for staged edits and deletes.
- Inline edits and staged deletes for primary-key-backed tables. PostgreSQL
  edits are guarded by `xmin` optimistic concurrency; MySQL edits match on the
  primary key. Redis table views edit strings, hashes, lists, sets, and sorted
  sets in place, and can delete keys from the key index. Read-only profile
  mode blocks GUI writes on every engine.
- SQL tabs using CodeMirror, query result grids, a 10,000-row safety cap, and
  per-profile history. Connecting a profile opens a query tab so you can run
  SQL immediately. Redis connections open a command workbench (`PING` by
  default) instead of SQL.
- Refresh on table previews and query results: reload the current page and
  filters, or re-run the last executed statement, without re-authoring them.

The browser preview used by Vite has a small in-memory mock so the layout can be
worked on without launching Tauri. The real desktop app uses the Rust commands.

## Deliberate follow-ups

SSH jump-host transport, query cancellation with dedicated sessions, Redis
Sentinel/Cluster, and encrypted profile sync are kept out of this vertical
slice. The profile model already reserves the SSH shape, but the backend
returns a clear unsupported-transport error until the forwarding
implementation is added and tested on all three OSes.

## Local data

The app stores non-secret profile metadata, settings, and up to 500 history
entries in the platform application data directory. Passwords are never put in
that SQLite file. There is no telemetry, account, or sync service.
