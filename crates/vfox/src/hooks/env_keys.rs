use mlua::prelude::LuaError;
use mlua::{FromLua, IntoLua, Lua, LuaSerdeExt, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::Plugin;
use crate::error::Result;
use crate::sdk_info::SdkInfo;

#[derive(Debug)]
pub struct EnvKey {
    pub key: String,
    pub value: String,
}

#[derive(Debug)]
pub struct EnvKeysContext<T: serde::Serialize> {
    pub args: Vec<String>,
    pub version: String,
    pub path: PathBuf,
    pub main: SdkInfo,
    pub sdk_info: BTreeMap<String, SdkInfo>,
    pub options: T,
}

impl Plugin {
    pub async fn env_keys<T: serde::Serialize>(
        &self,
        ctx: EnvKeysContext<T>,
    ) -> Result<Vec<EnvKey>> {
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

impl<T: serde::Serialize> IntoLua for EnvKeysContext<T> {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("version", self.version)?;
        table.set("path", self.path.to_string_lossy().to_string())?;
        table.set("sdkInfo", self.sdk_info)?;
        table.set("main", self.main)?;
        table.set("options", lua.to_value(&self.options)?)?;
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
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "EnvKey".to_string(),
                message: Some("expected table with key and value fields".to_string()),
            }),
        }
    }
}
