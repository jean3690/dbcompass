use std::sync::Arc;

use crate::app::AppState;
use crate::models::query_result::QueryResultView;

/// Execute a SQL query on a background thread and deliver the result to a callback.
///
/// The callback is invoked on the main thread via `slint::invoke_from_event_loop`.
pub fn execute_query(
    app: &Arc<AppState>,
    connector_name: &str,
    conn_id: u64,
    sql: String,
    max_rows: Option<usize>,
    on_result: Box<dyn FnOnce(QueryResultView) + Send + 'static>,
) {
    let app = app.clone();
    let connector_name = connector_name.to_string();

    std::thread::spawn(move || {
        let pm = app.plugin_manager.lock().unwrap_or_else(|e| {
            eprintln!("[query] Recovered from poisoned mutex");
            e.into_inner()
        });
        let connector = match pm.get_connector(&connector_name) {
            Some(c) => c,
            None => {
                if let Err(e) = slint::invoke_from_event_loop(move || {
                    on_result(QueryResultView::from_error(format!(
                        "Connector '{}' not found",
                        connector_name
                    )));
                }) {
                    eprintln!("[query] Failed to deliver error to UI: {e}");
                }
                return;
            }
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            connector.execute_query(conn_id, &sql, max_rows)
        }));

        let view = match result {
            Ok(Ok(query_result)) => QueryResultView::from_query_result(query_result),
            Ok(Err(db_err)) => QueryResultView::from_error(db_err.to_string()),
            Err(panic_info) => {
                let msg = panic_info
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic_info.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "Unknown plugin panic".into());
                QueryResultView::from_error(format!("Plugin crashed: {}", msg))
            }
        };

        if let Err(e) = slint::invoke_from_event_loop(move || {
            on_result(view);
        }) {
            eprintln!("[query] Failed to deliver result to UI: {e}");
        }
    });
}
