use mlua::{FromLua, IntoLua, Lua, Value, prelude::LuaError};

#[derive(Debug, Clone)]
pub struct BackendListVersionsContext {
    pub args: Vec<String>,
    pub tool: String,
}

#[derive(Debug, Clone)]
pub struct BackendListVersionsResponse {
    pub versions: Vec<String>,
}

impl IntoLua for BackendListVersionsContext {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("args", self.args)?;
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
