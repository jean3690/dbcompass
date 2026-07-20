use plugin_interface::QueryResult;

/// Convert a plugin QueryResult into Slint-compatible string arrays
pub struct QueryResultView {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Parallel to `rows`: tracks which cells are NULL for styled rendering
    pub null_cells: Vec<Vec<bool>>,
    pub rows_affected: u64,
    pub execution_time_ms: f64,
    pub has_more: bool,
    pub error: Option<String>,
}

impl QueryResultView {
    pub fn from_query_result(result: QueryResult) -> Self {
        let columns: Vec<String> = result.columns.iter().map(|c| c.name.clone()).collect();
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut null_cells: Vec<Vec<bool>> = Vec::new();

        for row in &result.rows {
            let mut cell_row = Vec::new();
            let mut null_row = Vec::new();
            for cell in row {
                if cell.is_null {
                    cell_row.push("⟨NULL⟩".into());
                    null_row.push(true);
                } else {
                    cell_row.push(cell.display.clone().unwrap_or_default());
                    null_row.push(false);
                }
            }
            rows.push(cell_row);
            null_cells.push(null_row);
        }

        Self {
            execution_time_ms: result.execution_time_ns as f64 / 1_000_000.0,
            rows_affected: result.rows_affected,
            has_more: result.has_more,
            columns,
            rows,
            null_cells,
            error: None,
        }
    }

    pub fn from_error(err: String) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            null_cells: Vec::new(),
            rows_affected: 0,
            execution_time_ms: 0.0,
            has_more: false,
            error: Some(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_interface::{CellValue, ColumnInfo, ColumnType};

    #[test]
    fn test_from_query_result_converts_columns() {
        let result = plugin_interface::QueryResult {
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    data_type: ColumnType::Int32,
                    nullable: false,
                    primary_key: true,
                    default_value: None,
                },
                ColumnInfo {
                    name: "name".into(),
                    data_type: ColumnType::String,
                    nullable: true,
                    primary_key: false,
                    default_value: Some("guest".into()),
                },
            ],
            rows: vec![],
            rows_affected: 0,
            execution_time_ns: 1_500_000,
            has_more: false,
        };

        let view = QueryResultView::from_query_result(result);
        assert_eq!(view.columns, vec!["id", "name"]);
        assert!(view.rows.is_empty());
        assert!(!view.has_more);
        assert!(view.error.is_none());
        assert!((view.execution_time_ms - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_from_query_result_converts_null_cells() {
        let result = plugin_interface::QueryResult {
            columns: vec![ColumnInfo {
                name: "val".into(),
                data_type: ColumnType::String,
                nullable: true,
                primary_key: false,
                default_value: None,
            }],
            rows: vec![
                vec![CellValue {
                    display: Some("hello".into()),
                    raw_type: ColumnType::String,
                    is_null: false,
                }],
                vec![CellValue {
                    display: None,
                    raw_type: ColumnType::String,
                    is_null: true,
                }],
            ],
            rows_affected: 0,
            execution_time_ns: 0,
            has_more: false,
        };

        let view = QueryResultView::from_query_result(result);
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[0][0], "hello");
        assert_eq!(view.rows[1][0], "⟨NULL⟩");
        assert!(!view.null_cells[0][0]);
        assert!(view.null_cells[1][0]);
    }

    #[test]
    fn test_from_query_result_empty() {
        let result = plugin_interface::QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
            execution_time_ns: 0,
            has_more: false,
        };
        let view = QueryResultView::from_query_result(result);
        assert!(view.columns.is_empty());
        assert!(view.rows.is_empty());
        assert!(view.error.is_none());
    }

    #[test]
    fn test_from_error_sets_error_field() {
        let view = QueryResultView::from_error("oops".into());
        assert_eq!(view.error, Some("oops".into()));
        assert!(view.columns.is_empty());
        assert!(view.rows.is_empty());
        assert!(!view.has_more);
    }

    #[test]
    fn test_from_query_result_has_more_flag() {
        let mut result = make_dummy_result();
        result.has_more = true;
        let view = QueryResultView::from_query_result(result);
        assert!(view.has_more);
    }

    fn make_dummy_result() -> plugin_interface::QueryResult {
        plugin_interface::QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
            execution_time_ns: 0,
            has_more: false,
        }
    }
}
