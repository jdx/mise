use crate::hooks::backend_tools::BackendToolsResponse;
use crate::{Plugin, error::Result};
use mlua::{IntoLua, Lua, Value};

#[derive(Debug, Clone)]
pub struct BackendListToolsContext {}

impl Plugin {
    pub async fn backend_list_tools(
        &self,
        ctx: BackendListToolsContext,
    ) -> Result<BackendToolsResponse> {
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

#[cfg(test)]
mod tests {
    use mlua::{IntoLua, Lua};

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
}
