use mlua::prelude::LuaError;
use mlua::{FromLua, IntoLua, Lua, LuaSerdeExt, Value};
use std::path::PathBuf;

use crate::Plugin;
use crate::error::Result;
use crate::hooks::env_keys::EnvKey;

#[derive(Debug)]
pub struct MiseEnvContext<T: serde::Serialize> {
    pub args: Vec<String>,
    pub options: T,
}

/// Result from a mise_env hook call
/// Supports both legacy format (just array of env keys) and extended format
/// with cache metadata
#[derive(Debug, Default)]
pub struct MiseEnvResult {
    /// Environment variables to set
    pub env: Vec<EnvKey>,
    /// Whether this module's output can be cached
    /// Defaults to false for backward compatibility
    pub cacheable: bool,
    /// Files to watch for cache invalidation
    pub watch_files: Vec<PathBuf>,
    /// Whether the plugin wants its env vars to be redacted
    /// When true, mise will redact these values unless the user explicitly opts out
    pub redact: bool,
}

impl Plugin {
    pub async fn mise_env<T: serde::Serialize>(
        &self,
        ctx: MiseEnvContext<T>,
    ) -> Result<MiseEnvResult> {
        debug!("[vfox:{}] mise_env", &self.name);
        let result = self
            .eval_async(chunk! {
                require "hooks/mise_env"
                return PLUGIN:MiseEnv($ctx)
            })
            .await?;

        Ok(result)
    }
}

impl<T: serde::Serialize> IntoLua for MiseEnvContext<T> {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("options", lua.to_value(&self.options)?)?;
        Ok(Value::Table(table))
    }
}

impl FromLua for MiseEnvResult {
    fn from_lua(value: Value, lua: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            // Extended format: { cacheable = true, watch_files = {...}, env = {...} }
            Value::Table(table) => {
                // Check if this is extended format by looking for known keys
                let has_env = table.contains_key("env")?;
                let has_cacheable = table.contains_key("cacheable")?;
                let has_watch_files = table.contains_key("watch_files")?;
                let has_redact = table.contains_key("redact")?;

                if has_env || has_cacheable || has_watch_files || has_redact {
                    // Extended format
                    let env: Vec<EnvKey> = table
                        .get::<Option<Vec<EnvKey>>>("env")
                        .map_err(|e| {
                            LuaError::RuntimeError(format!(
                                "Invalid 'env' field in MiseEnv result: expected array of {{key, value}} pairs. Error: {e}"
                            ))
                        })?
                        .unwrap_or_default();
                    let cacheable: bool = table
                        .get::<Option<bool>>("cacheable")
                        .map_err(|e| {
                            LuaError::RuntimeError(format!(
                                "Invalid 'cacheable' field in MiseEnv result: expected boolean. Error: {e}"
                            ))
                        })?
                        .unwrap_or(false);
                    let watch_files: Vec<String> = table
                        .get::<Option<Vec<String>>>("watch_files")
                        .map_err(|e| {
                            LuaError::RuntimeError(format!(
                                "Invalid 'watch_files' field in MiseEnv result: expected array of strings. Error: {e}"
                            ))
                        })?
                        .unwrap_or_default();
                    let redact: bool = table
                        .get::<Option<bool>>("redact")
                        .map_err(|e| {
                            LuaError::RuntimeError(format!(
                                "Invalid 'redact' field in MiseEnv result: expected boolean. Error: {e}"
                            ))
                        })?
                        .unwrap_or(false);

                    Ok(MiseEnvResult {
                        env,
                        cacheable,
                        watch_files: watch_files.into_iter().map(PathBuf::from).collect(),
                        redact,
                    })
                } else {
                    // Legacy format: table is actually an array of env keys
                    // Try to parse as array
                    let env: Vec<EnvKey> = Vec::from_lua(Value::Table(table), lua).map_err(|e| {
                        LuaError::RuntimeError(format!(
                            "Failed to parse MiseEnv hook result. Expected either:\n\
                             - Legacy format: array of {{key, value}} pairs like {{{{\"KEY\", \"VALUE\"}}, ...}}\n\
                             - Extended format: table with 'env' field like {{env = {{}}, cacheable = true}}\n\
                             Error: {e}"
                        ))
                    })?;
                    Ok(MiseEnvResult {
                        env,
                        cacheable: false,
                        watch_files: vec![],
                        redact: false,
                    })
                }
            }
            // Empty/nil result
            Value::Nil => Ok(MiseEnvResult::default()),
            _ => Err(LuaError::RuntimeError(
                "Expected table or nil from MiseEnv hook".to_string(),
            )),
        }
    }
}
