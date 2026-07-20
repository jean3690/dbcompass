use crate::types::*;

/// A database connector plugin.
///
/// All methods take `&self` — implementors use interior mutability
/// (`Mutex<HashMap<u64, Mutex<Connection>>>`) to manage multiple connections.
pub trait DatabaseConnector: Send + Sync {
    /// Human-readable name, e.g. "SQLite", "PostgreSQL"
    fn name(&self) -> &'static str;

    /// Open a new connection. Returns an opaque connection handle.
    fn connect(&self, config: &ConnectorConfig) -> Result<u64, DbError>;

    /// Close a connection.
    fn disconnect(&self, conn_id: u64) -> Result<(), DbError>;

    /// Check if a connection is still alive.
    fn is_connected(&self, conn_id: u64) -> bool;

    /// List all databases/schemas visible to this connection.
    fn list_databases(&self, conn_id: u64) -> Result<Vec<String>, DbError>;

    /// List tables in the given schema.
    fn list_tables(&self, conn_id: u64, schema: &str) -> Result<Vec<TableInfo>, DbError>;

    /// Get column info for a specific table.
    fn get_table_columns(
        &self,
        conn_id: u64,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, DbError>;

    /// Execute a SELECT / query and return results.
    fn execute_query(
        &self,
        conn_id: u64,
        sql: &str,
        max_rows: Option<usize>,
    ) -> Result<QueryResult, DbError>;

    /// Execute a INSERT / UPDATE / DELETE and return rows affected.
    fn execute_update(&self, conn_id: u64, sql: &str) -> Result<u64, DbError>;

    /// Quote an identifier for this dialect.
    fn quote_identifier(&self, ident: &str) -> String;
}
