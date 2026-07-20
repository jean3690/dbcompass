use plugin_interface::ConnectorConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedConnection {
    pub id: String,
    pub name: String,
    pub connector_type: String,
    pub config: ConnectorConfig,
    pub color: String,
    pub created_at: String,
    pub last_opened: Option<String>,
}

pub struct ConnectionManager {
    pub data_dir: PathBuf,
    connections: HashMap<String, SavedConnection>,
}

impl ConnectionManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let conns_dir = data_dir.join("connections");
        let mut mgr = Self {
            data_dir: conns_dir,
            connections: HashMap::new(),
        };
        let _ = mgr.load_all();
        mgr
    }

    pub fn list(&self) -> Vec<&SavedConnection> {
        let mut list: Vec<_> = self.connections.values().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn get(&self, id: &str) -> Option<&SavedConnection> {
        self.connections.get(id)
    }

    pub fn save(&mut self, conn: SavedConnection) -> Result<(), String> {
        let id = conn.id.clone();
        let path = self.conn_path(&id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&conn).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())?;
        self.connections.insert(id, conn);
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        self.connections.remove(id);
        let path = self.conn_path(id);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn conn_path(&self, id: &str) -> PathBuf {
        self.data_dir.join(format!("{}.json", id))
    }

    fn load_all(&mut self) -> Result<(), String> {
        if !self.data_dir.exists() {
            return Ok(());
        }
        let dir = std::fs::read_dir(&self.data_dir).map_err(|e| format!("read dir: {}", e))?;
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                match serde_json::from_str::<SavedConnection>(&content) {
                    Ok(conn) => {
                        self.connections.insert(conn.id.clone(), conn);
                    }
                    Err(e) => {
                        eprintln!(
                            "[connection-manager] Skipping invalid connection file {:?}: {}",
                            path.file_name().unwrap_or_default(),
                            e
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
