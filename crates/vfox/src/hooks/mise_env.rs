use mlua::{IntoLua, Lua, LuaSerdeExt, Value};

use crate::error::Result;
use crate::hooks::env_keys::EnvKey;
use crate::Plugin;

#[derive(Debug)]
pub struct MiseEnvContext<T: serde::Serialize> {
    pub args: Vec<String>,
    pub options: T,
}

impl Plugin {
    pub async fn mise_env<T: serde::Serialize>(
        &self,
        ctx: MiseEnvContext<T>,
    ) -> Result<Vec<EnvKey>> {
        debug!("[vfox:{}] mise_env", &self.name);
        let env_keys = self
            .eval_async(chunk! {
                require "hooks/mise_env"
                return PLUGIN:MiseEnv($ctx)
            })
            .await?;

        Ok(env_keys)
    }
}

impl<T: serde::Serialize> IntoLua for MiseEnvContext<T> {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("options", lua.to_value(&self.options)?)?;
        Ok(Value::Table(table))
    }
}
