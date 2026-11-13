//! Hooks of a bootstrap settings plugin, which reads and writes the values declared under
//! `[bootstrap.settings.<plugin>]`. mise compares the two and writes only what differs.

use indexmap::IndexMap;
use mlua::{FromLua, IntoLua, Lua, LuaSerdeExt, Value, prelude::LuaError};

use crate::{Plugin, error::Result};

/// The dotted keys mise wants the current values of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsReadContext {
    pub keys: Vec<String>,
}

/// Current values by dotted key. A key left out is unset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsReadResponse {
    pub values: IndexMap<String, serde_json::Value>,
}

/// One key whose current value differs from the declared one.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingChange {
    pub key: String,
    /// The current value, or `None` when the key is unset.
    pub from: Option<serde_json::Value>,
    pub to: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsWriteContext {
    pub changes: Vec<SettingChange>,
}

/// Steps the user must take for a change to show, such as logging out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsWriteResponse {
    pub follow_up: Vec<String>,
}

impl Plugin {
    pub async fn settings_read(&self, ctx: SettingsReadContext) -> Result<SettingsReadResponse> {
        debug!("[vfox:{}] settings_read", self.name);
        self.eval_async(chunk! {
            require "hooks/settings_read"
            return PLUGIN:SettingsRead($ctx)
        })
        .await
    }

    pub async fn settings_write(&self, ctx: SettingsWriteContext) -> Result<SettingsWriteResponse> {
        debug!("[vfox:{}] settings_write", self.name);
        self.eval_async(chunk! {
            require "hooks/settings_write"
            return PLUGIN:SettingsWrite($ctx)
        })
        .await
    }
}

impl IntoLua for SettingsReadContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("keys", lua.create_sequence_from(self.keys)?)?;
        Ok(Value::Table(table))
    }
}

impl IntoLua for SettingsWriteContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let changes = lua.create_table()?;
        for (index, change) in self.changes.into_iter().enumerate() {
            let item = lua.create_table()?;
            item.set("key", change.key)?;
            if let Some(from) = change.from {
                item.set("from", lua.to_value(&from)?)?;
            }
            item.set("to", lua.to_value(&change.to)?)?;
            changes.set(index + 1, item)?;
        }
        let table = lua.create_table()?;
        table.set("changes", changes)?;
        Ok(Value::Table(table))
    }
}

fn expected_table(value: &Value, to: &str) -> LuaError {
    LuaError::FromLuaConversionError {
        from: value.type_name(),
        to: to.to_string(),
        message: Some("Expected table".to_string()),
    }
}

impl FromLua for SettingsReadResponse {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let Value::Table(table) = value else {
            return Err(expected_table(&value, "SettingsReadResponse"));
        };
        let mut values = IndexMap::new();
        if let Some(found) = table.get::<Option<mlua::Table>>("values")? {
            for pair in found.pairs::<String, Value>() {
                let (key, value) = pair?;
                values.insert(key, lua.from_value(value)?);
            }
        }
        Ok(Self { values })
    }
}

impl FromLua for SettingsWriteResponse {
    fn from_lua(value: Value, _: &Lua) -> mlua::Result<Self> {
        match value {
            Value::Nil => Ok(Self::default()),
            Value::Table(table) => {
                let follow_up = match table.get::<Option<mlua::Table>>("follow_up")? {
                    Some(items) => items
                        .sequence_values::<String>()
                        .collect::<mlua::Result<Vec<_>>>()?,
                    None => vec![],
                };
                Ok(Self { follow_up })
            }
            _ => Err(expected_table(&value, "SettingsWriteResponse")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plugin(read: &str, write: &str) -> (tempfile::TempDir, Plugin) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("hooks")).unwrap();
        std::fs::write(
            tmp.path().join("metadata.lua"),
            r#"PLUGIN = { name = "fake", version = "0.1.0", homepage = "", license = "MIT",
                description = "fake", minRuntimeVersion = "0.3.0", notes = {}, legacyFilenames = {} }"#,
        )
        .unwrap();
        std::fs::write(tmp.path().join("hooks/settings_read.lua"), read).unwrap();
        std::fs::write(tmp.path().join("hooks/settings_write.lua"), write).unwrap();
        let plugin = Plugin::from_dir(tmp.path()).unwrap();
        (tmp, plugin)
    }

    #[tokio::test]
    async fn read_and_write_round_trip_typed_values() {
        let (_tmp, plugin) = plugin(
            r#"function PLUGIN:SettingsRead(ctx)
                assert(#ctx.keys == 2)
                assert(ctx.keys[1] == "appearance.blur")
                return { values = { ["appearance.blur"] = false, ["appearance.size"] = 2 } }
            end"#,
            r#"function PLUGIN:SettingsWrite(ctx)
                assert(#ctx.changes == 2)
                assert(ctx.changes[1].key == "appearance.blur")
                assert(ctx.changes[1].from == false)
                assert(ctx.changes[1].to == true)
                assert(ctx.changes[2].from == nil)
                assert(ctx.changes[2].to[2] == "b")
                return { follow_up = { "reload waybar" } }
            end"#,
        );
        let read = plugin
            .settings_read(SettingsReadContext {
                keys: vec!["appearance.blur".into(), "appearance.size".into()],
            })
            .await
            .unwrap();
        assert_eq!(read.values["appearance.blur"], json!(false));
        assert_eq!(read.values["appearance.size"], json!(2));

        let write = plugin
            .settings_write(SettingsWriteContext {
                changes: vec![
                    SettingChange {
                        key: "appearance.blur".into(),
                        from: Some(json!(false)),
                        to: json!(true),
                    },
                    SettingChange {
                        key: "monitors".into(),
                        from: None,
                        to: json!(["a", "b"]),
                    },
                ],
            })
            .await
            .unwrap();
        assert_eq!(write.follow_up, vec!["reload waybar".to_string()]);
    }

    #[tokio::test]
    async fn write_may_return_nothing() {
        let (_tmp, plugin) = plugin(
            "function PLUGIN:SettingsRead(ctx) return {} end",
            "function PLUGIN:SettingsWrite(ctx) end",
        );
        let read = plugin
            .settings_read(SettingsReadContext { keys: vec![] })
            .await
            .unwrap();
        assert!(read.values.is_empty());
        let write = plugin
            .settings_write(SettingsWriteContext { changes: vec![] })
            .await
            .unwrap();
        assert!(write.follow_up.is_empty());
    }
}
