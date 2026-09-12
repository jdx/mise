use crate::hooks::backend_tools::BackendToolsResponse;
use crate::{Plugin, error::Result};
use mlua::{IntoLua, Lua, Value};

#[derive(Debug, Clone)]
pub struct BackendSearchToolsContext {
    pub query: String,
}

impl Plugin {
    pub async fn backend_search_tools(
        &self,
        ctx: BackendSearchToolsContext,
    ) -> Result<BackendToolsResponse> {
        debug!("[vfox:{}] backend_search_tools", self.name);
        self.eval_async(chunk! {
            require "hooks/backend_search_tools"
            return PLUGIN:BackendSearchTools($ctx)
        })
        .await
    }
}

impl IntoLua for BackendSearchToolsContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("query", self.query)?;
        Ok(Value::Table(table))
    }
}

#[cfg(test)]
mod tests {
    use mlua::{IntoLua, Lua};

    use super::*;

    #[test]
    fn test_context_contains_query() {
        let lua = Lua::new();
        let value = BackendSearchToolsContext {
            query: "format".into(),
        }
        .into_lua(&lua)
        .unwrap();
        let Value::Table(table) = value else {
            panic!("expected table");
        };
        assert_eq!(table.get::<String>("query").unwrap(), "format");
    }
}
