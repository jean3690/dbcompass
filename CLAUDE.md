# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

DBCompass is a Rust desktop database client with a plugin architecture for connecting to multiple database engines. It uses the [Slint](https://slint.dev) GUI framework for its UI and the `slintcn` component library for themed widgets.

## Build & Run Commands

```bash
# Build the main application and all plugins
cargo build

# Build only the main app
cargo build -p dbcompass

# Build all plugins (shared libraries placed in target/debug/ or target/release/)
cargo build --workspace

# Build individual plugins
cargo build -p sqlite-plugin
cargo build -p mysql-plugin
cargo build -p postgres-plugin

# Run the application
cargo run -p dbcompass

# Run clippy (treat warnings as errors)
cargo clippy -- -D warnings

# Auto-format
cargo fmt

# Run all tests
cargo test --workspace

# Run tests with output shown
cargo test --workspace -- --nocapture

# Run a single test
cargo test -p dbcompass <test_name>

# Run tests for a specific plugin
cargo test -p sqlite-plugin
```

## Plugin Loading at Runtime

The app loads plugins (`.so` / `.dll` / `.dylib`) from the same directory as the compiled binary. After building, the plugins must be present alongside the `dbcompass` executable. On Linux with the default cargo target directory layout, the plugins are already in the same `target/debug/` directory as the binary, so this typically works directly:

```bash
cargo build --workspace && cargo run -p dbcompass
```

## Architecture

### Workspace Structure

```
dbcompass/                   # Root workspace
├── plugin-interface/        # Library crate: shared types, traits, FFI symbols
├── plugins/
│   ├── hello-plugin/        # Example plugin (no DB connector, Plugin trait only)
│   ├── sqlite-plugin/       # SQLite connector (rusqlite)
│   ├── mysql-plugin/        # MySQL connector (mysql crate)
│   └── postgres-plugin/     # PostgreSQL connector (postgres crate)
├── src/                     # Main application
│   ├── main.rs              # Window setup, event callbacks, CSV/JSON export
│   ├── app.rs               # AppState: all shared mutable state
│   ├── bridge.rs            # Glue layer: Slint UI ↔ backend logic
│   ├── plugin_manager.rs    # Dynamic library loading via libloading
│   ├── connection_manager.rs # Connection persistence (JSON files on disk)
│   ├── query_executor.rs    # Background-thread query execution
│   ├── sql_completion.rs    # Built-in offline SQL autocompletion engine
│   └── models/
│       └── query_result.rs  # QueryResultView: plugin types → Slint-compatible strings
├── ui/                      # Slint .slint files
│   ├── app.slint            # MainWindow: toolbar, layout, all property/callback declarations
│   ├── connection-tree.slint
│   ├── connection-form-dialog.slint
│   ├── query-editor.slint
│   ├── data-grid.slint
│   ├── status-bar.slint
│   └── slintcn/             # Slint Component Library (theme + reusable components)
└── build.rs                 # Compiles ui/app.slint → generated Rust code
```

### Plugin System

Plugins are `cdylib` crates that get dynamically loaded at runtime via `libloading`. The FFI boundary is a single exported symbol `_plugin_declaration` returning a `PluginDeclaration` struct containing:

- `interface_version`: checked against the host's `PLUGIN_INTERFACE_VERSION` for ABI compatibility
- `plugin_constructor`: `Option<fn() -> Box<dyn Plugin>>` — generic plugin metadata + lifecycle hooks
- `database_connector_constructor`: `Option<fn() -> Box<dyn DatabaseConnector>>` — the core DB trait

The `plugin-interface` crate provides three declarative macros:
- `declare_plugin!($plugin_ty, $ctor)` — plugin without a DB connector
- `declare_database_connector!($ty, $ctor)` — DB connector without a plugin
- `declare_plugin!($plugin_ty, $ctor, $db_ty, $db_ctor)` — both

### DatabaseConnector Trait (the plugin contract)

Defined in `plugin-interface/src/database.rs`. All DB connectors implement this trait. All methods take `&self` — implementors use interior mutability (`Mutex<HashMap<u64, Mutex<Connection>>>`) to manage multiple connections. The trait provides: `name`, `connect`, `disconnect`, `is_connected`, `list_databases`, `list_tables`, `get_table_columns`, `execute_query`, `execute_update`, `quote_identifier`.

### Key Types (plugin-interface/src/types.rs)

- `ConnectorConfig`: host, port, database, username, password, optional connection_string and extra_params
- `QueryResult`: columns, rows (as `Vec<Vec<CellValue>>`), rows_affected, execution_time_ns, has_more
- `CellValue`: display string, raw_type (ColumnType enum), is_null flag
- `ColumnInfo`: name, data_type, nullable, primary_key, default_value
- `TableInfo`: name, schema, table_type, columns, row_count
- `DbError`: rich error enum (ConnectionFailed, QueryFailed, Disconnected, InvalidConnection, NotFound, Timeout, Unsupported)
- `ColumnType`: comprehensive enum covering all major SQL types across dialects

### Data Flow: Query Execution

1. User types SQL and hits Run → `MainWindow.on_execute_query` callback fires
2. `bridge.rs` resolves which connection to use (active selected or first available)
3. `query_executor::execute_query()` spawns a `std::thread` to run the query off the main thread
4. The plugin's `DatabaseConnector::execute_query()` runs on the background thread
5. Result is marshaled into a `QueryResultView` (plain strings for Slint compatibility)
6. `slint::invoke_from_event_loop()` delivers the result back to the UI thread
7. `bridge::apply_query_result()` pushes columns/rows into Slint models

The query executor also catches panics from plugins via `std::panic::catch_unwind`.

### SQL Completion Engine

`SqlCompleter` in `sql_completion.rs` provides offline, context-aware SQL autocompletion. It has:

- **Built-in keywords** (~150): categorized as DML, DDL, TCL, DCL, clause, operator, etc., with optional snippet templates
- **Built-in functions** (~100): categorized as aggregate, string, math, datetime, conversion, conditional, json, window, system
- **Built-in data types** (~60): integers, floats, strings, binary, dates, JSON, spatial types
- **Runtime metadata**: table names and column names fed from connected databases (via `set_tables()` and `set_columns_for_table()`, called from `bridge.rs` when expanding tree nodes)

The `detect_context()` function analyzes the SQL text before the cursor to determine the syntactic context (SelectExpr, FromClause, WhereClause, JoinClause, OrderBy, GroupBy, TableDef, etc.) and filters suggestions accordingly.

### Connection Persistence

Saved connections are stored as individual JSON files under `<data_dir>/connections/<name>.json`. The `ConnectionManager` reads all `.json` files from this directory on startup. **Note:** `ConnectorConfig` is serialized with serde, including the password — passwords are stored in plaintext JSON on disk.

### UI Architecture (Slint + slintcn)

- `build.rs` compiles `ui/app.slint` into Rust code embedded via `slint::include_modules!()`
- The UI uses `slintcn` (Slint Component Library) with a "zinc" base color theme
- Custom components are in `ui/`; reusable themed components are in `ui/slintcn/components/`
- The theme system supports dark/light mode toggling via `Theme.mode`
- UI ↔ Rust communication is through Slint properties and callbacks — all UI callbacks are set up in `main.rs`

### Active vs Saved Connections

- **Saved connections** are on-disk JSON files managed by `ConnectionManager`
- **Active connections** are live database connections tracked in `AppState.active_connections: HashMap<String, ActiveConnection>` (maps connection name → conn_id + connector_name)
- Disconnecting removes from active connections but keeps the saved JSON; deleting both disconnects and removes the file

## Adding a New Database Connector

1. Create a new directory under `plugins/` with its own `Cargo.toml` (crate-type = `["cdylib"]`)
2. Depend on `plugin-interface` (path = `"../../plugin-interface"`)
3. Add the appropriate database driver crate
4. Implement the `DatabaseConnector` trait using interior mutability (`Mutex<HashMap<u64, Mutex<Connection>>>`)
5. Use `plugin_interface::declare_plugin!` or `declare_database_connector!` to export the FFI symbol
6. Add the new crate to the workspace `Cargo.toml` members list

## Important Notes

- Edition is **2024** — uses `#[unsafe(no_mangle)]` instead of `#[no_mangle]` for FFI exports
- The `plugin-interface` crate has a `serialize` feature (optional serde) — the main app enables it
- Query results are capped at 1000 rows by default in the UI layer (bridge passes `Some(1000)` to `execute_query`)
- Tree node IDs use `__` as a separator: `"connName__tableName"` for tables, `"connName__tableName__colName"` for columns
- The `hello-plugin` demonstrates a plugin without a DB connector — only implements the `Plugin` trait with lifecycle hooks
- `string` fields in Slint use `slint::SharedString` in Rust — use `.to_string()` and `slint::SharedString::from()`
