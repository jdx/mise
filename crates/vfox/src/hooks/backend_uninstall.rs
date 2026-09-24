use indexmap::IndexMap;
use std::path::PathBuf;

use mlua::{IntoLua, LuaSerdeExt, Value};

use crate::{Plugin, error::Result};

#[derive(Debug)]
pub struct BackendUninstallContext {
    pub tool: String,
    pub version: String,
    pub install_path: PathBuf,
    pub download_path: PathBuf,
    pub options: IndexMap<String, toml::Value>,
}

impl Plugin {
    pub async fn backend_uninstall(&self, ctx: BackendUninstallContext) -> Result<()> {
        debug!("[vfox:{}] backend_uninstall", self.name);
        self.exec_async(chunk! {
            require "hooks/backend_uninstall"
            PLUGIN:BackendUninstall($ctx)
        })
        .await
    }
}

impl IntoLua for BackendUninstallContext {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("tool", self.tool)?;
        table.set("version", self.version)?;
        table.set(
            "install_path",
            self.install_path.to_string_lossy().to_string(),
        )?;
        table.set(
            "download_path",
            self.download_path.to_string_lossy().to_string(),
        )?;
        table.set("options", lua.to_value(&self.options)?)?;
        Ok(Value::Table(table))
    }
}
