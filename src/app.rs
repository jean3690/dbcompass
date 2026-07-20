use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::connection_manager::ConnectionManager;
use crate::plugin_manager::PluginManager;
use crate::sql_completion::SqlCompleter;

/// Acquire a Mutex lock, recovering from poison if a previous holder panicked.
/// This prevents a single poisoned lock from cascading into a full application panic.
pub fn lock_mutex<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("[app] Recovered from poisoned mutex");
            poisoned.into_inner()
        }
    }
}

#[derive(Clone)]
pub struct ActiveConnection {
    pub conn_id: u64,
    pub connector_name: String,
}

const MAX_HISTORY: usize = 50;

pub struct AppState {
    pub plugin_manager: Mutex<PluginManager>,
    pub connection_manager: Mutex<ConnectionManager>,
    pub active_connections: Mutex<HashMap<String, ActiveConnection>>,
    pub query_history: Mutex<Vec<String>>,
    pub sql_completer: Mutex<SqlCompleter>,
    pub max_rows: Mutex<usize>,
    pub tree_filter: Mutex<String>,
    data_dir: PathBuf,
}

impl AppState {
    pub fn new(plugin_manager: PluginManager, connection_manager: ConnectionManager) -> Arc<Self> {
        let data_dir = connection_manager
            .data_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| connection_manager.data_dir.clone());
        let history = Self::load_history(&data_dir);
        Arc::new(Self {
            plugin_manager: Mutex::new(plugin_manager),
            connection_manager: Mutex::new(connection_manager),
            active_connections: Mutex::new(HashMap::new()),
            query_history: Mutex::new(history),
            sql_completer: Mutex::new(SqlCompleter::new()),
            max_rows: Mutex::new(1000),
            tree_filter: Mutex::new(String::new()),
            data_dir,
        })
    }

    pub fn add_query_history(&self, sql: &str) {
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return;
        }
        let mut history = lock_mutex(&self.query_history);
        // Remove duplicate if exists
        if let Some(pos) = history.iter().position(|h| h == &sql) {
            history.remove(pos);
        }
        history.insert(0, sql);
        history.truncate(MAX_HISTORY);
        drop(history);
        self.save_history();
    }

    pub fn get_query_history(&self) -> Vec<String> {
        lock_mutex(&self.query_history).clone()
    }

    fn history_path(&self) -> PathBuf {
        self.data_dir.join("query_history.json")
    }

    fn load_history(data_dir: &Path) -> Vec<String> {
        let path = data_dir.join("query_history.json");
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(history) = serde_json::from_str::<Vec<String>>(&content)
        {
            return history;
        }
        Vec::new()
    }

    fn save_history(&self) {
        let history = lock_mutex(&self.query_history);
        let path = self.history_path();
        if let Ok(content) = serde_json::to_string(&*history) {
            if let Some(parent) = path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                eprintln!("[app] Failed to create history directory: {e}");
            }
            if let Err(e) = std::fs::write(&path, content) {
                eprintln!("[app] Failed to write query history: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn test_lock_mutex_returns_guard() {
        let m = Mutex::new(42);
        let guard = lock_mutex(&m);
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_lock_mutex_recover_from_poison() {
        let m = Mutex::new(42);
        // Poison the mutex by panicking while holding the lock
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = m.lock().unwrap();
            panic!("intentional panic to poison mutex");
        }));
        assert!(result.is_err());
        // Now lock the poisoned mutex — should recover
        let guard = lock_mutex(&m);
        assert_eq!(*guard, 42);
    }

    /// Create an AppState that uses a temp dir to avoid filesystem pollution.
    fn make_app_state() -> Arc<AppState> {
        let dir = std::env::temp_dir().join("dbcompass-test");
        let _ = std::fs::remove_dir_all(&dir);
        let cm = ConnectionManager::new(dir);
        let pm = PluginManager::new(PathBuf::from("/nonexistent"));
        AppState::new(pm, cm)
    }

    #[test]
    fn test_add_query_history_dedup() {
        let app = make_app_state();

        app.add_query_history("SELECT 1");
        app.add_query_history("SELECT 2");
        app.add_query_history("SELECT 1"); // duplicate — should move to front

        let history = app.get_query_history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0], "SELECT 1");
        assert_eq!(history[1], "SELECT 2");
    }

    #[test]
    fn test_add_query_history_truncates() {
        let app = make_app_state();

        // Add more than MAX_HISTORY (50) entries
        for i in 0..60 {
            app.add_query_history(&format!("SELECT {}", i));
        }

        let history = app.get_query_history();
        assert_eq!(history.len(), 50); // MAX_HISTORY
        assert_eq!(history[0], "SELECT 59"); // newest first
    }

    #[test]
    fn test_add_query_history_ignores_empty() {
        let app = make_app_state();

        app.add_query_history("");
        app.add_query_history("  ");
        assert!(app.get_query_history().is_empty());
    }
}
