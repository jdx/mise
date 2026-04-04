use indexmap::IndexMap;
use std::path::PathBuf;

use mlua::{FromLua, IntoLua, Lua, LuaSerdeExt, Value, prelude::LuaError};

use crate::{Plugin, error::Result, hooks::env_keys::EnvKey};

#[derive(Debug, Clone)]
pub struct BackendExecEnvContext {
    pub tool: String,
    pub version: String,
    pub install_path: PathBuf,
    pub options: IndexMap<String, toml::Value>,
}

#[derive(Debug)]
pub struct BackendExecEnvResponse {
    pub env_vars: Vec<EnvKey>,
}

impl Plugin {
    pub async fn backend_exec_env(
        &self,
        ctx: BackendExecEnvContext,
    ) -> Result<BackendExecEnvResponse> {
        debug!("[vfox:{}] backend_exec_env", &self.name);
        self.eval_async(chunk! {
            require "hooks/backend_exec_env"
            return PLUGIN:BackendExecEnv($ctx)
        })
        .await
    }
}

impl IntoLua for BackendExecEnvContext {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("tool", self.tool)?;
        table.set("version", self.version)?;
        table.set(
            "install_path",
            self.install_path.to_string_lossy().to_string(),
        )?;
        table.set("options", lua.to_value(&self.options)?)?;
        Ok(Value::Table(table))
    }
}

impl FromLua for BackendExecEnvResponse {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => Ok(BackendExecEnvResponse {
                env_vars: table.get::<Vec<crate::hooks::env_keys::EnvKey>>("env_vars")?,
            }),
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "BackendExecEnvResponse".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}
