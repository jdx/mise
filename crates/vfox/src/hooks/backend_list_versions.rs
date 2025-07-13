use crate::{error::Result, Plugin};
use mlua::{prelude::LuaError, FromLua, IntoLua, Lua, Value};

#[derive(Debug, Clone)]
pub struct BackendListVersionsContext {
    pub tool: String,
}

#[derive(Debug, Clone)]
pub struct BackendListVersionsResponse {
    pub versions: Vec<String>,
}

impl Plugin {
    pub async fn backend_list_versions(
        &self,
        ctx: BackendListVersionsContext,
    ) -> Result<BackendListVersionsResponse> {
        debug!("[vfox:{}] backend_list_versions", &self.name);
        self.eval_async(chunk! {
            require "hooks/backend_list_versions"
            return PLUGIN:BackendListVersions($ctx)
        })
        .await
    }
}

impl IntoLua for BackendListVersionsContext {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("tool", self.tool)?;
        Ok(Value::Table(table))
    }
}

impl FromLua for BackendListVersionsResponse {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => Ok(BackendListVersionsResponse {
                versions: table.get::<Vec<String>>("versions")?,
            }),
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "BackendListVersionsResponse".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}
