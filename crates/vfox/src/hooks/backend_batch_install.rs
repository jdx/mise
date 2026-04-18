use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::PathBuf;

use mlua::{FromLua, IntoLua, Lua, LuaSerdeExt, Value, prelude::LuaError};

use crate::{Plugin, error::Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackendBatchInstallItem {
    pub id: String,
    pub tool: String,
    pub version: String,
    pub install_path: PathBuf,
    pub download_path: PathBuf,
    pub options: IndexMap<String, toml::Value>,
}

#[derive(Debug, Clone)]
pub struct BackendBatchInstallContext {
    pub tools: Vec<BackendBatchInstallItem>,
}

#[derive(Debug, Clone)]
pub struct BackendBatchInstallResult {
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BackendBatchInstallResponse {
    pub results: HashMap<String, BackendBatchInstallResult>,
}

impl Plugin {
    pub async fn backend_batch_install(
        &self,
        ctx: BackendBatchInstallContext,
    ) -> Result<BackendBatchInstallResponse> {
        debug!("[vfox:{}] backend_batch_install", &self.name);
        self.eval_async(chunk! {
            require "hooks/backend_batch_install"
            return PLUGIN:BackendBatchInstall($ctx)
        })
        .await
    }
}

impl IntoLua for BackendBatchInstallItem {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("id", self.id)?;
        table.set("tool", self.tool)?;
        table.set("version", self.version)?;
        table.set(
            "install_path",
            self.install_path.to_string_lossy().to_string(),
        )?;
        table.set(
            "download_path",
            self.download_path.to_string_lossy().to_string(),
        )?;
        table.set("options", lua.to_value(&self.options)?)?;
        Ok(Value::Table(table))
    }
}

impl IntoLua for BackendBatchInstallContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("tools", lua.create_sequence_from(self.tools)?)?;
        Ok(Value::Table(table))
    }
}

impl FromLua for BackendBatchInstallResult {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => {
                let result = Self {
                    version: table.get("version")?,
                    error: table.get("error")?,
                };
                if result.version.is_none() && result.error.is_none() {
                    return Err(LuaError::FromLuaConversionError {
                        from: "table",
                        to: "BackendBatchInstallResult".to_string(),
                        message: Some(
                            "Expected result table to contain either 'version' or 'error'"
                                .to_string(),
                        ),
                    });
                }
                Ok(result)
            }
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "BackendBatchInstallResult".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_batch_install_result_accepts_version_or_error() {
        let lua = Lua::new();

        let version_table = lua.create_table().unwrap();
        version_table.set("version", "1.0.0").unwrap();
        let version_result =
            BackendBatchInstallResult::from_lua(Value::Table(version_table), &lua).unwrap();
        assert_eq!(version_result.version.as_deref(), Some("1.0.0"));
        assert_eq!(version_result.error, None);

        let error_table = lua.create_table().unwrap();
        error_table.set("error", "boom").unwrap();
        let error_result =
            BackendBatchInstallResult::from_lua(Value::Table(error_table), &lua).unwrap();
        assert_eq!(error_result.version, None);
        assert_eq!(error_result.error.as_deref(), Some("boom"));
    }

    #[test]
    fn backend_batch_install_result_rejects_empty_table() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();

        let err = BackendBatchInstallResult::from_lua(Value::Table(table), &lua).unwrap_err();
        assert!(err.to_string().contains("either 'version' or 'error'"));
    }
}

impl FromLua for BackendBatchInstallResponse {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => {
                let mut results = HashMap::new();
                for pair in table.pairs::<String, BackendBatchInstallResult>() {
                    let (id, result) = pair?;
                    results.insert(id, result);
                }
                Ok(Self { results })
            }
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "BackendBatchInstallResponse".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}
