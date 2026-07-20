use plugin_interface::{Plugin, PluginMeta};

struct HelloPlugin;

impl Plugin for HelloPlugin {
    fn metadata(&self) -> PluginMeta {
        PluginMeta {
            name: "hello-plugin",
            version: "0.1.0",
            description: "A simple hello-world example plugin.",
        }
    }

    fn on_load(&mut self) {
        println!("[hello-plugin] Hello from the plugin system!");
    }

    fn on_unload(&mut self) {
        println!("[hello-plugin] Goodbye!");
    }
}

plugin_interface::declare_plugin!(HelloPlugin, HelloPlugin);
