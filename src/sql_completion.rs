// ──────────────────────────────────────────────────────────
//  SQL Completion Engine — built-in standard syntax
//  Works offline without any database connection.
//  Table/column metadata from connected databases enriches it.
// ──────────────────────────────────────────────────────────

use std::collections::HashSet;

/// A single completion suggestion.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub detail: String, // "keyword" | "function" | "type" | "snippet" | "table" | "column"
    pub insert_text: String,
}

/// Context the cursor is in.
#[derive(Debug, PartialEq)]
enum SqlContext {
    Global,      // top-level statement
    SelectExpr,  // after SELECT / comma
    FromClause,  // after FROM / JOIN / comma
    WhereClause, // after WHERE / AND / OR
    JoinClause,  // after JOIN / ON
    OrderBy,     // after ORDER BY
    GroupBy,     // after GROUP BY
    TableDef,    // inside CREATE TABLE
    #[expect(dead_code)]
    InsertCols, // after INSERT INTO table (
    #[expect(dead_code)]
    ValuesClause, // after VALUES
    #[expect(dead_code)]
    OnConflict, // after ON CONFLICT / ON DUPLICATE
}

/// Completion engine for SQL.
pub struct SqlCompleter {
    /// Built-in SQL keywords (always available).
    keywords: Vec<(&'static str, &'static str, Option<&'static str>)>,
    // (keyword, context_hint, snippet_suffix)
    /// Built-in SQL functions (always available).
    functions: Vec<(&'static str, &'static str)>,
    // (name, category)
    /// Built-in SQL data types (always available).
    data_types: Vec<&'static str>,

    /// Table names loaded from active connection (enriched at runtime).
    table_names: Vec<String>,

    /// Map: table_name -> vec of column names (enriched at runtime).
    table_columns: Vec<(String, Vec<String>)>,
}

impl SqlCompleter {
    pub fn new() -> Self {
        Self {
            keywords: Self::build_keywords(),
            functions: Self::build_functions(),
            data_types: Self::build_data_types(),
            table_names: Vec::new(),
            table_columns: Vec::new(),
        }
    }

    // ── Built-in keyword database ──

    fn build_keywords() -> Vec<(&'static str, &'static str, Option<&'static str>)> {
        vec![
            // ── DML: Data Manipulation ──
            ("SELECT", "DML", Some(" * FROM ")),
            ("FROM", "clause", None),
            ("WHERE", "clause", None),
            ("AND", "op", None),
            ("OR", "op", None),
            ("NOT", "op", None),
            ("IN", "op", None),
            ("IS", "op", None),
            ("NULL", "literal", None),
            ("TRUE", "literal", None),
            ("FALSE", "literal", None),
            ("AS", "alias", None),
            ("ON", "clause", None),
            ("JOIN", "clause", None),
            ("LEFT", "clause", Some(" JOIN ")),
            ("RIGHT", "clause", Some(" JOIN ")),
            ("INNER", "clause", Some(" JOIN ")),
            ("OUTER", "clause", None),
            ("FULL", "clause", Some(" JOIN ")),
            ("CROSS", "clause", Some(" JOIN ")),
            ("NATURAL", "clause", Some(" JOIN ")),
            ("LATERAL", "clause", None),
            ("UNION", "set", None),
            ("ALL", "set", None),
            ("INTERSECT", "set", None),
            ("EXCEPT", "set", None),
            ("DISTINCT", "mod", None),
            ("TOP", "mod", None),
            ("LIMIT", "clause", Some(" ")),
            ("OFFSET", "clause", Some(" ")),
            ("FETCH", "clause", Some(" NEXT ")),
            ("ORDER", "clause", Some(" BY ")),
            ("BY", "clause", None),
            ("ASC", "order", None),
            ("DESC", "order", None),
            ("GROUP", "clause", Some(" BY ")),
            ("HAVING", "clause", None),
            // ── DML: Modification ──
            ("INSERT", "DML", Some(" INTO ")),
            ("INTO", "clause", None),
            ("VALUES", "clause", Some(" (")),
            ("UPDATE", "DML", Some(" ")),
            ("SET", "clause", None),
            ("DELETE", "DML", Some(" FROM ")),
            ("REPLACE", "DML", Some(" INTO ")),
            ("MERGE", "DML", Some(" INTO ")),
            ("WHEN", "clause", Some(" MATCHED THEN ")),
            ("THEN", "clause", None),
            ("ELSE", "clause", None),
            ("DEFAULT", "keyword", Some(" VALUES")),
            ("RETURNING", "clause", Some(" *")),
            // ── DDL: Data Definition ──
            ("CREATE", "DDL", Some(" TABLE ")),
            ("TABLE", "DDL", None),
            ("DROP", "DDL", Some(" TABLE ")),
            ("ALTER", "DDL", Some(" TABLE ")),
            ("ADD", "DDL", Some(" COLUMN ")),
            ("COLUMN", "DDL", None),
            ("MODIFY", "DDL", Some(" COLUMN ")),
            ("RENAME", "DDL", Some(" TO ")),
            ("TRUNCATE", "DDL", Some(" TABLE ")),
            ("INDEX", "DDL", None),
            ("VIEW", "DDL", None),
            ("MATERIALIZED", "DDL", Some(" VIEW ")),
            ("TEMPORARY", "mod", Some(" TABLE ")),
            ("TEMP", "mod", Some(" TABLE ")),
            ("IF", "cond", Some(" NOT EXISTS ")),
            ("EXISTS", "cond", None),
            ("PRIMARY", "constraint", Some(" KEY")),
            ("KEY", "constraint", None),
            ("FOREIGN", "constraint", Some(" KEY")),
            ("REFERENCES", "constraint", Some(" (")),
            ("UNIQUE", "constraint", None),
            ("CHECK", "constraint", Some(" (")),
            ("CONSTRAINT", "constraint", None),
            ("CASCADE", "ref_action", None),
            ("RESTRICT", "ref_action", None),
            ("SET", "clause", Some(" NULL")),
            ("NO", "mod", Some(" ACTION")),
            ("ACTION", "ref_action", None),
            ("DEFERRABLE", "constraint", None),
            ("INITIALLY", "constraint", Some(" DEFERRED")),
            ("DEFERRED", "mod", None),
            ("IMMEDIATE", "mod", None),
            ("AUTO_INCREMENT", "col_opt", None),
            ("SERIAL", "type", None),
            ("GENERATED", "col_opt", Some(" ALWAYS AS IDENTITY")),
            ("IDENTITY", "col_opt", None),
            ("STORED", "col_opt", None),
            ("VIRTUAL", "col_opt", None),
            // ── DCL: Data Control ──
            ("GRANT", "DCL", Some(" ")),
            ("REVOKE", "DCL", Some(" ")),
            ("DENY", "DCL", Some(" ")),
            // ── TCL: Transaction Control ──
            ("BEGIN", "TCL", Some(" TRANSACTION")),
            ("COMMIT", "TCL", None),
            ("ROLLBACK", "TCL", None),
            ("SAVEPOINT", "TCL", Some(" ")),
            ("RELEASE", "TCL", Some(" SAVEPOINT ")),
            ("ABORT", "TCL", None),
            ("END", "TCL", Some(" TRANSACTION")),
            // ── Conditional Expressions ──
            ("CASE", "expr", Some(" WHEN ")),
            ("WHEN", "expr", None),
            ("THEN", "expr", None),
            ("ELSE", "expr", None),
            ("END", "expr", None),
            ("COALESCE", "func", Some("(")),
            ("NULLIF", "func", Some("(")),
            ("CAST", "func", Some("(")),
            ("TRY_CAST", "func", Some("(")),
            ("CONVERT", "func", Some("(")),
            // ── Operators ──
            ("BETWEEN", "op", Some(" AND ")),
            ("LIKE", "op", None),
            ("ILIKE", "op", None),
            ("GLOB", "op", None),
            ("MATCH", "op", None),
            ("SIMILAR", "op", Some(" TO ")),
            ("IS", "op", Some(" NULL")),
            ("ISNULL", "op", None),
            ("NOTNULL", "op", None),
            ("IN", "op", Some(" (")),
            ("ANY", "op", None),
            ("SOME", "op", None),
            ("ALL", "op", None),
            // ── Subquery / CTE ──
            ("WITH", "cte", Some(" ")),
            ("RECURSIVE", "cte", None),
            ("EXPLAIN", "util", Some(" ")),
            ("ANALYZE", "util", Some(" ")),
            ("DESCRIBE", "util", Some(" ")),
            ("EXECUTE", "util", Some(" ")),
            ("PREPARE", "util", Some(" ")),
            ("DEALLOCATE", "util", Some(" ")),
            ("CALL", "util", Some(" ")),
            ("DO", "util", Some(" ")),
            // ── Schema / Info ──
            ("USE", "util", Some(" ")),
            ("SHOW", "util", Some(" TABLES")),
            ("DESC", "util", Some(" ")),
            ("PRAGMA", "util", Some(" ")),
            // ── PostgreSQL / Extended ──
            ("LANGUAGE", "DDL", None),
            ("FUNCTION", "DDL", None),
            ("PROCEDURE", "DDL", None),
            ("TRIGGER", "DDL", None),
            ("SCHEMA", "DDL", None),
            ("DATABASE", "DDL", None),
            ("TABLESPACE", "DDL", None),
            ("DOMAIN", "DDL", None),
            ("SEQUENCE", "DDL", None),
            ("EXTENSION", "DDL", Some(" ")),
            ("TYPE", "DDL", None),
            ("ENUM", "type", None),
            ("ARRAY", "type", None),
            ("XML", "type", None),
            ("JSON", "type", None),
            ("JSONB", "type", None),
            ("VACUUM", "util", None),
            ("REINDEX", "util", None),
            ("CLUSTER", "util", None),
            ("LISTEN", "util", None),
            ("NOTIFY", "util", None),
            ("UNLISTEN", "util", None),
            ("DISCARD", "util", None),
            ("REFRESH", "DDL", Some(" MATERIALIZED VIEW ")),
            ("SECURITY", "DCL", None),
            ("INVOKER", "DCL", None),
            ("DEFINER", "DCL", None),
        ]
    }

    // ── Built-in function database ──

    fn build_functions() -> Vec<(&'static str, &'static str)> {
        vec![
            // ── Aggregate ──
            ("COUNT", "aggregate"),
            ("SUM", "aggregate"),
            ("AVG", "aggregate"),
            ("MIN", "aggregate"),
            ("MAX", "aggregate"),
            ("GROUP_CONCAT", "aggregate"),
            ("STRING_AGG", "aggregate"),
            ("ARRAY_AGG", "aggregate"),
            ("JSON_AGG", "aggregate"),
            ("BIT_AND", "aggregate"),
            ("BIT_OR", "aggregate"),
            ("STDDEV", "aggregate"),
            ("STDDEV_POP", "aggregate"),
            ("STDDEV_SAMP", "aggregate"),
            ("VARIANCE", "aggregate"),
            ("VAR_POP", "aggregate"),
            ("VAR_SAMP", "aggregate"),
            ("MODE", "aggregate"),
            ("MEDIAN", "aggregate"),
            ("PERCENTILE_CONT", "aggregate"),
            ("PERCENTILE_DISC", "aggregate"),
            // ── String Functions ──
            ("LENGTH", "string"),
            ("CHAR_LENGTH", "string"),
            ("CHARACTER_LENGTH", "string"),
            ("SUBSTR", "string"),
            ("SUBSTRING", "string"),
            ("TRIM", "string"),
            ("LTRIM", "string"),
            ("RTRIM", "string"),
            ("UPPER", "string"),
            ("LOWER", "string"),
            ("INITCAP", "string"),
            ("CONCAT", "string"),
            ("REPLACE", "string"),
            ("REVERSE", "string"),
            ("REPEAT", "string"),
            ("LPAD", "string"),
            ("RPAD", "string"),
            ("LEFT", "string"),
            ("RIGHT", "string"),
            ("POSITION", "string"),
            ("CHARINDEX", "string"),
            ("STRPOS", "string"),
            ("SPLIT_PART", "string"),
            ("REGEXP_REPLACE", "string"),
            ("REGEXP_MATCHES", "string"),
            ("REGEXP_SPLIT_TO_ARRAY", "string"),
            ("FORMAT", "string"),
            ("ASCII", "string"),
            ("CHR", "string"),
            ("TO_ASCII", "string"),
            ("QUOTE_IDENT", "string"),
            ("QUOTE_LITERAL", "string"),
            ("TRANSLATE", "string"),
            ("MD5", "string"),
            ("SHA256", "string"),
            ("ENCODE", "string"),
            ("DECODE", "string"),
            // ── Numeric / Math ──
            ("ABS", "math"),
            ("CEIL", "math"),
            ("CEILING", "math"),
            ("FLOOR", "math"),
            ("ROUND", "math"),
            ("TRUNC", "math"),
            ("TRUNCATE", "math"),
            ("SIGN", "math"),
            ("MOD", "math"),
            ("POWER", "math"),
            ("SQRT", "math"),
            ("CBRT", "math"),
            ("EXP", "math"),
            ("LN", "math"),
            ("LOG", "math"),
            ("LOG10", "math"),
            ("RADIANS", "math"),
            ("DEGREES", "math"),
            ("SIN", "math"),
            ("COS", "math"),
            ("TAN", "math"),
            ("ASIN", "math"),
            ("ACOS", "math"),
            ("ATAN", "math"),
            ("ATAN2", "math"),
            ("SINH", "math"),
            ("COSH", "math"),
            ("TANH", "math"),
            ("RANDOM", "math"),
            ("RAND", "math"),
            ("GREATEST", "math"),
            ("LEAST", "math"),
            ("WIDTH_BUCKET", "math"),
            // ── Date / Time ──
            ("NOW", "datetime"),
            ("CURRENT_DATE", "datetime"),
            ("CURRENT_TIME", "datetime"),
            ("CURRENT_TIMESTAMP", "datetime"),
            ("LOCALTIME", "datetime"),
            ("LOCALTIMESTAMP", "datetime"),
            ("DATE", "datetime"),
            ("TIME", "datetime"),
            ("TIMESTAMP", "datetime"),
            ("EXTRACT", "datetime"),
            ("DATEPART", "datetime"),
            ("DATEDIFF", "datetime"),
            ("DATEADD", "datetime"),
            ("DATE_TRUNC", "datetime"),
            ("TO_DATE", "datetime"),
            ("TO_TIMESTAMP", "datetime"),
            ("TO_CHAR", "datetime"),
            ("AGE", "datetime"),
            ("JUSTIFY_DAYS", "datetime"),
            ("JUSTIFY_HOURS", "datetime"),
            ("JUSTIFY_INTERVAL", "datetime"),
            ("MAKE_DATE", "datetime"),
            ("MAKE_INTERVAL", "datetime"),
            ("MAKE_TIME", "datetime"),
            ("MAKE_TIMESTAMP", "datetime"),
            // ── Type Conversion ──
            ("CAST", "conversion"),
            ("TRY_CAST", "conversion"),
            ("CONVERT", "conversion"),
            ("TO_NUMBER", "conversion"),
            ("TO_CHAR", "conversion"),
            // ── Conditional ──
            ("COALESCE", "conditional"),
            ("NULLIF", "conditional"),
            ("IFNULL", "conditional"),
            ("NVL", "conditional"),
            ("DECODE", "conditional"),
            ("IIF", "conditional"),
            ("LEAST", "conditional"),
            ("GREATEST", "conditional"),
            // ── JSON ──
            ("JSON_EXTRACT_PATH_TEXT", "json"),
            ("JSONB_EXTRACT_PATH", "json"),
            ("JSON_AGG", "json"),
            ("JSON_BUILD_OBJECT", "json"),
            ("JSON_BUILD_ARRAY", "json"),
            ("JSON_OBJECT", "json"),
            ("JSON_ARRAY", "json"),
            ("JSON_TYPEOF", "json"),
            // ── Window ──
            ("ROW_NUMBER", "window"),
            ("RANK", "window"),
            ("DENSE_RANK", "window"),
            ("NTILE", "window"),
            ("LAG", "window"),
            ("LEAD", "window"),
            ("FIRST_VALUE", "window"),
            ("LAST_VALUE", "window"),
            ("NTH_VALUE", "window"),
            ("CUME_DIST", "window"),
            ("PERCENT_RANK", "window"),
            // ── System / Info ──
            ("VERSION", "system"),
            ("USER", "system"),
            ("CURRENT_USER", "system"),
            ("SESSION_USER", "system"),
            ("SYSTEM_USER", "system"),
            ("CURRENT_SCHEMA", "system"),
            ("CURRENT_DATABASE", "system"),
            ("PG_SLEEP", "system"),
            ("GEN_RANDOM_UUID", "system"),
        ]
    }

    // ── Built-in data types ──

    fn build_data_types() -> Vec<&'static str> {
        vec![
            // Integer types
            "INT",
            "INTEGER",
            "BIGINT",
            "SMALLINT",
            "TINYINT",
            "INT2",
            "INT4",
            "INT8",
            "UNSIGNED INT",
            "UNSIGNED BIGINT",
            "SERIAL",
            "BIGSERIAL",
            "SMALLSERIAL",
            // Fixed / Float
            "DECIMAL",
            "NUMERIC",
            "DEC",
            "FLOAT",
            "FLOAT4",
            "FLOAT8",
            "REAL",
            "DOUBLE",
            "DOUBLE PRECISION",
            // Money
            "MONEY",
            "SMALLMONEY",
            // String
            "CHAR",
            "VARCHAR",
            "CHARACTER VARYING",
            "NCHAR",
            "NVARCHAR",
            "TEXT",
            "TINYTEXT",
            "MEDIUMTEXT",
            "LONGTEXT",
            "CLOB",
            "NCLOB",
            "BPCHAR",
            "NAME",
            "CITEXT",
            // Binary
            "BINARY",
            "VARBINARY",
            "BLOB",
            "TINYBLOB",
            "MEDIUMBLOB",
            "LONGBLOB",
            "BYTEA",
            "RAW",
            // Date / Time
            "DATE",
            "TIME",
            "TIME WITH TIME ZONE",
            "TIMETZ",
            "TIMESTAMP",
            "TIMESTAMP WITH TIME ZONE",
            "TIMESTAMPTZ",
            "DATETIME",
            "DATETIME2",
            "SMALLDATETIME",
            "INTERVAL",
            "YEAR",
            // Boolean
            "BOOLEAN",
            "BOOL",
            "BIT",
            // JSON
            "JSON",
            "JSONB",
            // Array / Struct (PostgreSQL)
            "ARRAY",
            "UUID",
            "XML",
            "ENUM",
            "INET",
            "CIDR",
            "MACADDR",
            "TSVECTOR",
            "TSQUERY",
            "HSTORE",
            "POINT",
            "LINE",
            "LSEG",
            "BOX",
            "PATH",
            "POLYGON",
            "CIRCLE",
            "GEOMETRY",
            "GEOGRAPHY",
        ]
    }

    // ── Runtime metadata (enriched from connected databases) ──

    pub fn set_tables(&mut self, tables: &[String]) {
        self.table_names = tables.to_vec();
        self.table_columns.retain(|(t, _)| tables.contains(t));
    }

    pub fn set_columns_for_table(&mut self, table: &str, columns: &[String]) {
        if let Some(pos) = self.table_columns.iter().position(|(t, _)| t == table) {
            self.table_columns[pos].1 = columns.to_vec();
        } else {
            self.table_columns
                .push((table.to_string(), columns.to_vec()));
        }
        if !self.table_names.contains(&table.to_string()) {
            self.table_names.push(table.to_string());
        }
    }

    pub fn get_all_column_names(&self) -> Vec<String> {
        let mut cols = Vec::new();
        for (_, columns) in &self.table_columns {
            for col in columns {
                if !cols.contains(col) {
                    cols.push(col.clone());
                }
            }
        }
        cols
    }

    #[expect(dead_code)]
    pub fn get_columns_for_table(&self, table: &str) -> Vec<String> {
        self.table_columns
            .iter()
            .find(|(t, _)| t == table)
            .map(|(_, cols)| cols.clone())
            .unwrap_or_default()
    }

    // ── Main completion entry point ──

    pub fn suggest(&self, text: &str) -> Vec<CompletionItem> {
        let current_word = extract_current_word(text);
        if current_word.is_empty() {
            return Vec::new();
        }

        let upper = current_word.to_uppercase();
        let context = detect_context(text, &current_word);
        let mut results: Vec<CompletionItem> = Vec::new();
        let mut seen = HashSet::new();

        // 1. Context-aware keyword suggestions (with snippet support)
        for &(kw, category, snippet) in &self.keywords {
            if !kw.starts_with(&upper) || kw.eq_ignore_ascii_case(&current_word) {
                continue;
            }
            // Filter by context
            if !context_matches(&context, category) {
                continue;
            }

            let insert = snippet
                .map(|s| format!("{}{}", kw, s))
                .unwrap_or_else(|| kw.to_string());
            if seen.insert(kw.to_string()) {
                results.push(CompletionItem {
                    label: if snippet.is_some() {
                        format!("{}  ↳ template", kw)
                    } else {
                        kw.to_string()
                    },
                    detail: category.to_string(),
                    insert_text: insert,
                });
            }
        }

        // 2. Built-in SQL functions
        for &(name, cat) in &self.functions {
            if !name.starts_with(&upper) || name.eq_ignore_ascii_case(&current_word) {
                continue;
            }
            // In SELECT / WHERE / ORDER BY context, prefer functions
            if context == SqlContext::FromClause || context == SqlContext::TableDef {
                continue;
            }
            let insert = format!("{}()", name);
            if seen.insert(name.to_string()) {
                results.push(CompletionItem {
                    label: format!("{}()", name),
                    detail: format!("function  [{}]", cat),
                    insert_text: insert,
                });
            }
        }

        // 3. Built-in SQL data types (for DDL context)
        if context == SqlContext::TableDef {
            for dt in &self.data_types {
                if !dt.starts_with(&upper) || dt.eq_ignore_ascii_case(&current_word) {
                    continue;
                }
                if seen.insert(dt.to_string()) {
                    results.push(CompletionItem {
                        label: dt.to_string(),
                        detail: "type".into(),
                        insert_text: dt.to_string(),
                    });
                }
            }
        }

        // 4. Table names (enriched at runtime)
        if context == SqlContext::FromClause
            || context == SqlContext::JoinClause
            || context == SqlContext::Global
        {
            for t in &self.table_names {
                if !t.to_uppercase().starts_with(&upper) || t.eq_ignore_ascii_case(&current_word) {
                    continue;
                }
                if seen.insert(t.clone()) {
                    results.push(CompletionItem {
                        label: t.clone(),
                        detail: "table".into(),
                        insert_text: t.clone(),
                    });
                }
            }
        }

        // 5. Column names (enriched at runtime)
        if context == SqlContext::SelectExpr
            || context == SqlContext::WhereClause
            || context == SqlContext::OrderBy
            || context == SqlContext::GroupBy
            || context == SqlContext::Global
        {
            let all_cols = self.get_all_column_names();
            for c in &all_cols {
                if !c.to_uppercase().starts_with(&upper) || c.eq_ignore_ascii_case(&current_word) {
                    continue;
                }
                if seen.insert(c.clone()) {
                    results.push(CompletionItem {
                        label: c.clone(),
                        detail: "column".into(),
                        insert_text: c.clone(),
                    });
                }
            }
        }

        // Sort by rank then alphabetically
        results.sort_by(|a, b| {
            let a_rank = detail_rank(&a.detail);
            let b_rank = detail_rank(&b.detail);
            a_rank.cmp(&b_rank).then_with(|| a.label.cmp(&b.label))
        });

        results.truncate(20);
        results
    }
}

// ── Context detection ──

/// Detect SQL context from the text before the current word.
fn detect_context(text: &str, current_word: &str) -> SqlContext {
    let before = text[..text.len().saturating_sub(current_word.len())].trim();
    let before_upper = before.to_uppercase();

    // Peek at last few keywords
    let tokens: Vec<&str> = before_upper
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .filter(|t| !t.is_empty())
        .collect();

    let last = tokens.last().copied().unwrap_or("");
    let prev = tokens.iter().rev().nth(1).copied().unwrap_or("");

    match (prev, last) {
        // CREATE TABLE / ALTER TABLE → table definition context
        (_, "TABLE") if prev == "CREATE" || prev == "ALTER" || prev == "DROP" => {
            SqlContext::TableDef
        }
        (_, "COLUMN") | (_, "ADD") | (_, "DROP") | (_, "MODIFY") => SqlContext::TableDef,

        // FROM context
        (_, "FROM") | (_, "INTO") | (_, "UPDATE") => SqlContext::FromClause,

        // JOIN / ON context
        ("LEFT", "JOIN")
        | ("RIGHT", "JOIN")
        | ("INNER", "JOIN")
        | ("FULL", "JOIN")
        | ("CROSS", "JOIN")
        | ("OUTER", "JOIN")
        | ("NATURAL", "JOIN") => SqlContext::FromClause,
        (_, "JOIN") | (_, "ON") => SqlContext::JoinClause,

        // WHERE / AND / OR context
        (_, "WHERE") | (_, "AND") | (_, "OR") => SqlContext::WhereClause,

        // SELECT context
        (_, "SELECT") | (_, "DISTINCT") => SqlContext::SelectExpr,

        // ORDER BY context
        (_, "ORDER") => SqlContext::OrderBy, // next token would be BY
        ("ORDER", "BY") => SqlContext::OrderBy,
        (_, "BY") if prev == "ORDER" => SqlContext::OrderBy,

        // GROUP BY context
        (_, "GROUP") => SqlContext::GroupBy,
        ("GROUP", "BY") => SqlContext::GroupBy,

        // Comma → same context as parent
        (_, ",") => {
            // Look further back
            let third = tokens.iter().rev().nth(2).copied().unwrap_or("");
            if third == "SELECT" || last == "SELECT" {
                SqlContext::SelectExpr
            } else if third == "FROM" || last == "FROM" || third == "JOIN" {
                SqlContext::FromClause
            } else {
                SqlContext::Global
            }
        }

        // Check partial keyword prefixes in current_word or scan tokens for clause context
        _ => {
            let ctx = partial_to_context(current_word, &before_upper);
            if ctx != SqlContext::Global {
                ctx
            } else {
                // Scan tokens backwards for the most recent clause keyword
                clause_context_from_tokens(&tokens, &before_upper)
            }
        }
    }
}

/// Map a partial word to a likely SQL context based on keyword prefixes.
fn partial_to_context(cw: &str, before: &str) -> SqlContext {
    let cw = cw.trim();
    if cw.is_empty() {
        return SqlContext::Global;
    }
    let cw_upper = cw.to_uppercase();
    if ("WHERE".starts_with(&cw_upper) || "HAVING".starts_with(&cw_upper))
        && before.contains("FROM")
    {
        return SqlContext::WhereClause;
    }
    if "ORDER".starts_with(&cw_upper) && before.contains("FROM") {
        return SqlContext::OrderBy;
    }
    if "GROUP".starts_with(&cw_upper) && before.contains("FROM") {
        return SqlContext::GroupBy;
    }
    if "LIMIT".starts_with(&cw_upper) && before.contains("FROM") {
        return SqlContext::WhereClause;
    }
    if "JOIN".starts_with(&cw_upper) && before.contains("FROM") {
        return SqlContext::FromClause;
    }
    SqlContext::Global
}

/// Scan tokens backwards for the most recent clause-introducing keyword.
fn clause_context_from_tokens(tokens: &[&str], before: &str) -> SqlContext {
    for tok in tokens.iter().rev() {
        match *tok {
            "WHERE" | "AND" | "OR" => return SqlContext::WhereClause,
            "ORDER" => return SqlContext::OrderBy,
            "GROUP" => return SqlContext::GroupBy,
            "JOIN" | "ON" => return SqlContext::JoinClause,
            "FROM" | "INTO" | "UPDATE" => return SqlContext::FromClause,
            "SELECT" | "DISTINCT" => return SqlContext::SelectExpr,
            _ => continue,
        }
    }
    // Check for DDL table context
    if before.contains("CREATE TABLE")
        || before.contains("ALTER TABLE")
        || before.contains("DROP TABLE")
    {
        return SqlContext::TableDef;
    }
    if before.contains("SELECT") && !before.contains("INSERT") {
        SqlContext::SelectExpr
    } else {
        SqlContext::Global
    }
}

/// Filter keywords by context.
fn context_matches(ctx: &SqlContext, category: &str) -> bool {
    match ctx {
        SqlContext::Global => true,
        SqlContext::SelectExpr => matches!(
            category,
            "DML" | "mod" | "clause" | "expr" | "func" | "op" | "set"
        ),
        SqlContext::FromClause => matches!(category, "clause" | "DML" | "set" | "cte"),
        SqlContext::WhereClause => matches!(
            category,
            "op" | "expr" | "func" | "cond" | "literal" | "clause"
        ),
        SqlContext::JoinClause => matches!(category, "clause" | "op" | "cond" | "alias"),
        SqlContext::OrderBy | SqlContext::GroupBy => {
            matches!(category, "order" | "expr" | "func" | "clause")
        }
        SqlContext::TableDef => matches!(
            category,
            "DDL" | "type" | "constraint" | "col_opt" | "ref_action" | "mod"
        ),
        SqlContext::InsertCols | SqlContext::ValuesClause => matches!(category, "DML" | "clause"),
        SqlContext::OnConflict => matches!(category, "clause" | "DDL"),
    }
}

fn detail_rank(detail: &str) -> u8 {
    if detail.starts_with("function") {
        return 1;
    }
    match detail {
        "DML" | "DDL" | "TCL" | "DCL" => 0,
        "clause" | "mod" | "op" | "alias" | "order" | "cond" | "literal" | "set" | "expr"
        | "cte" | "util" => 0,
        "constraint" | "col_opt" | "ref_action" | "type" => 2,
        "table" => 3,
        "column" => 4,
        _ => 5,
    }
}

// ── Word extraction & replacement ──

fn extract_current_word(text: &str) -> String {
    let text = text.trim_end();
    if text.is_empty() {
        return String::new();
    }

    let mut end = text.len();
    // Skip trailing whitespace
    for c in text.chars().rev() {
        if !c.is_whitespace() {
            break;
        }
        end -= c.len_utf8();
    }

    let mut start = 0;
    for (i, c) in text.char_indices() {
        if i >= end {
            break;
        }
        if c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == ';' {
            start = i + c.len_utf8();
        }
    }

    if start < end {
        text[start..end].to_string()
    } else {
        String::new()
    }
}

pub fn replace_current_word(text: &str, replacement: &str) -> String {
    let text = text.trim_end();
    if text.is_empty() {
        return replacement.to_string();
    }

    let mut end = text.len();
    for c in text.chars().rev() {
        if !c.is_whitespace() {
            break;
        }
        end -= c.len_utf8();
    }

    let mut start = 0;
    for (i, c) in text.char_indices() {
        if i >= end {
            break;
        }
        if c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == ';' {
            start = i + c.len_utf8();
        }
    }

    if start > 0 {
        format!("{}{}", &text[..start], replacement)
    } else {
        replacement.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_context() {
        assert_eq!(detect_context("SELECT ", ""), SqlContext::SelectExpr);
        assert_eq!(
            detect_context("SELECT name FR", "FR"),
            SqlContext::SelectExpr
        );
        assert_eq!(detect_context("SELECT * FROM ", ""), SqlContext::FromClause);
        assert_eq!(
            detect_context("SELECT * FROM emp WH", "WH"),
            SqlContext::WhereClause
        );
        assert_eq!(
            detect_context("SELECT * FROM emp WHERE age ", ""),
            SqlContext::WhereClause
        );
        assert_eq!(
            detect_context("SELECT * FROM emp ORDER ", ""),
            SqlContext::OrderBy
        );
        assert_eq!(
            detect_context("CREATE TABLE users (", ""),
            SqlContext::TableDef
        );
    }

    #[test]
    fn test_keyword_suggestions() {
        let c = SqlCompleter::new();
        let r = c.suggest("SEL");
        assert!(r.iter().any(|i| i.label.starts_with("SELECT")));
        let r = c.suggest("WH");
        assert!(r.iter().any(|i| i.label.starts_with("WHERE")));
        let r = c.suggest("CR");
        assert!(r.iter().any(|i| i.label.starts_with("CREATE")));
    }

    #[test]
    fn test_function_suggestions() {
        let c = SqlCompleter::new();
        let r = c.suggest("COUN");
        assert!(r.iter().any(|i| i.label == "COUNT()"));
        let r = c.suggest("COUN");
        assert!(r.iter().any(|i| i.detail.contains("aggregate")));
    }

    #[test]
    fn test_data_type_suggestions() {
        let c = SqlCompleter::new();
        // Types suggested only in DDL context — in global context they'd still show as keywords
        let r = c.suggest("VARCH");
        // VARCHAR should show up as a keyword (it's in the keyword list as DDL type)
        // Actually it's not in keywords, but in data_types
        assert!(r.is_empty()); // because global context doesn't add types
    }
}
