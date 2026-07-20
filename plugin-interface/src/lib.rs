pub mod database;
pub mod types;

/// 用于 ABI 兼容性检查的版本号
pub const PLUGIN_INTERFACE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub use database::DatabaseConnector;
pub use types::*;

/// 插件元信息
#[derive(Debug, Clone)]
pub struct PluginMeta {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
}

/// 插件必须实现此 trait，所有方法都是 object-safe
pub trait Plugin: Send + Sync {
    fn metadata(&self) -> PluginMeta;

    /// 插件被加载时调用
    fn on_load(&mut self) {}

    /// 插件被卸载前调用
    fn on_unload(&mut self) {}
}

/// 动态库导出的声明结构
pub struct PluginDeclaration {
    pub interface_version: &'static str,
    pub plugin_constructor: Option<fn() -> Box<dyn Plugin>>,
    pub database_connector_constructor: Option<fn() -> Box<dyn DatabaseConnector>>,
}

/// 插件导出函数名：各平台统一使用的符号名
pub const PLUGIN_DECLARATION_SYMBOL: &[u8] = b"_plugin_declaration\0";

/// 声明一个纯插件（无数据库连接器）
#[macro_export]
macro_rules! declare_plugin {
    ($ty:ty, $ctor:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _plugin_declaration() -> $crate::PluginDeclaration {
            fn constructor() -> Box<dyn $crate::Plugin> {
                Box::new($ctor)
            }
            $crate::PluginDeclaration {
                interface_version: $crate::PLUGIN_INTERFACE_VERSION,
                plugin_constructor: Some(constructor),
                database_connector_constructor: None,
            }
        }
    };
    ($plugin_ty:ty, $plugin_ctor:expr, $db_ty:ty, $db_ctor:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _plugin_declaration() -> $crate::PluginDeclaration {
            fn plugin_constructor() -> Box<dyn $crate::Plugin> {
                Box::new($plugin_ctor)
            }
            fn db_constructor() -> Box<dyn $crate::DatabaseConnector> {
                Box::new($db_ctor)
            }
            $crate::PluginDeclaration {
                interface_version: $crate::PLUGIN_INTERFACE_VERSION,
                plugin_constructor: Some(plugin_constructor),
                database_connector_constructor: Some(db_constructor),
            }
        }
    };
}

/// 声明一个纯数据库连接器插件（无通用插件）
#[macro_export]
macro_rules! declare_database_connector {
    ($ty:ty, $ctor:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _plugin_declaration() -> $crate::PluginDeclaration {
            fn db_constructor() -> Box<dyn $crate::DatabaseConnector> {
                Box::new($ctor)
            }
            $crate::PluginDeclaration {
                interface_version: $crate::PLUGIN_INTERFACE_VERSION,
                plugin_constructor: None,
                database_connector_constructor: Some(db_constructor),
            }
        }
    };
}
