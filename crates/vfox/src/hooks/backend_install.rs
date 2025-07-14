use mlua::{prelude::LuaError, FromLua, IntoLua, Lua, Value};
use std::path::PathBuf;

use crate::{error::Result, Plugin};

#[derive(Debug)]
pub struct BackendInstallContext {
    pub tool: String,
    pub version: String,
    pub install_path: PathBuf,
}

#[derive(Debug)]
pub struct BackendInstallResponse {}

impl Plugin {
    pub async fn backend_install(
        &self,
        ctx: BackendInstallContext,
    ) -> Result<BackendInstallResponse> {
        debug!("[vfox:{}] backend_install", &self.name);
        self.eval_async(chunk! {
            require "hooks/backend_install"
            return PLUGIN:BackendInstall($ctx)
        })
        .await
    }
}

impl IntoLua for BackendInstallContext {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("tool", self.tool)?;
        table.set("version", self.version)?;
        table.set(
            "install_path",
            self.install_path.to_string_lossy().to_string(),
        )?;
        Ok(Value::Table(table))
    }
}

impl FromLua for BackendInstallResponse {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(_) => Ok(BackendInstallResponse {}),
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "BackendInstallResponse".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}
