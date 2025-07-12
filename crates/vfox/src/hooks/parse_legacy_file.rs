use mlua::prelude::LuaError;
use mlua::{FromLua, IntoLua, Lua, MultiValue, Value};
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::Plugin;

#[derive(Debug)]
pub struct LegacyFileContext {
    pub args: Vec<String>,
    pub filepath: PathBuf,
}

#[derive(Debug)]
pub struct ParseLegacyFileResponse {
    pub version: Option<String>,
}

impl Plugin {
    pub async fn parse_legacy_file(&self, legacy_file: &Path) -> Result<ParseLegacyFileResponse> {
        debug!("[vfox:{}] parse_legacy_file", &self.name);
        let ctx = LegacyFileContext {
            args: vec![],
            filepath: legacy_file.to_path_buf(),
        };
        let legacy_file_response = self
            .eval_async(chunk! {
                require "hooks/available"
                require "hooks/parse_legacy_file"
                return PLUGIN:ParseLegacyFile($ctx)
            })
            .await?;

        Ok(legacy_file_response)
    }
}

impl IntoLua for LegacyFileContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("args", self.args)?;
        table.set("filepath", self.filepath.to_string_lossy().to_string())?;
        table.set(
            "getInstalledVersions",
            lua.create_async_function(|lua, _input: MultiValue| async move {
                let plugin_dir = lua.named_registry_value::<PathBuf>("plugin_dir")?;
                Ok(Plugin::from_dir(plugin_dir.as_path())
                    .map_err(|e| LuaError::RuntimeError(e.to_string()))?
                    .available_async()
                    .await
                    .map_err(|e| LuaError::RuntimeError(e.to_string()))?
                    .into_iter()
                    .map(|v| v.version)
                    .collect::<Vec<String>>())
            })?,
        )?;
        Ok(Value::Table(table))
    }
}

impl FromLua for ParseLegacyFileResponse {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => Ok(ParseLegacyFileResponse {
                version: table.get::<Option<String>>("version")?,
            }),
            _ => panic!("Expected table"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vfox;

    #[tokio::test]
    async fn test_parse_legacy_file_nodejs() {
        let vfox = Vfox::test();
        let response = vfox
            .parse_legacy_file("nodejs", Path::new("test/data/.node-version"))
            .await
            .unwrap();
        let out = format!("{response:?}");
        assert_snapshot!(out, @r###"ParseLegacyFileResponse { version: Some("20.0.0") }"###);
    }

    #[tokio::test]
    async fn test_parse_legacy_file_dummy() {
        let vfox = Vfox::test();
        let response = vfox
            .parse_legacy_file("dummy", Path::new("test/data/.dummy-version"))
            .await
            .unwrap();
        let out = format!("{response:?}");
        assert_snapshot!(out, @r###"ParseLegacyFileResponse { version: Some("1.0.0") }"###);
    }
}
