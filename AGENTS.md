# Repository guidance

## Product

- DBM is a local-first desktop database manager (TablePlus / DataGrip style) for macOS, Windows, and Linux.
- The current milestone is PostgreSQL, MySQL, and Redis: saved connections, schema or keyspace exploration, paginated table and key browsing, SQL/Redis workbench tabs, PK-backed or key-backed edits, and safe local profile storage.
- Clearly separate current features from roadmap ideas. Do not present Redis Sentinel/Cluster, SSH jump hosts, encrypted profile sync, or other follow-ups as shipped unless the repository already implements them.
- Prefer TablePlus/DataGrip-like defaults when UX is ambiguous: fast path to query, obvious refresh, non-destructive confirms for bulk writes.

## Repository map

- `apps/desktop` contains the Tauri desktop application (`@dbm/desktop`) and its React UI.
- `apps/desktop/ui/src` is the React frontend (Vite, Zustand, CodeMirror SQL editor).
- `apps/desktop/src-tauri/src` is the Tauri command layer (IPC and updates) for the desktop app.
- `crates/dbm-engine` is the toolkit-independent engine crate (profiles, storage, credentials, sessions, PostgreSQL/MySQL/Redis adapters, models). Frontends share it; it must not depend on Tauri or any UI framework.
- `crates/dbm-workbench` is the toolkit-independent presentation logic (engine presets, connection-URL import, CSV, statement targeting, schema-refresh summaries). Frontends share it; it must not depend on a UI toolkit.
- `experiments/linux-native` (GTK4), `experiments/windows-native` (Win32 + Direct2D), and `experiments/macos-native` (SwiftUI + a Rust C-ABI bridge) are in-development native frontends, each a separate Cargo workspace so root checks stay toolkit-free. They are the direction of record for the next release; the Tauri app stays the fallback until they reach parity (staged edits, Windows/macOS filters, tab rename, installers) and are documented in `docs/native-platforms.md`.
- `docs/releases.md` contains public-release signing, notarization, and publishing requirements.
- `scripts` contains build and install helpers.
- There is no separate monorepo package for the UI; frontend and backend live under `apps/desktop`.
- UI calls Tauri through `commands.ts`. The Vite browser preview uses in-memory mocks in that module so layout work does not require Tauri.

## Working conventions

- Keep changes focused on the request and preserve existing behavior unless a change is intentional.
- Do not overwrite, stage, or publish unrelated work already present in the checkout.
- Assume other agents may be working in this repository concurrently. Prefer a dedicated branch (or Git worktree) for new work, and never stash, switch, or overwrite another agent's checkout without explicit instruction.
- Reuse established patterns in the repository before introducing a new abstraction or dependency. Match existing naming (`profileId`, `workspace`, tab kinds `"table" | "query"`).
- Treat macOS, Windows, and Linux parity as the default. Prefer shared UI and logic that works on every supported platform; when a fix or feature must be platform-specific, implement or stub the equivalent path on the others (or explicitly gate with `cfg` / runtime checks), and document any unavoidable limitation in the PR and README if user-facing. Do not assume “works on my Mac” is enough—call out what was and was not verified on other platforms.
- Do not add telemetry, cloud sync, or network calls that send connection profiles, query history, or database results off-device.
- Keep this file concise and update it when a recurring repository convention or correction should persist across future work.

## Visual design

- DBM uses a dark workbench chrome: near-black surfaces (`--bg`, `--panel`), muted borders, high-contrast body text, and a cyan accent (`--accent` / `--accent-strong`) for primary actions and focus.
- Connection identity is multi-color: each profile has its own color for sidebar, tabs, and main-pane theming. Do not force a single accent across all connections.
- Establish hierarchy with typography, spacing, and dense-but-readable layout before adding color. Prefer restrained shadows, small corner radii, and concise UI copy.
- Preserve accessible contrast on dark surfaces. Status and danger colors (`--success`, `--danger`) keep stable meanings.

## Product behavior to preserve

- **Local-only:** connection profiles, query history, and results stay on the machine. Passwords live only in the OS keyring / credential store, never in the app SQLite file.
- **Connect → query:** opening a connection should land the user in a SQL query tab so they can run statements immediately (schema tree remains in the sidebar). Selecting an already-connected profile should keep or restore that profile's workbench, not dump the user on the empty welcome pane. Deleting or disconnecting a profile must close its tabs.
- **Table tabs:** paginated previews, filters, ordering, CSV copy/export, PK-backed edits (PostgreSQL `xmin` concurrency; MySQL primary-key matching), Redis key index and typed key views, read-only profiles.
- **Query tabs:** run statement under cursor or selection (⌘/Ctrl+Enter), history per profile+database, results capped (10k rows). Simple `SELECT * FROM table` can open the editable table viewer. Redis connections use a command workbench instead of SQL.
- **Refresh:** table and query result views should be re-fetchable without re-authoring filters or SQL (toolbar Refresh).
- SSH jump hosts, Redis Sentinel/Cluster, and encrypted profile sync are intentional non-goals until documented otherwise.

## Documentation

- Every pull request must leave the root `README.md` accurate. Update it when a change affects features, platform support, setup, build commands, privacy, networking, releases, or what is implemented vs planned.
- Keep the root README product- and developer-focused. Put detailed public-release signing and publishing procedures in `docs/releases.md`.
- If a pull request does not need a README edit, still verify that its changes do not make the README inaccurate; do not add no-op wording solely to touch the file.
- Keep current behavior and roadmap / deliberate follow-ups distinct, especially for adapters and transports that are not implemented yet.

## Validation

- Run `npm run check` for the default desktop UI gate (typecheck, lint, Vitest).
- For Rust changes, also run `cargo fmt --all -- --check`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings` when practical.
- Common local commands:

  ```sh
  npm install
  cargo test --workspace
  npm run check          # typecheck + lint + unit tests (desktop UI)
  npm run test           # UI tests only
  npm run dev            # Tauri dev
  npm run build          # native build (+ install/launch on macOS by default)
  ```

- UI tests use Vitest + Testing Library under `apps/desktop/ui/src/*.test.tsx`.
- Report exactly which checks ran and any checks that could not run. Do not wait on CI unless asked; the maintainer will report failures.

## Pull requests

- Prefer a focused pull request over pushing directly to `main` unless explicitly asked otherwise. Ready-for-review (not draft) is fine by default.
- Use `gh` for GitHub operations (works more reliably with private repositories than alternate CLIs).
- Use a concise title and description covering what changed, why it changed, user or developer impact, and validation.
