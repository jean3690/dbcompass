use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use plugin_interface::ConnectorConfig;
use slint::ComponentHandle;

use crate::app::{ActiveConnection, AppState};
use crate::connection_manager::SavedConnection;
use crate::models::query_result::QueryResultView;
use crate::query_executor;

// ── Connector types ──────────────────────────────────────────

pub fn populate_connectors(app: &Arc<AppState>, window: &crate::MainWindow) {
    let types = {
        let pm = app.plugin_manager.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        let connectors = pm.list_connectors();
        connectors
            .iter()
            .map(|(name, _)| slint::SharedString::from(*name))
            .collect::<Vec<_>>()
    };
    let model = slint::VecModel::from(types);
    window.set_connector_types(slint::ModelRc::from(Rc::new(model)));
}

// ── Connection tree ─────────────────────────────────────────

/// Track which connections and tables have been expanded.
pub struct TreeCache {
    /// conn_name → tables (used when a specific database is configured)
    pub expanded_connections: HashMap<String, Vec<TreeNodeEntry>>,
    /// conn_name → list of database names (when no database specified)
    pub expanded_databases: HashMap<String, Vec<String>>,
    /// conn_name__db → tables under that database
    pub expanded_db_tables: HashMap<String, Vec<TreeNodeEntry>>,
    /// node_id → columns (node_id format: conn__table or conn__db__table)
    pub expanded_tables: HashMap<String, Vec<ColumnEntry>>,
}

pub struct TreeNodeEntry {
    pub table_name: String,
    pub node_id: String,
}

pub struct ColumnEntry {
    pub column_name: String,
    #[expect(dead_code)]
    pub column_type: String,
    pub node_id: String,
}

impl TreeCache {
    pub fn new() -> Self {
        Self {
            expanded_connections: HashMap::new(),
            expanded_databases: HashMap::new(),
            expanded_db_tables: HashMap::new(),
            expanded_tables: HashMap::new(),
        }
    }

}

const CONNECTION_COLORS: &[&str] = &[
    "#6366f1", "#ec4899", "#14b8a6", "#f97316", "#8b5cf6", "#06b6d4", "#84cc16", "#e11d48",
    "#0ea5e9", "#a855f7",
];

fn pick_color(index: usize) -> String {
    CONNECTION_COLORS[index % CONNECTION_COLORS.len()].to_string()
}

fn hex_to_rgb(hex: &str) -> (i32, i32, i32) {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6
        && let (Ok(r), Ok(g), Ok(b)) = (
            i32::from_str_radix(&hex[0..2], 16),
            i32::from_str_radix(&hex[2..4], 16),
            i32::from_str_radix(&hex[4..6], 16),
        )
    {
        return (r, g, b);
    }
    (0, 0, 0)
}

/// Build flat tree nodes: saved connections only (no expanded children).
pub fn build_connection_tree(
    cm: &crate::connection_manager::ConnectionManager,
) -> Vec<crate::FlatTreeNode> {
    let mut nodes = Vec::new();
    for conn in cm.list() {
        let (r, g, b) = hex_to_rgb(&conn.color);
        nodes.push(crate::FlatTreeNode {
            id: slint::SharedString::from(&conn.id),
            label: slint::SharedString::from(&conn.name),
            icon: slint::SharedString::from(""),
            depth: 0,
            is_expandable: true,
            is_expanded: false,
            conn_id: -1,
            node_type: slint::SharedString::from("connection"),
            color_r: r,
            color_g: g,
            color_b: b,
        });
    }
    nodes
}

/// Rebuild flat nodes including any expanded children.
pub fn build_tree_with_children(
    cm: &crate::connection_manager::ConnectionManager,
    cache: &TreeCache,
) -> Vec<crate::FlatTreeNode> {
    let mut nodes = Vec::new();
    for conn in cm.list() {
        let has_databases = cache.expanded_databases.contains_key(&conn.id);
        let has_tables = cache.expanded_connections.contains_key(&conn.id);
        let is_expanded = has_databases || has_tables;
        let (r, g, b) = hex_to_rgb(&conn.color);
        nodes.push(crate::FlatTreeNode {
            id: slint::SharedString::from(&conn.id),
            label: slint::SharedString::from(&conn.name),
            icon: slint::SharedString::from(""),
            depth: 0,
            is_expandable: true,
            is_expanded,
            conn_id: -1,
            node_type: slint::SharedString::from("connection"),
            color_r: r,
            color_g: g,
            color_b: b,
        });

        // If expanded with databases (no specific database configured)
        if has_databases && let Some(dbs) = cache.expanded_databases.get(&conn.id) {
            for db_name in dbs {
                let db_node_id = format!("{}__{}", conn.id, db_name);
                let db_expanded = cache.expanded_db_tables.contains_key(&db_node_id);
                nodes.push(crate::FlatTreeNode {
                    id: slint::SharedString::from(&db_node_id),
                    label: slint::SharedString::from(db_name),
                    icon: slint::SharedString::from(""),
                    depth: 1,
                    is_expandable: true,
                    is_expanded: db_expanded,
                    conn_id: -1,
                    node_type: slint::SharedString::from("database"),
                    color_r: 0,
                    color_g: 0,
                    color_b: 0,
                });

                // Show tables under this database
                if db_expanded && let Some(tables) = cache.expanded_db_tables.get(&db_node_id) {
                    for t in tables {
                        let tbl_expanded = cache.expanded_tables.contains_key(&t.node_id);
                        nodes.push(crate::FlatTreeNode {
                            id: slint::SharedString::from(&t.node_id),
                            label: slint::SharedString::from(&t.table_name),
                            icon: slint::SharedString::from(""),
                            depth: 2,
                            is_expandable: true,
                            is_expanded: tbl_expanded,
                            conn_id: -1,
                            node_type: slint::SharedString::from("table"),
                            color_r: 0,
                            color_g: 0,
                            color_b: 0,
                        });

                        // Show column children if table expanded
                        if tbl_expanded && let Some(cols) = cache.expanded_tables.get(&t.node_id) {
                            for col in cols {
                                nodes.push(crate::FlatTreeNode {
                                    id: slint::SharedString::from(&col.node_id),
                                    label: slint::SharedString::from(&col.column_name),
                                    icon: slint::SharedString::from(""),
                                    depth: 3,
                                    is_expandable: false,
                                    is_expanded: false,
                                    conn_id: -1,
                                    node_type: slint::SharedString::from("column"),
                                    color_r: 0,
                                    color_g: 0,
                                    color_b: 0,
                                });
                            }
                        }
                    }
                }
            }
        }

        // If expanded with direct tables (backward compat: database specified)
        if has_tables && let Some(children) = cache.expanded_connections.get(&conn.id) {
            for child in children {
                let tbl_expanded = cache.expanded_tables.contains_key(&child.node_id);
                nodes.push(crate::FlatTreeNode {
                    id: slint::SharedString::from(&child.node_id),
                    label: slint::SharedString::from(&child.table_name),
                    icon: slint::SharedString::from(""),
                    depth: 1,
                    is_expandable: true,
                    is_expanded: tbl_expanded,
                    conn_id: -1,
                    node_type: slint::SharedString::from("table"),
                    color_r: 0,
                    color_g: 0,
                    color_b: 0,
                });

                // Show column children if table expanded
                if tbl_expanded && let Some(cols) = cache.expanded_tables.get(&child.node_id) {
                    for col in cols {
                        nodes.push(crate::FlatTreeNode {
                            id: slint::SharedString::from(&col.node_id),
                            label: slint::SharedString::from(&col.column_name),
                            icon: slint::SharedString::from(""),
                            depth: 2,
                            is_expandable: false,
                            is_expanded: false,
                            conn_id: -1,
                            node_type: slint::SharedString::from("column"),
                            color_r: 0,
                            color_g: 0,
                            color_b: 0,
                        });
                    }
                }
            }
        }
    }
    nodes
}

/// Push flat tree nodes to the Slint window.
pub fn push_tree(window: &crate::MainWindow, nodes: Vec<crate::FlatTreeNode>) {
    let model = slint::VecModel::from(nodes);
    window.set_tree_nodes(slint::ModelRc::from(Rc::new(model)));
}

/// Expand a connection node: fetch databases or tables from the connector.
pub fn expand_connection(
    app: &Arc<AppState>,
    cache: &mut TreeCache,
    window: &crate::MainWindow,
    conn_id: &str,
) {
    // Try to get from active connections first
    let conn_info = {
        let ac = app.active_connections.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        ac.get(conn_id).cloned()
    };

    let conn_info = match conn_info {
        Some(info) => info,
        None => {
            // Not active — try auto-reconnect from saved config
            let saved = {
                let cm = app.connection_manager.lock().unwrap_or_else(|e| {
                    eprintln!("[bridge] Recovered from poisoned mutex");
                    e.into_inner()
                });
                cm.get(conn_id).cloned()
            };
            let saved = match saved {
                Some(s) => s,
                None => {
                    window.set_status_left(slint::SharedString::from(
                        "Connection not found.",
                    ));
                    return;
                }
            };

            let connector_name = saved.connector_type.clone();
            let new_conn_id = {
                let pm = app.plugin_manager.lock().unwrap_or_else(|e| {
                    eprintln!("[bridge] Recovered from poisoned mutex");
                    e.into_inner()
                });
                let connector = match pm.get_connector(&saved.connector_type) {
                    Some(c) => c,
                    None => {
                        window.set_status_left(slint::SharedString::from(format!(
                            "Connector '{}' not found",
                            saved.connector_type
                        )));
                        return;
                    }
                };
                match connector.connect(&saved.config) {
                    Ok(id) => id,
                    Err(e) => {
                        window.set_status_left(slint::SharedString::from(format!(
                            "Reconnect failed: {}",
                            e
                        )));
                        return;
                    }
                }
            };

            {
                let mut ac = app.active_connections.lock().unwrap_or_else(|e| {
                    eprintln!("[bridge] Recovered from poisoned mutex");
                    e.into_inner()
                });
                ac.insert(
                    conn_id.to_string(),
                    ActiveConnection {
                        conn_id: new_conn_id,
                        connector_name: connector_name.clone(),
                    },
                );
            }
            window.set_status_left(slint::SharedString::from(format!(
                "Reconnected: {}",
                conn_id
            )));
            ActiveConnection {
                conn_id: new_conn_id,
                connector_name,
            }
        }
    };

    // Check if this connection has a specific database configured
    let has_database = {
        let cm = app.connection_manager.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        cm.get(conn_id)
            .map(|c| !c.config.database.is_empty())
            .unwrap_or(false)
    };

    let pm = app.plugin_manager.lock().unwrap_or_else(|e| {
        eprintln!("[bridge] Recovered from poisoned mutex");
        e.into_inner()
    });
    let connector = match pm.get_connector(&conn_info.connector_name) {
        Some(c) => c,
        None => {
            window.set_status_left(slint::SharedString::from(format!(
                "Connector '{}' not found",
                conn_info.connector_name
            )));
            return;
        }
    };

    if has_database {
        // Database specified → list tables directly
        let tables = match connector.list_tables(conn_info.conn_id, "main") {
            Ok(tables) => tables,
            Err(e) => {
                window.set_status_left(slint::SharedString::from(format!(
                    "Failed to list tables: {}",
                    e
                )));
                return;
            }
        };

        let mut children: Vec<TreeNodeEntry> = Vec::new();
        for t in &tables {
            let row_count = {
                let qname = connector.quote_identifier(&t.name);
                let sql = format!("SELECT COUNT(*) FROM {}", qname);
                connector
                    .execute_query(conn_info.conn_id, &sql, Some(1))
                    .ok()
                    .and_then(|r| {
                        r.rows
                            .first()
                            .and_then(|cells| cells.first().and_then(|c| c.display.clone()))
                    })
            };

            let label = match row_count {
                Some(ref count) => format!("{}  ({})", t.name, count),
                None => t.name.clone(),
            };

            children.push(TreeNodeEntry {
                table_name: label,
                node_id: format!("{}__{}", conn_id, t.name),
            });
        }

        // Update SQL completer with table names
        {
            let mut completer = app.sql_completer.lock().unwrap_or_else(|e| {
                eprintln!("[bridge] Recovered from poisoned mutex");
                e.into_inner()
            });
            let table_names: Vec<String> = tables.iter().map(|t| t.name.clone()).collect();
            completer.set_tables(&table_names);
        }

        cache
            .expanded_connections
            .insert(conn_id.to_string(), children);

        window.set_status_left(slint::SharedString::from(format!(
            "{} tables loaded",
            tables.len()
        )));
    } else {
        // No database specified → list databases
        let databases = match connector.list_databases(conn_info.conn_id) {
            Ok(dbs) => dbs,
            Err(e) => {
                window.set_status_left(slint::SharedString::from(format!(
                    "Failed to list databases: {}",
                    e
                )));
                return;
            }
        };

        window.set_status_left(slint::SharedString::from(format!(
            "{} databases loaded — click to expand",
            databases.len()
        )));

        cache
            .expanded_databases
            .insert(conn_id.to_string(), databases);
    }

    let cm = app.connection_manager.lock().unwrap_or_else(|e| {
        eprintln!("[bridge] Recovered from poisoned mutex");
        e.into_inner()
    });
    let nodes = build_tree_with_children(&cm, cache);
    push_tree(window, nodes);
}

/// Expand a database node: fetch tables for this database.
pub fn expand_database_tables(
    app: &Arc<AppState>,
    cache: &mut TreeCache,
    window: &crate::MainWindow,
    node_id: &str,
) {
    // node_id format: conn_name__db_name
    let parts: Vec<&str> = node_id.splitn(2, "__").collect();
    if parts.len() != 2 {
        return;
    }
    let conn_id = parts[0];
    let db_name = parts[1];

    let conn_info = {
        let ac = app.active_connections.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        ac.get(conn_id).cloned()
    };

    let conn_info = match conn_info {
        Some(info) => info,
        None => {
            window.set_status_left(slint::SharedString::from(
                "Connection is not active. Reconnect first.",
            ));
            return;
        }
    };

    let tables = {
        let pm = app.plugin_manager.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        let connector = match pm.get_connector(&conn_info.connector_name) {
            Some(c) => c,
            None => {
                window.set_status_left(slint::SharedString::from(format!(
                    "Connector '{}' not found",
                    conn_info.connector_name
                )));
                return;
            }
        };
        match connector.list_tables(conn_info.conn_id, db_name) {
            Ok(tables) => tables,
            Err(e) => {
                window.set_status_left(slint::SharedString::from(format!(
                    "Failed to list tables: {}",
                    e
                )));
                return;
            }
        }
    };

    let mut children: Vec<TreeNodeEntry> = Vec::new();
    for t in &tables {
        children.push(TreeNodeEntry {
            table_name: t.name.clone(),
            node_id: format!("{}__{}", node_id, t.name),
        });
    }

    cache
        .expanded_db_tables
        .insert(node_id.to_string(), children);

    let cm = app.connection_manager.lock().unwrap_or_else(|e| {
        eprintln!("[bridge] Recovered from poisoned mutex");
        e.into_inner()
    });
    let nodes = build_tree_with_children(&cm, cache);
    push_tree(window, nodes);

    window.set_status_left(slint::SharedString::from(format!(
        "{} tables loaded from {}",
        tables.len(),
        db_name
    )));
}

/// Collapse a database node: remove tables from the cache.
pub fn collapse_database_tables(
    app: &Arc<AppState>,
    cache: &mut TreeCache,
    window: &crate::MainWindow,
    node_id: &str,
) {
    cache.expanded_db_tables.remove(node_id);

    let cm = app.connection_manager.lock().unwrap_or_else(|e| {
        eprintln!("[bridge] Recovered from poisoned mutex");
        e.into_inner()
    });
    let nodes = build_tree_with_children(&cm, cache);
    push_tree(window, nodes);
}

/// Collapse a connection node: remove children from the cache.
pub fn collapse_connection(
    app: &Arc<AppState>,
    cache: &mut TreeCache,
    window: &crate::MainWindow,
    conn_id: &str,
) {
    cache.expanded_connections.remove(conn_id);
    // Clear database-level expansions too
    if let Some(dbs) = cache.expanded_databases.remove(conn_id) {
        for db in &dbs {
            let db_node = format!("{}__{}", conn_id, db);
            cache.expanded_db_tables.remove(&db_node);
        }
    }

    let cm = app.connection_manager.lock().unwrap_or_else(|e| {
        eprintln!("[bridge] Recovered from poisoned mutex");
        e.into_inner()
    });
    let nodes = build_tree_with_children(&cm, cache);
    push_tree(window, nodes);
}

/// Expand a table node: fetch columns from the connector.
pub fn expand_table(
    app: &Arc<AppState>,
    cache: &mut TreeCache,
    window: &crate::MainWindow,
    node_id: &str,
) {
    // node_id format: "conn_id__table_name"
    if let Some(sep) = node_id.rfind("__") {
        let conn_key = &node_id[..sep];
        let table_name = &node_id[sep + 2..];

        let conn_info = {
            let ac = app.active_connections.lock().unwrap_or_else(|e| {
                eprintln!("[bridge] Recovered from poisoned mutex");
                e.into_inner()
            });
            ac.get(conn_key).cloned()
        };

        let columns = match conn_info {
            Some(ref info) => {
                let pm = app.plugin_manager.lock().unwrap_or_else(|e| {
                    eprintln!("[bridge] Recovered from poisoned mutex");
                    e.into_inner()
                });
                let connector = match pm.get_connector(&info.connector_name) {
                    Some(c) => c,
                    None => return,
                };
                match connector.get_table_columns(info.conn_id, "main", table_name) {
                    Ok(cols) => cols,
                    Err(_) => return,
                }
            }
            None => return,
        };

        let col_entries: Vec<ColumnEntry> = columns
            .iter()
            .map(|c| {
                let type_str = format!("{:?}", c.data_type);
                ColumnEntry {
                    column_name: format!("{}  [{}]", c.name, type_str),
                    column_type: type_str,
                    node_id: format!("{}__{}__{}", conn_key, table_name, c.name),
                }
            })
            .collect();

        // Update SQL completer with column names
        {
            let mut completer = app.sql_completer.lock().unwrap_or_else(|e| {
                eprintln!("[bridge] Recovered from poisoned mutex");
                e.into_inner()
            });
            let col_names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
            completer.set_columns_for_table(table_name, &col_names);
        }

        cache
            .expanded_tables
            .insert(node_id.to_string(), col_entries);

        let cm = app.connection_manager.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        let nodes = build_tree_with_children(&cm, cache);
        push_tree(window, nodes);
    }
}

/// Collapse a table node: remove columns from the cache.
pub fn collapse_table(
    app: &Arc<AppState>,
    cache: &mut TreeCache,
    window: &crate::MainWindow,
    node_id: &str,
) {
    cache.expanded_tables.remove(node_id);

    let cm = app.connection_manager.lock().unwrap_or_else(|e| {
        eprintln!("[bridge] Recovered from poisoned mutex");
        e.into_inner()
    });
    let nodes = build_tree_with_children(&cm, cache);
    push_tree(window, nodes);
}

/// Handle clicking a table node: auto-fill query editor and execute.
pub fn select_table(
    app: &Arc<AppState>,
    _cache: &TreeCache,
    window: &crate::MainWindow,
    node_id: &str,
) {
    // Parse node_id: "conn_id__table_name"
    if let Some(sep) = node_id.rfind("__") {
        let conn_key = &node_id[..sep];
        let table_name = &node_id[sep + 2..];

        // Find the connector for quoting
        let conn_info = {
            let ac = app.active_connections.lock().unwrap_or_else(|e| {
                eprintln!("[bridge] Recovered from poisoned mutex");
                e.into_inner()
            });
            ac.get(conn_key)
                .map(|c| (c.connector_name.clone(), c.conn_id))
        };

        let (quoted, connector_name, conn_id) = match conn_info {
            Some((ref name, cid)) => {
                let pm = app.plugin_manager.lock().unwrap_or_else(|e| {
                    eprintln!("[bridge] Recovered from poisoned mutex");
                    e.into_inner()
                });
                let quoted = pm
                    .get_connector(name)
                    .map(|c| c.quote_identifier(table_name))
                    .unwrap_or_else(|| format!("\"{}\"", table_name));
                (quoted, name.clone(), cid)
            }
            None => {
                window.set_status_left(slint::SharedString::from(
                    "Connection is not active. Reconnect first.",
                ));
                return;
            }
        };

        let sql = format!("SELECT * FROM {} LIMIT 1000;", quoted);
        window.set_sql_text(slint::SharedString::from(sql.clone()));

        // Execute the query on the specific connection (not just the first one)
        execute_on_connection(app, window, &connector_name, conn_id, sql);
    }
}

// ── Connection management ───────────────────────────────────

pub fn refresh_tree(app: &Arc<AppState>, window: &crate::MainWindow) {
    let cm = app.connection_manager.lock().unwrap_or_else(|e| {
        eprintln!("[bridge] Recovered from poisoned mutex");
        e.into_inner()
    });
    let nodes = build_connection_tree(&cm);
    push_tree(window, nodes);
}

pub fn disconnect_connection(
    app: &Arc<AppState>,
    cache: &mut TreeCache,
    window: &crate::MainWindow,
    conn_id: &str,
) {
    // Remove from expanded caches
    cache.expanded_connections.remove(conn_id);
    cache
        .expanded_tables
        .retain(|k, _| !k.starts_with(&format!("{}__", conn_id)));

    // Get connection info and disconnect
    let conn_info = {
        let ac = app.active_connections.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        ac.get(conn_id).cloned()
    };

    if let Some(info) = conn_info {
        let pm = app.plugin_manager.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        if let Some(connector) = pm.get_connector(&info.connector_name) {
            let _ = connector.disconnect(info.conn_id);
        }
    }

    // Remove from active connections
    {
        let mut ac = app.active_connections.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        ac.remove(conn_id);
    }

    // Refresh tree
    refresh_tree(app, window);
    window.set_status_left(slint::SharedString::from(format!(
        "Disconnected: {}",
        conn_id
    )));
}

#[allow(clippy::too_many_arguments)]
pub fn connect_to_database(
    app: &Arc<AppState>,
    window: &crate::MainWindow,
    name: String,
    connector_type: String,
    host: String,
    port: u16,
    database: String,
    username: String,
    password: String,
) {
    let pm = app.plugin_manager.lock().unwrap_or_else(|e| {
        eprintln!("[bridge] Recovered from poisoned mutex");
        e.into_inner()
    });
    let connector = match pm.get_connector(&connector_type) {
        Some(c) => c,
        None => {
            window.set_status_left(slint::SharedString::from(format!(
                "Error: No connector for '{}'",
                connector_type
            )));
            return;
        }
    };

    let config = ConnectorConfig {
        host,
        port,
        database,
        username,
        password,
        connection_string: None,
        extra_params: Vec::new(),
    };

    match connector.connect(&config) {
        Ok(conn_id) => {
            window.set_status_left(slint::SharedString::from(format!("Connected: {}", name)));

            {
                let mut ac = app.active_connections.lock().unwrap_or_else(|e| {
                    eprintln!("[bridge] Recovered from poisoned mutex");
                    e.into_inner()
                });
                ac.insert(
                    name.clone(),
                    ActiveConnection {
                        conn_id,
                        connector_name: connector_type.clone(),
                    },
                );
            }

            let color = {
                let cm = app.connection_manager.lock().unwrap_or_else(|e| {
                    eprintln!("[bridge] Recovered from poisoned mutex");
                    e.into_inner()
                });
                let existing = cm.get(&name);
                if let Some(prev) = existing {
                    prev.color.clone()
                } else {
                    let conn_count = cm.list().len();
                    pick_color(conn_count)
                }
            };
            let mut cm = app.connection_manager.lock().unwrap_or_else(|e| {
                eprintln!("[bridge] Recovered from poisoned mutex");
                e.into_inner()
            });
            let existing = cm.get(&name).cloned();
            let saved = SavedConnection {
                id: name.clone(),
                name: name.clone(),
                connector_type,
                config,
                color,
                created_at: existing
                    .as_ref()
                    .map(|c| c.created_at.clone())
                    .unwrap_or_else(chrono_now),
                last_opened: Some(chrono_now()),
            };
            if let Err(e) = cm.save(saved) {
                window.set_status_left(slint::SharedString::from(format!("Save error: {}", e)));
            }
            drop(cm);

            refresh_tree(app, window);
        }
        Err(e) => {
            window.set_status_left(slint::SharedString::from(format!(
                "Connection failed: {}",
                e
            )));
        }
    }
}

// ── Query execution ─────────────────────────────────────────

pub fn execute_on_first_connection(app: &Arc<AppState>, window: &crate::MainWindow, sql: String) {
    let active = {
        let ac = app.active_connections.lock().unwrap_or_else(|e| {
            eprintln!("[bridge] Recovered from poisoned mutex");
            e.into_inner()
        });
        ac.values().next().cloned()
    };

    match active {
        Some(conn) => {
            window.set_result_error(slint::SharedString::from(""));
            window.set_result_status(slint::SharedString::from("Executing..."));

            let app = app.clone();
            let window_weak = window.as_weak();

            query_executor::execute_query(
                &app,
                &conn.connector_name,
                conn.conn_id,
                sql,
                Some(1000),
                Box::new(move |view: QueryResultView| {
                    if let Some(w) = window_weak.upgrade() {
                        apply_query_result(&w, &view);
                    }
                }),
            );
        }
        None => {
            window.set_result_error(slint::SharedString::from(
                "No active connection. Create a connection first.",
            ));
            window.set_has_results(false);
        }
    }
}

/// Apply a QueryResultView to the window (shared by both execute callbacks).
pub fn apply_query_result(w: &crate::MainWindow, view: &QueryResultView) {
    if let Some(err) = &view.error {
        w.set_result_error(slint::SharedString::from(err));
        w.set_has_results(false);
        return;
    }

    let cols: Vec<slint::SharedString> =
        view.columns.iter().map(slint::SharedString::from).collect();
    let col_model = slint::VecModel::from(cols);

    let mut slint_rows = Vec::new();
    for row in &view.rows {
        let cells: Vec<slint::SharedString> = row.iter().map(slint::SharedString::from).collect();
        slint_rows.push(crate::TableRow {
            cells: slint::ModelRc::from(Rc::new(slint::VecModel::from(cells))),
        });
    }
    let row_model = slint::VecModel::from(slint_rows);

    w.set_result_columns(slint::ModelRc::from(Rc::new(col_model)));
    w.set_result_rows(slint::ModelRc::from(Rc::new(row_model)));
    w.set_has_results(true);

    let mut status = format!(
        "{} rows returned in {:.1} ms",
        view.rows.len(),
        view.execution_time_ms
    );
    if view.has_more {
        status.push_str(" (truncated, more rows available)");
    }
    w.set_result_status(slint::SharedString::from(status));
}

/// Execute a query, specifying which connection to use.
pub fn execute_on_connection(
    app: &Arc<AppState>,
    window: &crate::MainWindow,
    connector_name: &str,
    conn_id: u64,
    sql: String,
) {
    window.set_result_error(slint::SharedString::from(""));
    window.set_result_status(slint::SharedString::from("Executing..."));

    let app = app.clone();
    let window_weak = window.as_weak();
    let connector_name = connector_name.to_string();

    query_executor::execute_query(
        &app,
        &connector_name,
        conn_id,
        sql,
        Some(1000),
        Box::new(move |view: QueryResultView| {
            if let Some(w) = window_weak.upgrade() {
                apply_query_result(&w, &view);
            }
        }),
    );
}

// ── Query history ──────────────────────────────────────────

pub fn update_query_history(app: &Arc<AppState>, window: &crate::MainWindow) {
    let history = app.get_query_history();
    let items: Vec<slint::SharedString> = history
        .iter()
        .map(|s| slint::SharedString::from(s.as_str()))
        .collect();
    let model = slint::VecModel::from(items);
    window.set_query_history_items(slint::ModelRc::from(std::rc::Rc::new(model)));
}

// ── Date helpers ────────────────────────────────────────────

fn chrono_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}
