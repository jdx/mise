use mlua::{prelude::LuaError, FromLua, IntoLua, Lua, Value};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BackendInstallContext {
    pub args: Vec<String>,
    pub tool: String,
    pub version: String,
    pub install_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BackendInstallResponse {}

impl IntoLua for BackendInstallContext {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("args", self.args)?;
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
