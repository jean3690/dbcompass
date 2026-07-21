# Repository Guidelines

## Project Overview

DBCompass is a Rust desktop database client with a plugin architecture for connecting to multiple database engines (MySQL, PostgreSQL, SQLite). It uses the [Slint](https://slint.dev) GUI framework and the `slintcn` component library (zinc theme). Plugins are dynamically loaded `cdylib` crates that communicate through a shared FFI contract.

## Architecture & Data Flow

### Component Layers

```
UI (.slint files)  ←→  bridge.rs  ←→  plugins (cdylib .so/.dll/.dylib)
     ↑                    ↑
  Slint callbacks    AppState (Arc<Mutex>)
  properties/models  plugin_manager, connection_manager
                     query_executor, sql_completer
```

### Query Execution Flow

1. User types SQL → hits Run → `MainWindow` callback fires
2. `bridge.rs` resolves the active connection (selected or first available)
3. `query_executor::execute_query()` spawns a `std::thread` for background execution
4. Plugin's `DatabaseConnector::execute_query()` runs on the background thread
5. Result marshaled into `QueryResultView` (all strings — Slint-compatible)
6. `slint::invoke_from_event_loop()` delivers result to UI thread
7. `bridge::apply_query_result()` pushes columns/rows into Slint models

### Plugin Loading

- `PluginManager` scans `<binary_dir>/` for `.dll`/`.so`/`.dylib` files at startup
- Each plugin exports `_plugin_declaration` symbol (ABI version checked against `PLUGIN_INTERFACE_VERSION`)
- Declarative macros in `plugin-interface`: `declare_plugin!`, `declare_database_connector!`
- `PluginHandle` owns the `Library` for lifetime safety; `unload_all()` calls `on_unload()` in LIFO order

### State Management

- All shared state lives in `AppState` (defined in `src/app.rs`), wrapped in `Arc`
- Every field is `Mutex<T>` — single-threaded Slint UI + background threads for queries
- Poisoned mutex recovery: `lock().unwrap_or_else(|e| { eprintln!("[tag] Recovered…"); e.into_inner() })`
- `lock_mutex()` helper in `app.rs` abstracts this pattern (defined but not universally adopted yet)

### UI ↔ Rust Communication

- All Slint callbacks wired in `src/main.rs` via `window.on_<callback>(...)`
- Callbacks use `window.as_weak()` + `.upgrade()` to avoid dangling references
- Data flows Rust→Slint via Slint properties and `ModelRc` collections
- Data flows Slint→Rust via Slint callbacks with `SharedString` parameters

## Key Directories

| Directory | Purpose |
|---|---|
| `src/` | Main application: entry point, bridge, state, plugin loading, query execution, SQL completion |
| `plugin-interface/` | Library crate: shared types, traits (`DatabaseConnector`, `Plugin`), FFI macros |
| `plugins/` | Individual DB connector plugins (cdylib crates loaded at runtime) |
| `ui/` | Slint `.slint` UI definition files |
| `ui/slintcn/` | Generated component library (theme tokens, components: Button, Dialog, Table, Input, etc.) |
| `.github/workflows/` | CI (fmt+clippy+build+test), Release (cross-platform), Version Bump |

## Development Commands

```bash
# Build everything
cargo build --workspace

# Build only the main app
cargo build -p dbcompass

# Build a specific plugin
cargo build -p sqlite-plugin

# Run the application (plugins auto-discovered from binary directory)
cargo run -p dbcompass

# Lint (warnings as errors)
cargo clippy --workspace -- -D warnings

# Format
cargo fmt

# Run all tests
cargo test --workspace

# Run tests with output
cargo test --workspace -- --nocapture

# Run a single test
cargo test -p dbcompass <test_name>

# Release workflow
cargo release patch --no-publish --dry-run   # preview
cargo release patch --no-publish --execute   # apply
git-cliff -o CHANGELOG.md                    # generate changelog
```

### Linux System Dependencies

```bash
sudo apt-get install libglib2.0-dev libxkbcommon-dev libfontconfig-dev libwayland-dev
```

## Code Conventions & Common Patterns

### Naming

- Rust modules: `snake_case` (`plugin_manager`, `connection_manager`, `query_executor`)
- Slint components: `PascalCase` (`ConnectionTree`, `DataGrid`, `QueryEditor`)
- Slint callbacks: `kebab-case` (`execute-query`, `export-csv`, `tree-node-clicked`)
- Slint properties: `kebab-case` (`error-text`, `has-results`, `connector-type`)

### Threading & Async

- Slint runs on the main thread — **no async runtime**, no tokio
- Database queries spawn `std::thread` (see `query_executor.rs`); never block the main thread
- Results delivered back to UI via `slint::invoke_from_event_loop()`
- Plugin panics caught with `std::panic::catch_unwind`

### Mutex Patterns

- **Use poisoned mutex recovery everywhere:** `lock().unwrap_or_else(|e| { e.into_inner() })`
- Prefer the `lock_mutex()` helper from `app.rs` for readability
- Avoid locking the same mutex twice in one function (TOCTOU risk)

### Plugin Implementation Pattern

Every DB plugin follows this structure (see `plugins/sqlite-plugin/src/lib.rs` as reference):

```rust
// 1. Define connector struct with interior mutability
struct MyConnector {
    connections: Mutex<HashMap<u64, Mutex<Connection>>>,
    next_id: Mutex<u64>,
}

// 2. Implement Plugin trait (lifecycle hooks)
struct MyPlugin;
impl Plugin for MyPlugin { fn on_load(&self) { } fn on_unload(&self) { } }

// 3. Implement DatabaseConnector trait (9 methods, all &self)
impl DatabaseConnector for MyConnector { /* ... */ }

// 4. Export via macro
plugin_interface::declare_plugin!(MyPlugin, MyPlugin, MyConnector, MyConnector::new());
```

### Error Handling

- Plugin errors: `DbError` enum (ConnectionFailed, QueryFailed, Disconnected, etc.)
- Application errors: `Result<(), String>` with `eprintln!` for logging
- No `thiserror` or `anyhow` in use — manual `Display` impls and string conversions
- Connection file I/O errors return `String` (not `DbError`)

### Important Conventions

- **Edition 2024**: `#[unsafe(no_mangle)]` for FFI exports (not `#[no_mangle]`)
- Tree node IDs use `__` separator: `"connName__tableName"` for tables, `"connName__tableName__colName"` for columns
- Query results capped at **1000 rows** by default (`bridge.rs` passes `Some(1000)`)
- **Passwords stored in plaintext JSON** on disk (`<data_dir>/connections/<id>.json`)
- Slint strings use `slint::SharedString` — convert with `.to_string()` and `slint::SharedString::from()`
- `plugin-interface` has a `serialize` feature (optional serde) — the main app enables it

## Important Files

| File | Role |
|---|---|
| `src/main.rs` | Entry point, window creation, all Slint callback wiring (~550 lines) |
| `src/app.rs` | `AppState`: all shared state, `lock_mutex` helper, query history persistence |
| `src/bridge.rs` | Glue layer: tree operations, connection management, query dispatch, result application (~1315 lines) |
| `src/plugin_manager.rs` | Dynamic plugin loading via `libloading`, platform-specific library filtering |
| `src/connection_manager.rs` | Connection CRUD, JSON file persistence per connection |
| `src/query_executor.rs` | Background-thread query execution with catch_unwind (~65 lines) |
| `src/sql_completion.rs` | Offline SQL autocomplete (~980 lines): keywords, functions, types, context detection |
| `src/models/query_result.rs` | `QueryResultView`: plugin types → Slint-compatible string arrays |
| `plugin-interface/src/lib.rs` | FFI contract: traits, macros, `PluginDeclaration`, `_plugin_declaration` symbol |
| `plugin-interface/src/database.rs` | `DatabaseConnector` trait (9 methods) |
| `plugin-interface/src/types.rs` | `ConnectorConfig`, `QueryResult`, `CellValue`, `ColumnInfo`, `DbError`, `ColumnType` |
| `ui/app.slint` | Main window: toolbar, layout, all property/callback declarations |
| `build.rs` | Compiles `ui/app.slint` → generated Rust code via `slint_build::compile()` |
| `Cargo.toml` | Workspace root: 5 member crates, Slint 1.17.1, libloading 0.8 |

## Runtime/Tooling Preferences

- **Runtime**: Rust (edition 2024), no async runtime
- **Build system**: Cargo workspace
- **Package manager**: Cargo (no external packaging)
- **Version management**: `cargo-release` (auto bump, tag, push) + `git-cliff` (changelog from conventional commits)
- **Tag convention**: `v` prefix (`v0.1.2`)
- **Commit convention**: [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `chore:`, etc.)
- **UI tooling**: `slintcn` CLI generates themed components from `slintcn.json` config (zinc base color)
- **CI**: GitHub Actions — Ubuntu for checks/tests, Ubuntu + Windows for release builds

## Testing & QA

- **Framework**: `#[test]` / `#[cfg(test)]` (built-in Rust test harness, no external framework)
- **Run all**: `cargo test --workspace`
- **Coverage expectations**: Tests exist for `query_result.rs` (5 tests: null handling, empty results, errors, has_more), `sql_completion.rs` (context detection, keyword suggestions), `app.rs` (lock_mutex poison recovery, history dedup/truncate)
- **Plugin tests**: Each plugin may have its own tests; run with `cargo test -p <plugin-name>`
- **CI gates**: `cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`, `cargo build --workspace`, `cargo test --workspace`
