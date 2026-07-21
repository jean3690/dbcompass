mod app;
mod bridge;
mod connection_manager;
mod models;
mod plugin_manager;
mod query_executor;
mod sql_completion;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app::AppState;
use bridge::TreeCache;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| {
            eprintln!(
                "[main] Warning: could not determine executable directory, using current directory"
            );
            PathBuf::new()
        });

    let data_dir = directories::ProjectDirs::from("com", "dbcompass", "DBCompass")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| exe_dir.join("data"));

    let mut plugin_manager = plugin_manager::PluginManager::new(exe_dir.clone());
    plugin_manager.load_all();

    let connection_manager = connection_manager::ConnectionManager::new(data_dir);
    let app = AppState::new(plugin_manager, connection_manager);

    let tree_cache: Arc<Mutex<TreeCache>> = Arc::new(Mutex::new(TreeCache::new()));

    let window = MainWindow::new()?;

    bridge::populate_connectors(&app, &window);
    bridge::refresh_tree(&app, &window);

    // Auto-reconnect to connections from last session
    bridge::auto_reconnect_last_session(&app, &window);

    // ── Tree node clicked (select connection / auto-fill table) ──

    let app_clone = app.clone();
    let cache_clone = tree_cache.clone();
    let window_handle = window.as_weak();
    window.on_tree_node_clicked(move |id: slint::SharedString| {
        if let Some(w) = window_handle.upgrade() {
            let id_str = id.to_string();
            // If it contains "__", it's a table node
            if id_str.contains("__") {
                let cache = cache_clone.lock().unwrap_or_else(|e| {
                    eprintln!("[main] Recovered from poisoned mutex");
                    e.into_inner()
                });
                bridge::select_table(&app_clone, &cache, &w, &id_str);
            } else {
                w.set_status_left(slint::SharedString::from(format!("Selected: {}", id_str)));
            }
        }
    });

    // ── Tree node expand/collapse ──

    let app_clone = app.clone();
    let cache_clone = tree_cache.clone();
    let window_handle = window.as_weak();
    window.on_tree_node_expand_toggled(move |id: slint::SharedString| {
        if let Some(w) = window_handle.upgrade() {
            let id_str = id.to_string();
            let mut cache = cache_clone.lock().unwrap_or_else(|e| {
                eprintln!("[main] Recovered from poisoned mutex");
                e.into_inner()
            });

            if id_str.contains("__") {
                let parts: Vec<&str> = id_str.split("__").collect();
                if parts.len() >= 3 {
                    // conn__db__table → expand/collapse columns
                    if cache.expanded_tables.contains_key(&id_str) {
                        bridge::collapse_table(&app_clone, &mut cache, &w, &id_str);
                    } else {
                        bridge::expand_table(&app_clone, &mut cache, &w, &id_str);
                    }
                } else if parts.len() == 2 {
                    // conn__db → expand/collapse tables for this database
                    if cache.expanded_db_tables.contains_key(&id_str) {
                        bridge::collapse_database_tables(&app_clone, &mut cache, &w, &id_str);
                    } else {
                        bridge::expand_database_tables(&app_clone, &mut cache, &w, &id_str);
                    }
                } else {
                    // Fallback: already-expanded table node
                    if cache.expanded_tables.contains_key(&id_str) {
                        bridge::collapse_table(&app_clone, &mut cache, &w, &id_str);
                    }
                }
            } else if cache.expanded_databases.contains_key(&id_str) {
                // Connection expanded to show databases → collapse
                bridge::collapse_connection(&app_clone, &mut cache, &w, &id_str);
            } else if cache.expanded_connections.contains_key(&id_str) {
                // Connection expanded to show tables directly → collapse
                bridge::collapse_connection(&app_clone, &mut cache, &w, &id_str);
            } else {
                // Not expanded yet → expand
                bridge::expand_connection(&app_clone, &mut cache, &w, &id_str);
            }
        }
    });

    // ── Connect accepted ──

    let app_clone = app.clone();
    let window_handle = window.as_weak();
    window.on_connect_accepted(move || {
        if let Some(w) = window_handle.upgrade() {
            let name = w.get_form_conn_name().to_string();
            let ctype = w.get_form_connector_type().to_string();
            let host = w.get_form_host().to_string();
            let port_text = w.get_form_port_text().to_string();
            let port_val = port_text.trim().to_string();
            let port: u16 = if port_val.is_empty() || port_val == "0" {
                // SQLite and other file-based databases don't need a port
                0
            } else {
                match port_val.parse() {
                    Ok(p @ 1..) => p,
                    _ => {
                        w.set_status_left(slint::SharedString::from(format!(
                            "Invalid port: '{}'. Must be a number between 1 and 65535.",
                            port_text
                        )));
                        return;
                    }
                }
            };
            let database = w.get_form_database().to_string();
            let username = w.get_form_username().to_string();
            let password = w.get_form_password().to_string();

            bridge::connect_to_database(
                &app_clone, &w, name, ctype, host, port, database, username, password,
            );

            w.set_connection_form_open(false);
        }
    });

    // ── Connect dismissed ──

    let window_handle = window.as_weak();
    window.on_connect_dismissed(move || {
        if let Some(w) = window_handle.upgrade() {
            w.set_connection_form_open(false);
            // Clear form fields
            w.set_form_conn_name(slint::SharedString::from(""));
            w.set_form_connector_type(slint::SharedString::from(""));
            w.set_form_host(slint::SharedString::from(""));
            w.set_form_port_text(slint::SharedString::from("0"));
            w.set_form_database(slint::SharedString::from(""));
            w.set_form_username(slint::SharedString::from(""));
            w.set_form_password(slint::SharedString::from(""));
        }
    });

    // ── Browse file (SQLite) ──

    let window_handle = window.as_weak();
    window.on_browse_file(move || {
        if let Some(w) = window_handle.upgrade() {
            let file = rfd::FileDialog::new()
                .add_filter("SQLite Database", &["db", "sqlite", "sqlite3"])
                .set_file_name("database.db")
                .pick_file();
            if let Some(path) = file {
                w.set_form_database(slint::SharedString::from(
                    path.to_string_lossy().to_string(),
                ));
            }
        }
    });

    // ── SQL text changed (for completion) ──

    let app_clone = app.clone();
    let window_handle = window.as_weak();
    window.on_text_changed(move |text: slint::SharedString| {
        if let Some(w) = window_handle.upgrade() {
            let text = text.to_string();

            // Hide history when user is typing
            w.set_history_visible(false);

            let suggestions = {
                let completer = app_clone.sql_completer.lock().unwrap_or_else(|e| {
                    eprintln!("[main] Recovered from poisoned mutex");
                    e.into_inner()
                });
                completer.suggest(&text)
            };

            if suggestions.is_empty() {
                w.set_completion_visible(false);
            } else {
                use std::rc::Rc;

                // Build CompletionItem model
                let mut items = Vec::new();
                for s in &suggestions {
                    items.push(crate::CompletionItem {
                        label: slint::SharedString::from(&s.label),
                        detail: slint::SharedString::from(&s.detail),
                        insert_text: slint::SharedString::from(&s.insert_text),
                    });
                }
                let model = slint::VecModel::from(items);
                w.set_completion_items(slint::ModelRc::from(Rc::new(model)));
                w.set_completion_visible(true);
            }
        }
    });

    // ── Completion selected ──

    let window_handle = window.as_weak();
    window.on_completion_selected(move |insert_text: slint::SharedString| {
        if let Some(w) = window_handle.upgrade() {
            let current = w.get_sql_text().to_string();
            let new_text = crate::sql_completion::replace_current_word(&current, &insert_text);
            w.set_sql_text(slint::SharedString::from(new_text));
            w.set_completion_visible(false);
        }
    });

    // ── Execute query ──

    let app_clone = app.clone();
    let window_handle = window.as_weak();
    window.on_execute_query(move || {
        if let Some(w) = window_handle.upgrade() {
            w.set_completion_visible(false);
            let sql = w.get_sql_text().to_string().trim().to_string();
            if sql.is_empty() {
                return;
            }
            w.set_sql_text(slint::SharedString::from(&sql));
            app_clone.add_query_history(&sql);
            bridge::update_query_history(&app_clone, &w);

            // Use active (selected) connection if available; otherwise fall back to first
            let active_id = w.get_active_node().to_string();
            if !active_id.is_empty() && !active_id.contains("__") {
                // active_id is a connection ID (no "__")
                let ac = app_clone.active_connections.lock().unwrap_or_else(|e| {
                    eprintln!("[main] Recovered from poisoned mutex");
                    e.into_inner()
                });
                if let Some(conn) = ac.get(&active_id) {
                    let connector_name = conn.connector_name.clone();
                    let conn_id = conn.conn_id;
                    drop(ac);
                    bridge::execute_on_connection(&app_clone, &w, &connector_name, conn_id, sql);
                    return;
                }
            }
            bridge::execute_on_first_connection(&app_clone, &w, sql);
        }
    });

    // ── Query history selected ──

    let window_handle = window.as_weak();
    window.on_history_selected(move |text: slint::SharedString| {
        if let Some(w) = window_handle.upgrade() {
            w.set_sql_text(text);
        }
    });

    // ── Max rows changed ──

    let app_clone = app.clone();
    window.on_max_rows_changed(move |val: i32| {
        let rows = val.max(1) as usize;
        if let Ok(mut max_rows) = app_clone.max_rows.lock() {
            *max_rows = rows;
        }
    });

    // ── Tree filter changed ──

    let app_clone = app.clone();
    let window_handle = window.as_weak();
    window.on_filter_changed(move |filter_text: slint::SharedString| {
        if let Ok(mut tf) = app_clone.tree_filter.lock() {
            *tf = filter_text.to_string();
        }
        if let Some(w) = window_handle.upgrade() {
            bridge::refresh_tree(&app_clone, &w);
        }
    });

    // ── Transaction: Begin ──

    let app_clone = app.clone();
    let window_handle = window.as_weak();
    window.on_begin_transaction(move || {
        if let Some(w) = window_handle.upgrade() {
            let active_id = w.get_active_node().to_string();
            if !active_id.is_empty() && !active_id.contains("__") {
                let ac = app_clone.active_connections.lock().unwrap_or_else(|e| {
                    eprintln!("[main] Recovered from poisoned mutex");
                    e.into_inner()
                });
                if let Some(conn) = ac.get(&active_id) {
                    let connector_name = conn.connector_name.clone();
                    let conn_id = conn.conn_id;
                    drop(ac);
                    bridge::execute_transaction_sql(
                        &app_clone,
                        &w,
                        &connector_name,
                        conn_id,
                        "BEGIN",
                        true,
                    );
                }
            }
        }
    });

    // ── Transaction: Commit ──

    let app_clone = app.clone();
    let window_handle = window.as_weak();
    window.on_commit_transaction(move || {
        if let Some(w) = window_handle.upgrade() {
            let active_id = w.get_active_node().to_string();
            if !active_id.is_empty() && !active_id.contains("__") {
                let ac = app_clone.active_connections.lock().unwrap_or_else(|e| {
                    eprintln!("[main] Recovered from poisoned mutex");
                    e.into_inner()
                });
                if let Some(conn) = ac.get(&active_id) {
                    let connector_name = conn.connector_name.clone();
                    let conn_id = conn.conn_id;
                    drop(ac);
                    bridge::execute_transaction_sql(
                        &app_clone,
                        &w,
                        &connector_name,
                        conn_id,
                        "COMMIT",
                        false,
                    );
                }
            }
        }
    });

    // ── Transaction: Rollback ──

    let app_clone = app.clone();
    let window_handle = window.as_weak();
    window.on_rollback_transaction(move || {
        if let Some(w) = window_handle.upgrade() {
            let active_id = w.get_active_node().to_string();
            if !active_id.is_empty() && !active_id.contains("__") {
                let ac = app_clone.active_connections.lock().unwrap_or_else(|e| {
                    eprintln!("[main] Recovered from poisoned mutex");
                    e.into_inner()
                });
                if let Some(conn) = ac.get(&active_id) {
                    let connector_name = conn.connector_name.clone();
                    let conn_id = conn.conn_id;
                    drop(ac);
                    bridge::execute_transaction_sql(
                        &app_clone,
                        &w,
                        &connector_name,
                        conn_id,
                        "ROLLBACK",
                        false,
                    );
                }
            }
        }
    });

    // Initial history load
    bridge::update_query_history(&app, &window);

    // ── Export CSV ──

    let window_handle = window.as_weak();
    window.on_export_csv(move || {
        if let Some(w) = window_handle.upgrade() {
            if !w.get_has_results() {
                return;
            }

            let file = rfd::FileDialog::new()
                .add_filter("CSV Files", &["csv"])
                .set_file_name("query_result.csv")
                .save_file();

            if let Some(path) = file {
                use slint::Model;

                let cols_model = w.get_result_columns();
                let rows_model = w.get_result_rows();

                let mut wtr = match csv::Writer::from_path(&path) {
                    Ok(w) => w,
                    Err(e) => {
                        w.set_status_left(slint::SharedString::from(format!(
                            "CSV export failed: {}",
                            e
                        )));
                        return;
                    }
                };

                // Write header
                let headers: Vec<String> = (0..cols_model.row_count())
                    .map(|i| cols_model.row_data(i).unwrap_or_default().to_string())
                    .collect();
                if let Err(e) = wtr.write_record(&headers) {
                    w.set_status_left(slint::SharedString::from(format!("CSV write error: {}", e)));
                    return;
                }

                // Write rows
                for i in 0..rows_model.row_count() {
                    if let Some(row) = rows_model.row_data(i) {
                        let cells: Vec<String> = (0..row.cells.row_count())
                            .map(|j| row.cells.row_data(j).unwrap_or_default().to_string())
                            .collect();
                        if let Err(e) = wtr.write_record(&cells) {
                            w.set_status_left(slint::SharedString::from(format!(
                                "CSV write error: {}",
                                e
                            )));
                            return;
                        }
                    }
                }

                if let Err(e) = wtr.flush() {
                    w.set_status_left(slint::SharedString::from(format!("CSV flush error: {}", e)));
                    return;
                }

                w.set_status_left(slint::SharedString::from(format!(
                    "Exported CSV: {}",
                    path.display()
                )));
            }
        }
    });

    // ── Export JSON ──

    let window_handle = window.as_weak();
    window.on_export_json(move || {
        if let Some(w) = window_handle.upgrade() {
            if !w.get_has_results() {
                return;
            }

            let file = rfd::FileDialog::new()
                .add_filter("JSON Files", &["json"])
                .set_file_name("query_result.json")
                .save_file();

            if let Some(path) = file {
                use slint::Model;

                let cols_model = w.get_result_columns();
                let rows_model = w.get_result_rows();

                // Build array of row objects
                let headers: Vec<String> = (0..cols_model.row_count())
                    .map(|i| cols_model.row_data(i).unwrap_or_default().to_string())
                    .collect();

                let mut records: Vec<serde_json::Value> = Vec::new();
                for i in 0..rows_model.row_count() {
                    if let Some(row) = rows_model.row_data(i) {
                        let mut map = serde_json::Map::new();
                        for (j, header) in headers.iter().enumerate() {
                            let val = row.cells.row_data(j).unwrap_or_default().to_string();
                            map.insert(header.clone(), serde_json::Value::String(val));
                        }
                        records.push(serde_json::Value::Object(map));
                    }
                }

                let json = serde_json::to_string_pretty(&serde_json::Value::Array(records));
                match json {
                    Ok(content) => {
                        if let Err(e) = std::fs::write(&path, &content) {
                            w.set_status_left(slint::SharedString::from(format!(
                                "JSON write error: {}",
                                e
                            )));
                            return;
                        }
                    }
                    Err(e) => {
                        w.set_status_left(slint::SharedString::from(format!(
                            "JSON serialization error: {}",
                            e
                        )));
                        return;
                    }
                }

                w.set_status_left(slint::SharedString::from(format!(
                    "Exported JSON: {}",
                    path.display()
                )));
            }
        }
    });

    // ── Edit row (build UPDATE SQL) ──

    let window_handle = window.as_weak();
    window.on_edit_row(move |row_idx: i32| {
        if let Some(w) = window_handle.upgrade() {
            if row_idx < 0 {
                return;
            }
            let sql = bridge::build_edit_row_sql(&w, row_idx as usize);
            w.set_sql_text(slint::SharedString::from(sql));
        }
    });

    // ── Delete row (build DELETE SQL) ──

    let window_handle = window.as_weak();
    window.on_delete_row(move |row_idx: i32| {
        if let Some(w) = window_handle.upgrade() {
            if row_idx < 0 {
                return;
            }
            let sql = bridge::build_delete_row_sql(&w, row_idx as usize);
            w.set_sql_text(slint::SharedString::from(sql));
        }
    });

    // ── Disconnect ──

    let app_clone = app.clone();
    let cache_clone = tree_cache.clone();
    let window_handle = window.as_weak();
    window.on_disconnect_connection(move || {
        if let Some(w) = window_handle.upgrade() {
            let id = w.get_active_node().to_string();
            if id.is_empty() || id.contains("__") {
                return;
            }
            bridge::disconnect_connection(
                &app_clone,
                &mut cache_clone.lock().unwrap_or_else(|e| {
                    eprintln!("[main] Recovered from poisoned mutex");
                    e.into_inner()
                }),
                &w,
                &id,
            );
        }
    });

    // ── Delete connection ──

    let app_clone = app.clone();
    let cache_clone = tree_cache.clone();
    let window_handle = window.as_weak();
    window.on_delete_connection(move || {
        if let Some(w) = window_handle.upgrade() {
            let id = w.get_active_node().to_string();
            if id.is_empty() || id.contains("__") {
                return;
            }

            let confirm = rfd::MessageDialog::new()
                .set_title("Delete Connection")
                .set_description(format!("Delete connection \"{}\"?", id))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if confirm != rfd::MessageDialogResult::Yes {
                return;
            }

            // Disconnect first
            bridge::disconnect_connection(
                &app_clone,
                &mut cache_clone.lock().unwrap_or_else(|e| {
                    eprintln!("[main] Recovered from poisoned mutex");
                    e.into_inner()
                }),
                &w,
                &id,
            );

            // Delete saved connection
            {
                let mut cm = app_clone.connection_manager.lock().unwrap_or_else(|e| {
                    eprintln!("[main] Recovered from poisoned mutex");
                    e.into_inner()
                });
                let _ = cm.delete(&id);
            }

            bridge::refresh_tree(&app_clone, &w);
            w.set_status_left(slint::SharedString::from(format!(
                "Deleted connection: {}",
                id
            )));
        }
    });

    // ── Edit connection ──

    let app_clone = app.clone();
    let window_handle = window.as_weak();
    window.on_edit_connection(move || {
        if let Some(w) = window_handle.upgrade() {
            let id = w.get_active_node().to_string();
            if id.is_empty() || id.contains("__") {
                return;
            }
            bridge::populate_edit_form(&app_clone, &w, &id);
            w.set_dialog_title(slint::SharedString::from("Edit Connection"));
        }
    });

    // ── Test connection ──

    let app_clone = app.clone();
    let window_handle = window.as_weak();
    window.on_test_connection(move || {
        if let Some(w) = window_handle.upgrade() {
            let ctype = w.get_form_connector_type().to_string();
            let host = w.get_form_host().to_string();
            let port = w.get_form_port_text().to_string();
            let database = w.get_form_database().to_string();
            let username = w.get_form_username().to_string();
            let password = w.get_form_password().to_string();

            w.set_status_left(slint::SharedString::from("Testing connection..."));
            bridge::test_connection(
                &app_clone, &w, ctype, host, port, database, username, password,
            );
        }
    });

    window.run()?;

    let mut pm = app.plugin_manager.lock().unwrap_or_else(|e| {
        eprintln!("[main] Recovered from poisoned mutex");
        e.into_inner()
    });
    pm.unload_all();

    Ok(())
}
