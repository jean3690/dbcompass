use libloading::{Library, Symbol};
use plugin_interface::{DatabaseConnector, PLUGIN_DECLARATION_SYMBOL, Plugin, PluginDeclaration};
use std::path::{Path, PathBuf};

pub struct PluginHandle {
    pub plugin: Option<Box<dyn Plugin>>,
    pub database_connector: Option<Box<dyn DatabaseConnector>>,
    _lib: Library,
}

pub struct PluginManager {
    plugins_dir: PathBuf,
    handles: Vec<PluginHandle>,
}

impl PluginManager {
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self {
            plugins_dir,
            handles: Vec::new(),
        }
    }

    /// Scan plugins directory and load all compatible libraries
    pub fn load_all(&mut self) {
        let dir = match std::fs::read_dir(&self.plugins_dir) {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!(
                    "[plugin-manager] Cannot read plugins dir {:?}: {}",
                    self.plugins_dir, e
                );
                return;
            }
        };

        for entry in dir.flatten() {
            let path = entry.path();
            if !Self::is_plugin_library(&path) {
                continue;
            }
            self.load_plugin(&path);
        }
    }

    /// Unload all plugins (LIFO order)
    pub fn unload_all(&mut self) {
        while let Some(mut handle) = self.handles.pop() {
            if let Some(ref mut p) = handle.plugin {
                p.on_unload();
                let meta = p.metadata();
                println!("[plugin-manager] Unloaded: {} v{}", meta.name, meta.version);
            }
        }
    }

    /// Get a database connector by name
    pub fn get_connector(&self, name: &str) -> Option<&dyn DatabaseConnector> {
        self.handles.iter().find_map(|h| {
            h.database_connector.as_ref().and_then(|c| {
                if c.name() == name {
                    Some(c.as_ref())
                } else {
                    None
                }
            })
        })
    }

    /// List all available database connectors: Vec<(name, description)>
    pub fn list_connectors(&self) -> Vec<(&str, &str)> {
        self.handles
            .iter()
            .filter_map(|h| {
                let name = h.database_connector.as_ref().map(|c| c.name());
                let desc = h.plugin.as_ref().map(|p| p.metadata().description);
                name.map(|n| (n, desc.unwrap_or("")))
            })
            .collect()
    }

    fn load_plugin(&mut self, path: &Path) {
        // SAFETY: Loading a dynamic library is inherently unsafe because it executes
        // the library's constructors and init code. The caller must ensure:
        // 1. The library file is a valid, untampered dynamic library compiled for
        //    the same platform and ABI
        // 2. The library's init code does not cause undefined behavior
        // ABI compatibility is verified below via interface_version check.
        let lib = unsafe {
            match Library::new(path) {
                Ok(lib) => lib,
                Err(e) => {
                    eprintln!(
                        "[plugin-manager] Failed to load {:?}: {}",
                        path.file_name().unwrap_or_default(),
                        e
                    );
                    return;
                }
            }
        };

        // SAFETY: lib.get() returns a symbol pointer from the loaded library.
        // This is safe because:
        // 1. The symbol name _plugin_declaration is guaranteed null-terminated
        // 2. We verify the function signature matches via PluginDeclaration struct
        // 3. The lib reference is kept alive (stored in PluginHandle._lib) for the
        //    lifetime of the loaded symbols
        let decl: Symbol<extern "C" fn() -> PluginDeclaration> = unsafe {
            match lib.get(PLUGIN_DECLARATION_SYMBOL) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "[plugin-manager] Symbol not found in {:?}: {}",
                        path.file_name().unwrap_or_default(),
                        e
                    );
                    return;
                }
            }
        };

        let declaration = decl();

        if declaration.interface_version != plugin_interface::PLUGIN_INTERFACE_VERSION {
            eprintln!(
                "[plugin-manager] ABI mismatch in {:?}: plugin v{}, host v{}",
                path.file_name().unwrap_or_default(),
                declaration.interface_version,
                plugin_interface::PLUGIN_INTERFACE_VERSION
            );
            return;
        }

        let plugin = declaration.plugin_constructor.map(|ctor| {
            let mut p = ctor();
            let meta = p.metadata();
            println!("[plugin-manager] Loaded: {} v{}", meta.name, meta.version);
            p.on_load();
            p
        });

        let database_connector = declaration.database_connector_constructor.map(|ctor| {
            let c = ctor();
            println!("[plugin-manager] Loaded database connector: {}", c.name());
            c
        });

        self.handles.push(PluginHandle {
            _lib: lib,
            plugin,
            database_connector,
        });
    }

    #[cfg(target_os = "windows")]
    fn is_plugin_library(path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "dll")
    }

    #[cfg(target_os = "macos")]
    fn is_plugin_library(path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "dylib")
    }

    #[cfg(target_os = "linux")]
    fn is_plugin_library(path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "so")
    }
}
