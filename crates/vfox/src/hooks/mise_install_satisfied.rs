use mlua::prelude::LuaError;
use mlua::{FromLua, IntoLua, Lua, LuaSerdeExt, Value};
use std::path::PathBuf;

use crate::Plugin;
use crate::error::Result;

#[derive(Debug)]
pub struct MiseInstallSatisfiedContext<T: serde::Serialize> {
    pub version: String,
    pub path: PathBuf,
    pub options: T,
}

/// Whether an installed version still matches what was requested, for install
/// state a plugin manages beyond the version itself (e.g. add-on components
/// selected with tool options).
#[derive(Debug, PartialEq, Eq)]
pub struct MiseInstallSatisfiedResult {
    pub satisfied: bool,
    /// Why the install is not satisfied, for debug output.
    pub reason: Option<String>,
}

impl Default for MiseInstallSatisfiedResult {
    fn default() -> Self {
        Self {
            satisfied: true,
            reason: None,
        }
    }
}

impl Plugin {
    pub async fn mise_install_satisfied<T: serde::Serialize>(
        &self,
        ctx: MiseInstallSatisfiedContext<T>,
    ) -> Result<MiseInstallSatisfiedResult> {
        debug!("[vfox:{}] mise_install_satisfied", self.name);
        self.eval_async(chunk! {
            require "hooks/mise_install_satisfied"
            return PLUGIN:MiseInstallSatisfied($ctx)
        })
        .await
    }
}

impl<T: serde::Serialize> IntoLua for MiseInstallSatisfiedContext<T> {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("version", self.version)?;
        table.set("path", self.path.to_string_lossy().to_string())?;
        table.set("options", lua.to_value(&self.options)?)?;
        Ok(Value::Table(table))
    }
}

impl FromLua for MiseInstallSatisfiedResult {
    fn from_lua(value: Value, _lua: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Nil => Ok(Self::default()),
            Value::Boolean(satisfied) => Ok(Self {
                satisfied,
                reason: None,
            }),
            Value::Table(table) => {
                let satisfied = table.get::<Option<bool>>("satisfied").map_err(|e| {
                    LuaError::RuntimeError(format!(
                        "Invalid 'satisfied' field in MiseInstallSatisfied result: expected boolean. Error: {e}"
                    ))
                })?;
                let Some(satisfied) = satisfied else {
                    return Err(LuaError::RuntimeError(
                        "MiseInstallSatisfied result is missing the 'satisfied' field".to_string(),
                    ));
                };
                let reason = table.get::<Option<String>>("reason").map_err(|e| {
                    LuaError::RuntimeError(format!(
                        "Invalid 'reason' field in MiseInstallSatisfied result: expected string. Error: {e}"
                    ))
                })?;
                Ok(Self { satisfied, reason })
            }
            _ => Err(LuaError::RuntimeError(
                "Expected boolean, table, or nil from MiseInstallSatisfied hook".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(code: &str) -> std::result::Result<MiseInstallSatisfiedResult, LuaError> {
        let lua = Lua::new();
        let value = lua.load(code).eval::<Value>()?;
        MiseInstallSatisfiedResult::from_lua(value, &lua)
    }

    #[test]
    fn parses_results() {
        assert_eq!(
            parse("return nil").unwrap(),
            MiseInstallSatisfiedResult::default()
        );
        assert!(!parse("return false").unwrap().satisfied);
        assert_eq!(
            parse(r#"return { satisfied = false, reason = "missing beta" }"#).unwrap(),
            MiseInstallSatisfiedResult {
                satisfied: false,
                reason: Some("missing beta".to_string()),
            }
        );
        assert!(parse("return {}").is_err());
        assert!(parse(r#"return "yes""#).is_err());
    }
}
