use plugin_interface::{
    CellValue, ColumnInfo, ColumnType, ConnectorConfig, DatabaseConnector, DbError, Plugin,
    PluginMeta, QueryResult, TableInfo,
};
use std::collections::HashMap;
use std::sync::Mutex;

struct PostgresPlugin;
impl Plugin for PostgresPlugin {
    fn metadata(&self) -> PluginMeta {
        PluginMeta {
            name: "postgres-plugin",
            version: "0.1.0",
            description: "PostgreSQL database connector plugin",
        }
    }
}

pub struct PostgresConnector {
    connections: Mutex<HashMap<u64, Mutex<postgres::Client>>>,
    next_id: Mutex<u64>,
}

impl PostgresConnector {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
        }
    }
}

impl Default for PostgresConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl DatabaseConnector for PostgresConnector {
    fn name(&self) -> &'static str {
        "PostgreSQL"
    }

    fn connect(&self, config: &ConnectorConfig) -> Result<u64, DbError> {
        let conn_str = if !config.host.is_empty() {
            {
                let mut conn_str = format!(
                    "host={} port={} user={} password={}",
                    config.host, config.port, config.username, config.password
                );
                if !config.database.is_empty() {
                    conn_str.push_str(&format!(" dbname={}", config.database));
                }
                conn_str
            }
        } else if let Some(ref cs) = config.connection_string {
            cs.clone()
        } else {
            return Err(DbError::ConnectionFailed(
                "No connection info provided".into(),
            ));
        };

        let client = postgres::Client::connect(&conn_str, postgres::NoTls)
            .map_err(|e| DbError::ConnectionFailed(e.to_string()))?;

        let mut id_lock = self.next_id.lock().unwrap_or_else(|e| e.into_inner());
        let conn_id = *id_lock;
        *id_lock += 1;

        let mut map = self.connections.lock().unwrap();
        map.insert(conn_id, Mutex::new(client));
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
        let mut conn = conn_mutex.lock().unwrap();

        let rows = conn
            .query(
                "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
                &[],
            )
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
    }

    fn list_tables(&self, conn_id: u64, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let mut conn = conn_mutex.lock().unwrap();

        let schema_name = if schema.is_empty() || schema == "main" {
            "public"
        } else {
            schema
        };

        let rows = conn
            .query(
                "SELECT table_name, table_type FROM information_schema.tables \
                 WHERE table_schema = $1 ORDER BY table_name",
                &[&schema_name],
            )
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        let tables: Vec<TableInfo> = rows
            .iter()
            .map(|r| {
                let name: String = r.get(0);
                let tbl_type: String = r.get(1);
                TableInfo {
                    name,
                    schema: Some(schema_name.to_string()),
                    table_type: if tbl_type == "VIEW" {
                        "VIEW".into()
                    } else {
                        "TABLE".into()
                    },
                    columns: Vec::new(),
                    row_count: None,
                }
            })
            .collect();

        Ok(tables)
    }

    fn get_table_columns(
        &self,
        conn_id: u64,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, DbError> {
        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let mut conn = conn_mutex.lock().unwrap();

        let schema_name = if schema.is_empty() || schema == "main" {
            "public"
        } else {
            schema
        };

        let rows = conn
            .query(
                "SELECT c.column_name, c.data_type, c.is_nullable, \
                 CASE WHEN pk.column_name IS NOT NULL THEN true ELSE false END, \
                 c.column_default \
                 FROM information_schema.columns c \
                 LEFT JOIN (SELECT ku.column_name FROM information_schema.table_constraints tc \
                     JOIN information_schema.key_column_usage ku \
                     ON tc.constraint_name = ku.constraint_name \
                     AND tc.table_schema = ku.table_schema \
                     WHERE tc.constraint_type = 'PRIMARY KEY' \
                     AND tc.table_schema = $1 AND tc.table_name = $2) pk \
                 ON c.column_name = pk.column_name \
                 WHERE c.table_schema = $1 AND c.table_name = $2 \
                 ORDER BY c.ordinal_position",
                &[&schema_name, &table],
            )
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        let columns: Vec<ColumnInfo> = rows
            .iter()
            .map(|r| {
                let name: String = r.get(0);
                let raw_type: String = r.get(1);
                let nullable: String = r.get(2);
                let pk: bool = r.get(3);
                let default: Option<String> = r.get(4);
                ColumnInfo {
                    name,
                    data_type: pg_type_str_to_column_type(&raw_type),
                    nullable: nullable == "YES",
                    primary_key: pk,
                    default_value: default,
                }
            })
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
        let mut conn = conn_mutex.lock().unwrap();
        let start = std::time::Instant::now();

        let stmt = conn
            .prepare(sql)
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        let columns: Vec<ColumnInfo> = stmt
            .columns()
            .iter()
            .map(|col| ColumnInfo {
                name: col.name().to_string(),
                data_type: pg_type_str_to_column_type(col.type_().name()),
                nullable: true,
                primary_key: false,
                default_value: None,
            })
            .collect();

        if columns.is_empty() {
            let rows_affected = conn
                .execute(&stmt, &[])
                .map_err(|e| DbError::QueryFailed(e.to_string()))?;
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                rows_affected,
                execution_time_ns: start.elapsed().as_nanos(),
                has_more: false,
            });
        }

        let limit = max_rows.unwrap_or(usize::MAX);
        let mut rows: Vec<Vec<CellValue>> = Vec::new();

        let row_iter = conn
            .query(&stmt, &[])
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        for pg_row in row_iter.iter() {
            if rows.len() >= limit {
                break;
            }
            let mut cells = Vec::new();
            for i in 0..columns.len() {
                let cell = pg_cell_to_value(pg_row, i);
                cells.push(cell);
            }
            rows.push(cells);
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
        let mut conn = conn_mutex.lock().unwrap();

        let count = conn
            .execute(sql, &[])
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;
        Ok(count)
    }

    fn quote_identifier(&self, ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }
}

fn pg_cell_to_value(row: &postgres::Row, idx: usize) -> CellValue {
    // Try getting as string first (most types can be cast)
    if let Ok(val) = row.try_get::<_, Option<String>>(idx) {
        return match val {
            Some(s) => CellValue {
                display: Some(s),
                raw_type: ColumnType::String,
                is_null: false,
            },
            None => CellValue {
                display: None,
                raw_type: ColumnType::Null,
                is_null: true,
            },
        };
    }
    // Fallback
    CellValue {
        display: Some(format!("{:?}", row.try_get::<_, Option<bool>>(idx))),
        raw_type: ColumnType::String,
        is_null: false,
    }
}

fn pg_type_str_to_column_type(t: &str) -> ColumnType {
    match t {
        "bool" => ColumnType::Bool,
        "int2" | "smallint" => ColumnType::Int16,
        "int4" | "integer" => ColumnType::Int32,
        "int8" | "bigint" => ColumnType::Int64,
        "float4" | "real" => ColumnType::Float32,
        "float8" | "double precision" => ColumnType::Float64,
        "numeric" | "decimal" => ColumnType::Decimal(0, 0),
        "bytea" => ColumnType::Binary,
        "date" => ColumnType::Date,
        "time" | "timetz" => ColumnType::Time,
        "timestamp" => ColumnType::DateTime,
        "timestamptz" => ColumnType::DateTimeTz,
        "interval" => ColumnType::Interval,
        "json" => ColumnType::Json,
        "jsonb" => ColumnType::JsonB,
        "uuid" => ColumnType::Uuid,
        "text" | "varchar" | "char" | "bpchar" | "name" | "citext" => ColumnType::String,
        _ if t.starts_with('_') => ColumnType::Array(Box::new(ColumnType::String)),
        _ => ColumnType::Other(t.to_string()),
    }
}

plugin_interface::declare_plugin!(
    PostgresPlugin,
    PostgresPlugin,
    PostgresConnector,
    PostgresConnector::new()
);
