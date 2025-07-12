use mlua::{FromLua, IntoLua, Lua, Value, prelude::LuaError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::hooks::env_keys::EnvKey;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendExecEnvContext {
    pub args: Vec<String>,
    pub tool: String,
    pub version: String,
    pub install_path: PathBuf,
    pub options: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendExecEnvResponse {
    pub env_vars: Vec<EnvKey>,
}

impl IntoLua for BackendExecEnvContext {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("args", self.args)?;
        table.set("tool", self.tool)?;
        table.set("version", self.version)?;
        table.set(
            "install_path",
            self.install_path.to_string_lossy().to_string(),
        )?;
        // Convert serde_json::Value to mlua::Value
        let options_table = lua.create_table()?;
        for (key, value) in self.options {
            let lua_value = serde_json_to_lua_value(lua, value)?;
            options_table.set(key, lua_value)?;
        }
        table.set("options", options_table)?;
        Ok(Value::Table(table))
    }
}

fn serde_json_to_lua_value(lua: &mlua::Lua, value: serde_json::Value) -> mlua::Result<Value> {
    match value {
        serde_json::Value::Null => Ok(Value::Nil),
        serde_json::Value::Bool(b) => Ok(Value::Boolean(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Number(f))
            } else {
                Ok(Value::Nil)
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(lua.create_string(s)?)),
        serde_json::Value::Array(arr) => {
            let table = lua.create_table()?;
            for (i, item) in arr.into_iter().enumerate() {
                table.set(i + 1, serde_json_to_lua_value(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
        serde_json::Value::Object(obj) => {
            let table = lua.create_table()?;
            for (key, value) in obj {
                table.set(key, serde_json_to_lua_value(lua, value)?)?;
            }
            Ok(Value::Table(table))
        }
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
