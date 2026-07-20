use plugin_interface::{
    CellValue, ColumnInfo, ColumnType, ConnectorConfig, DatabaseConnector, DbError, Plugin,
    PluginMeta, QueryResult, TableInfo,
};
use std::collections::HashMap;
use std::sync::Mutex;

struct SqlitePlugin;
impl Plugin for SqlitePlugin {
    fn metadata(&self) -> PluginMeta {
        PluginMeta {
            name: "sqlite-plugin",
            version: "0.1.0",
            description: "SQLite database connector plugin",
        }
    }
}

pub struct SqliteConnector {
    connections: Mutex<HashMap<u64, Mutex<rusqlite::Connection>>>,
    next_id: Mutex<u64>,
}

impl SqliteConnector {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
        }
    }
}

impl Default for SqliteConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl DatabaseConnector for SqliteConnector {
    fn name(&self) -> &'static str {
        "SQLite"
    }

    fn connect(&self, config: &ConnectorConfig) -> Result<u64, DbError> {
        let path = if !config.database.is_empty() {
            &config.database
        } else if let Some(ref cs) = config.connection_string {
            cs
        } else {
            return Err(DbError::ConnectionFailed(
                "No database path provided".into(),
            ));
        };

        let conn = rusqlite::Connection::open(path)
            .map_err(|e| DbError::ConnectionFailed(e.to_string()))?;

        let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");

        let mut id_lock = self.next_id.lock().unwrap();
        let conn_id = *id_lock;
        *id_lock += 1;

        let mut map = self.connections.lock().unwrap();
        map.insert(conn_id, Mutex::new(conn));
        Ok(conn_id)
    }

    fn disconnect(&self, conn_id: u64) -> Result<(), DbError> {
        let mut map = self.connections.lock().unwrap();
        map.remove(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        Ok(())
    }

    fn is_connected(&self, conn_id: u64) -> bool {
        self.connections.lock().unwrap().contains_key(&conn_id)
    }

    fn list_databases(&self, conn_id: u64) -> Result<Vec<String>, DbError> {
        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let conn = conn_mutex.lock().unwrap();

        let mut stmt = conn
            .prepare("PRAGMA database_list")
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;
        let dbs: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| DbError::QueryFailed(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(dbs)
    }

    fn list_tables(&self, conn_id: u64, _schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let conn = conn_mutex.lock().unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT name, type FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        let tables: Vec<TableInfo> = stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let tbl_type: String = row.get(1)?;
                Ok(TableInfo {
                    name,
                    schema: Some("main".into()),
                    table_type: tbl_type.to_uppercase(),
                    columns: Vec::new(),
                    row_count: None,
                })
            })
            .map_err(|e| DbError::QueryFailed(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tables)
    }

    fn get_table_columns(
        &self,
        conn_id: u64,
        _schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, DbError> {
        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let conn = conn_mutex.lock().unwrap();

        let sql = format!("PRAGMA table_info(\"{}\")", table.replace('"', "\"\""));
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        let columns: Vec<ColumnInfo> = stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let col_type: String = row.get(2)?;
                let not_null: bool = row.get::<_, i32>(3)? != 0;
                let pk: bool = row.get::<_, i32>(5)? != 0;
                let default: Option<String> = row.get(4)?;
                Ok(ColumnInfo {
                    name,
                    data_type: sqlite_type_str_to_column_type(&col_type),
                    nullable: !not_null,
                    primary_key: pk,
                    default_value: default.filter(|s: &String| !s.is_empty()),
                })
            })
            .map_err(|e| DbError::QueryFailed(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(columns)
    }

    fn execute_query(
        &self,
        conn_id: u64,
        sql: &str,
        max_rows: Option<usize>,
    ) -> Result<QueryResult, DbError> {
        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let conn = conn_mutex.lock().unwrap();
        let start = std::time::Instant::now();

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        let col_count = stmt.column_count();
        let columns: Vec<ColumnInfo> = (0..col_count)
            .map(|i| ColumnInfo {
                name: stmt.column_name(i).unwrap_or("?").to_string(),
                data_type: ColumnType::String,
                nullable: true,
                primary_key: false,
                default_value: None,
            })
            .collect();

        if columns.is_empty() {
            let rows_affected = stmt
                .execute([])
                .map_err(|e| DbError::QueryFailed(e.to_string()))?;
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                rows_affected: rows_affected as u64,
                execution_time_ns: start.elapsed().as_nanos(),
                has_more: false,
            });
        }

        let limit = max_rows.unwrap_or(usize::MAX);
        let mut rows: Vec<Vec<CellValue>> = Vec::new();

        let row_iter = stmt
            .query_map([], |row| {
                let mut cells = Vec::new();
                for i in 0..col_count {
                    let val: rusqlite::types::Value = row.get_unwrap(i);
                    cells.push(match val {
                        rusqlite::types::Value::Null => CellValue {
                            display: None,
                            raw_type: ColumnType::Null,
                            is_null: true,
                        },
                        rusqlite::types::Value::Integer(v) => CellValue {
                            display: Some(v.to_string()),
                            raw_type: ColumnType::Int64,
                            is_null: false,
                        },
                        rusqlite::types::Value::Real(v) => CellValue {
                            display: Some(v.to_string()),
                            raw_type: ColumnType::Float64,
                            is_null: false,
                        },
                        rusqlite::types::Value::Text(s) => CellValue {
                            display: Some(s),
                            raw_type: ColumnType::String,
                            is_null: false,
                        },
                        rusqlite::types::Value::Blob(b) => CellValue {
                            display: Some(format!("<blob {} bytes>", b.len())),
                            raw_type: ColumnType::Binary,
                            is_null: false,
                        },
                    });
                }
                Ok(cells)
            })
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        for row_result in row_iter {
            if rows.len() >= limit {
                break;
            }
            match row_result {
                Ok(cells) => rows.push(cells),
                Err(e) => {
                    return Err(DbError::QueryFailed(e.to_string()));
                }
            }
        }

        let has_more = rows.len() >= limit;

        Ok(QueryResult {
            columns,
            rows,
            rows_affected: 0,
            execution_time_ns: start.elapsed().as_nanos(),
            has_more,
        })
    }

    fn execute_update(&self, conn_id: u64, sql: &str) -> Result<u64, DbError> {
        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let conn = conn_mutex.lock().unwrap();

        let count = conn
            .execute(sql, [])
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;
        Ok(count as u64)
    }

    fn quote_identifier(&self, ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }
}

fn sqlite_type_str_to_column_type(t: &str) -> ColumnType {
    match t.to_uppercase().as_str() {
        "INT" | "INTEGER" | "BIGINT" | "INT64" => ColumnType::Int64,
        "INT2" | "SMALLINT" | "INT16" => ColumnType::Int16,
        "INT8" | "TINYINT" => ColumnType::Int8,
        "UNSIGNED BIGINT" | "UINT64" => ColumnType::UInt64,
        "FLOAT" | "DOUBLE" | "REAL" => ColumnType::Float64,
        "NUMERIC" | "DECIMAL" => ColumnType::Decimal(0, 0),
        "BOOLEAN" | "BOOL" => ColumnType::Bool,
        "BLOB" => ColumnType::Binary,
        "DATE" => ColumnType::Date,
        "DATETIME" | "TIMESTAMP" => ColumnType::DateTime,
        "TEXT" | "VARCHAR" | "CHAR" | "CLOB" | "STRING" => ColumnType::String,
        "JSON" => ColumnType::Json,
        "UUID" => ColumnType::Uuid,
        _ => {
            if t.is_empty() {
                ColumnType::String
            } else {
                ColumnType::Other(t.to_string())
            }
        }
    }
}

plugin_interface::declare_plugin!(
    SqlitePlugin,
    SqlitePlugin,
    SqliteConnector,
    SqliteConnector::new()
);
