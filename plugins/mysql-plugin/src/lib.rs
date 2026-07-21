use mysql::prelude::Queryable;
use plugin_interface::{
    CellValue, ColumnInfo, ColumnType, ConnectorConfig, DatabaseConnector, DbError, Plugin,
    PluginMeta, QueryResult, TableInfo,
};
use std::collections::HashMap;
use std::sync::Mutex;

struct MySqlPlugin;
impl Plugin for MySqlPlugin {
    fn metadata(&self) -> PluginMeta {
        PluginMeta {
            name: "mysql-plugin",
            version: "0.1.0",
            description: "MySQL database connector plugin",
        }
    }
}

pub struct MySqlConnector {
    connections: Mutex<HashMap<u64, Mutex<mysql::Conn>>>,
    next_id: Mutex<u64>,
}

impl MySqlConnector {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
        }
    }
}

impl Default for MySqlConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl DatabaseConnector for MySqlConnector {
    fn name(&self) -> &'static str {
        "MySQL"
    }

    fn connect(&self, config: &ConnectorConfig) -> Result<u64, DbError> {
        let opts = mysql::OptsBuilder::new()
            .ip_or_hostname(Some(&config.host))
            .tcp_port(config.port)
            .db_name(if config.database.is_empty() {
                None
            } else {
                Some(config.database.as_str())
            })
            .user(Some(&config.username))
            .pass(Some(&config.password));

        let conn = mysql::Conn::new(opts).map_err(|e| DbError::ConnectionFailed(e.to_string()))?;

        let mut id_lock = self.next_id.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut conn = conn_mutex.lock().unwrap();

        conn.query_map("SHOW DATABASES", |row: mysql::Row| {
            let name: String = row.get(0).unwrap_or_default();
            name
        })
        .map_err(|e| DbError::QueryFailed(e.to_string()))
    }

    fn list_tables(&self, conn_id: u64, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let mut conn = conn_mutex.lock().unwrap();

        let db_name = if schema.is_empty() || schema == "main" {
            conn.query_first::<String, _>("SELECT DATABASE()")
                .ok()
                .flatten()
                .unwrap_or_else(|| "public".into())
        } else {
            schema.to_string()
        };

        let sql = format!(
            "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = '{}' ORDER BY TABLE_NAME",
            db_name.replace('\'', "''")
        );

        conn.query_map(sql, |row: mysql::Row| {
            let name: String = row.get(0).unwrap_or_default();
            let tbl_type: String = row.get(1).unwrap_or_default();
            TableInfo {
                name,
                schema: Some(db_name.clone()),
                table_type: if tbl_type.to_uppercase().contains("VIEW") {
                    "VIEW".into()
                } else {
                    "TABLE".into()
                },
                columns: Vec::new(),
                row_count: None,
            }
        })
        .map_err(|e| DbError::QueryFailed(e.to_string()))
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

        let db_name = if schema.is_empty() || schema == "main" {
            conn.query_first::<String, _>("SELECT DATABASE()")
                .ok()
                .flatten()
                .unwrap_or_else(|| "public".into())
        } else {
            schema.to_string()
        };

        let sql = format!(
            "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE, COALESCE(COLUMN_KEY,''), \
             COALESCE(COLUMN_DEFAULT,'') \
             FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = '{}' AND TABLE_NAME = '{}' \
             ORDER BY ORDINAL_POSITION",
            db_name.replace('\'', "''"),
            table.replace('\'', "''")
        );

        conn.query_map(sql, |row: mysql::Row| {
            let name: String = row.get(0).unwrap_or_default();
            let raw_type: String = row.get(1).unwrap_or_default();
            let nullable: String = row.get(2).unwrap_or_default();
            let pk: String = row.get(3).unwrap_or_default();
            let def: String = row.get(4).unwrap_or_default();
            ColumnInfo {
                name,
                data_type: mysql_type_str_to_column_type(&raw_type),
                nullable: nullable.to_uppercase() == "YES",
                primary_key: pk.to_uppercase() == "PRI",
                default_value: if def.is_empty() { None } else { Some(def) },
            }
        })
        .map_err(|e| DbError::QueryFailed(e.to_string()))
    }

    fn execute_query(
        &self,
        conn_id: u64,
        sql: &str,
        max_rows: Option<usize>,
    ) -> Result<QueryResult, DbError> {
        use mysql::prelude::*;

        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let mut conn = conn_mutex.lock().unwrap();
        let start = std::time::Instant::now();

        let limit = max_rows.unwrap_or(usize::MAX);
        let mut rows: Vec<Vec<CellValue>> = Vec::new();

        let result = conn
            .exec_iter(sql, ())
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;

        let col_set = result.columns();
        let col_refs = col_set.as_ref();
        let columns: Vec<ColumnInfo> = col_refs
            .iter()
            .map(|col| ColumnInfo {
                name: col.name_str().to_string(),
                data_type: mysql_type_str_to_column_type(col.name_str().as_ref()),
                nullable: true,
                primary_key: false,
                default_value: None,
            })
            .collect();

        for row_result in result.into_iter() {
            if rows.len() >= limit {
                break;
            }
            match row_result {
                Ok(row) => {
                    let mut cells = Vec::new();
                    for i in 0..columns.len() {
                        let val: Option<mysql::Value> = row.get(i);
                        cells.push(mysql_value_to_cell(val));
                    }
                    rows.push(cells);
                }
                Err(e) => {
                    return Err(DbError::QueryFailed(e.to_string()));
                }
            }
        }

        let elapsed = start.elapsed().as_nanos();
        let has_more = rows.len() >= limit;
        Ok(QueryResult {
            columns,
            rows,
            rows_affected: 0,
            execution_time_ns: elapsed,
            has_more,
        })
    }

    fn execute_update(&self, conn_id: u64, sql: &str) -> Result<u64, DbError> {
        use mysql::prelude::Queryable;

        let map = self.connections.lock().unwrap();
        let conn_mutex = map
            .get(&conn_id)
            .ok_or(DbError::InvalidConnection("Connection not found".into()))?;
        let mut conn = conn_mutex.lock().unwrap();

        let result = conn
            .exec_iter(sql, ())
            .map_err(|e| DbError::QueryFailed(e.to_string()))?;
        let affected = result.affected_rows();
        Ok(affected)
    }

    fn quote_identifier(&self, ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }
}

fn mysql_type_str_to_column_type(t: &str) -> ColumnType {
    match t.to_uppercase().as_str() {
        "TINYINT" => ColumnType::Int8,
        "SMALLINT" => ColumnType::Int16,
        "MEDIUMINT" | "INT" | "INTEGER" => ColumnType::Int32,
        "BIGINT" => ColumnType::Int64,
        "UNSIGNED TINYINT" => ColumnType::UInt8,
        "UNSIGNED SMALLINT" => ColumnType::UInt16,
        "UNSIGNED INT" | "UNSIGNED INTEGER" => ColumnType::UInt32,
        "UNSIGNED BIGINT" => ColumnType::UInt64,
        "FLOAT" => ColumnType::Float32,
        "DOUBLE" => ColumnType::Float64,
        "DECIMAL" | "NUMERIC" => ColumnType::Decimal(0, 0),
        "CHAR" | "VARCHAR" | "TINYTEXT" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT" | "ENUM" | "SET" => {
            ColumnType::String
        }
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => {
            ColumnType::Binary
        }
        "DATE" => ColumnType::Date,
        "TIME" => ColumnType::Time,
        "DATETIME" => ColumnType::DateTime,
        "TIMESTAMP" => ColumnType::DateTimeTz,
        "YEAR" => ColumnType::Int16,
        "JSON" => ColumnType::Json,
        "BOOLEAN" | "BOOL" => ColumnType::Bool,
        _ => ColumnType::Other(t.to_string()),
    }
}

fn mysql_value_to_cell(val: Option<mysql::Value>) -> CellValue {
    match val {
        None => CellValue {
            display: None,
            raw_type: ColumnType::Null,
            is_null: true,
        },
        Some(mysql::Value::NULL) => CellValue {
            display: None,
            raw_type: ColumnType::Null,
            is_null: true,
        },
        Some(mysql::Value::Int(v)) => CellValue {
            display: Some(v.to_string()),
            raw_type: ColumnType::Int64,
            is_null: false,
        },
        Some(mysql::Value::UInt(v)) => CellValue {
            display: Some(v.to_string()),
            raw_type: ColumnType::UInt64,
            is_null: false,
        },
        Some(mysql::Value::Float(v)) => CellValue {
            display: Some(v.to_string()),
            raw_type: ColumnType::Float32,
            is_null: false,
        },
        Some(mysql::Value::Double(v)) => CellValue {
            display: Some(v.to_string()),
            raw_type: ColumnType::Float64,
            is_null: false,
        },
        Some(mysql::Value::Date(y, m, d, h, mi, s, us)) => {
            if h == 0 && mi == 0 && s == 0 && us == 0 {
                CellValue {
                    display: Some(format!("{:04}-{:02}-{:02}", y, m, d)),
                    raw_type: ColumnType::Date,
                    is_null: false,
                }
            } else {
                CellValue {
                    display: Some(format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                        y, m, d, h, mi, s
                    )),
                    raw_type: ColumnType::DateTime,
                    is_null: false,
                }
            }
        }
        Some(mysql::Value::Time(neg, d, h, mi, s, us)) => {
            let sign = if neg { "-" } else { "" };
            CellValue {
                display: Some(format!(
                    "{}{} days {:02}:{:02}:{:02}.{:06}",
                    sign, d, h, mi, s, us
                )),
                raw_type: ColumnType::Interval,
                is_null: false,
            }
        }
        Some(mysql::Value::Bytes(b)) => {
            if let Ok(s) = String::from_utf8(b.clone()) {
                CellValue {
                    display: Some(s),
                    raw_type: ColumnType::String,
                    is_null: false,
                }
            } else {
                CellValue {
                    display: Some(format!("<binary {} bytes>", b.len())),
                    raw_type: ColumnType::Binary,
                    is_null: false,
                }
            }
        }
    }
}

plugin_interface::declare_plugin!(
    MySqlPlugin,
    MySqlPlugin,
    MySqlConnector,
    MySqlConnector::new()
);
