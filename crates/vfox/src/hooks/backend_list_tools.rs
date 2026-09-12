use crate::{Plugin, error::Result};
use mlua::{FromLua, IntoLua, Lua, Value, prelude::LuaError};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BackendTool {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BackendListToolsContext {}

#[derive(Debug, Clone)]
pub struct BackendListToolsResponse {
    pub tools: Vec<BackendTool>,
}

impl Plugin {
    pub async fn backend_list_tools(
        &self,
        ctx: BackendListToolsContext,
    ) -> Result<BackendListToolsResponse> {
        debug!("[vfox:{}] backend_list_tools", self.name);
        self.eval_async(chunk! {
            require "hooks/backend_list_tools"
            return PLUGIN:BackendListTools($ctx)
        })
        .await
    }
}

impl IntoLua for BackendListToolsContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        Ok(Value::Table(lua.create_table()?))
    }
}

impl FromLua for BackendTool {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => Ok(Self {
                name: table.get("name")?,
                description: table.get("description")?,
            }),
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "BackendTool".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}

impl FromLua for BackendListToolsResponse {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => Ok(Self {
                tools: table.get("tools")?,
            }),
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "BackendListToolsResponse".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use mlua::{FromLua, IntoLua, Lua};

    use super::*;

    #[test]
    fn test_context_is_empty_table() {
        let lua = Lua::new();
        let value = BackendListToolsContext {}.into_lua(&lua).unwrap();
        let Value::Table(table) = value else {
            panic!("expected table");
        };
        assert!(table.is_empty());
    }

    #[test]
    fn test_response_with_optional_descriptions() {
        let lua = Lua::new();
        let value = lua
            .load(
                r#"return { tools = {
                    { name = "prettier", description = "JavaScript formatter" },
                    { name = "eslint" },
                } }"#,
            )
            .eval()
            .unwrap();
        let response = BackendListToolsResponse::from_lua(value, &lua).unwrap();
        assert_eq!(
            response.tools,
            vec![
                BackendTool {
                    name: "prettier".into(),
                    description: Some("JavaScript formatter".into()),
                },
                BackendTool {
                    name: "eslint".into(),
                    description: None,
                },
            ]
        );
    }
}
