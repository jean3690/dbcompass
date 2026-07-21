#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnType {
    Null,
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Decimal(u8, u8),
    String,
    Binary,
    DateTime,
    DateTimeTz,
    Date,
    Time,
    TimeTz,
    Interval,
    Json,
    JsonB,
    Uuid,
    Array(Box<ColumnType>),
    Enum(Vec<String>),
    Other(String),
}

#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct CellValue {
    pub display: Option<String>,
    pub raw_type: ColumnType,
    pub is_null: bool,
}

#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: ColumnType,
    pub nullable: bool,
    pub primary_key: bool,
    pub default_value: Option<String>,
}

#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<CellValue>>,
    pub rows_affected: u64,
    #[cfg_attr(feature = "serialize", serde(skip))]
    pub execution_time_ns: u128,
    pub has_more: bool,
}

#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    pub schema: Option<String>,
    pub table_type: String,
    pub columns: Vec<ColumnInfo>,
    pub row_count: Option<u64>,
}

#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct ConnectorConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub connection_string: Option<String>,
    #[serde(default)]
    pub extra_params: Vec<(String, String)>,
}

#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum DbError {
    ConnectionFailed(String),
    QueryFailed(String),
    Disconnected(String),
    InvalidConnection(String),
    NotFound(String),
    Timeout(String),
    Unsupported(String),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            DbError::QueryFailed(msg) => write!(f, "Query failed: {}", msg),
            DbError::Disconnected(msg) => write!(f, "Disconnected: {}", msg),
            DbError::InvalidConnection(msg) => write!(f, "Invalid connection: {}", msg),
            DbError::NotFound(msg) => write!(f, "Not found: {}", msg),
            DbError::Timeout(msg) => write!(f, "Timeout: {}", msg),
            DbError::Unsupported(msg) => write!(f, "Unsupported: {}", msg),
        }
    }
}
