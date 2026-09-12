use mlua::{FromLua, Lua, Value, prelude::LuaError};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BackendTool {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BackendToolsResponse {
    pub tools: Vec<BackendTool>,
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

impl FromLua for BackendToolsResponse {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => Ok(Self {
                tools: table.get("tools")?,
            }),
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "BackendToolsResponse".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use mlua::{FromLua, Lua};

    use super::*;

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
        let response = BackendToolsResponse::from_lua(value, &lua).unwrap();
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
