use mlua::prelude::LuaError;
use mlua::{FromLua, IntoLua, Lua, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::error::Result;
use crate::sdk_info::SdkInfo;
use crate::Plugin;

#[derive(Debug)]
pub struct EnvKey {
    pub key: String,
    pub value: String,
}

#[derive(Debug)]
pub struct EnvKeysContext {
    pub args: Vec<String>,
    pub version: String,
    pub path: PathBuf,
    pub main: SdkInfo,
    pub sdk_info: BTreeMap<String, SdkInfo>,
}

impl Plugin {
    pub async fn env_keys(&self, ctx: EnvKeysContext) -> Result<Vec<EnvKey>> {
        debug!("[vfox:{}] env_keys", &self.name);
        let env_keys = self
            .eval_async(chunk! {
                require "hooks/env_keys"
                return PLUGIN:EnvKeys($ctx)
            })
            .await?;

        Ok(env_keys)
    }
}

impl IntoLua for EnvKeysContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("version", self.version)?;
        table.set("path", self.path.to_string_lossy().to_string())?;
        table.set("sdkInfo", self.sdk_info)?;
        table.set("main", self.main)?;
        Ok(Value::Table(table))
    }
}

impl FromLua for EnvKey {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => Ok(EnvKey {
                key: table.get::<String>("key")?,
                value: table.get::<String>("value")?,
            }),
            _ => panic!("Expected table"),
        }
    }
}
