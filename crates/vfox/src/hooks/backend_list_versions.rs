use std::collections::BTreeMap;

use indexmap::IndexMap;

use crate::{Plugin, error::Result};
use mlua::{FromLua, IntoLua, Lua, LuaSerdeExt, Value, prelude::LuaError};

#[derive(Debug, Clone)]
pub struct BackendListVersionsContext {
    pub tool: String,
    pub options: IndexMap<String, toml::Value>,
}

#[derive(Debug, Clone)]
pub struct BackendListVersionsResponse {
    pub versions: Vec<BackendVersion>,
    /// Moving names the registry publishes, such as `recommended`, mapped to
    /// the version they currently point at
    pub aliases: BTreeMap<String, String>,
}

/// One entry of the `versions` list. A plugin returns either a plain version
/// string or a table with `version` and an optional `created_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendVersion {
    pub version: String,
    /// RFC 3339 release date, used by `minimum_release_age` and `--before`
    pub created_at: Option<String>,
}

impl FromLua for BackendVersion {
    fn from_lua(value: Value, lua: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => Ok(BackendVersion {
                version: table.get::<String>("version")?,
                created_at: table.get::<Option<String>>("created_at")?,
            }),
            other => Ok(BackendVersion {
                version: String::from_lua(other, lua)?,
                created_at: None,
            }),
        }
    }
}

impl Plugin {
    pub async fn backend_list_versions(
        &self,
        ctx: BackendListVersionsContext,
    ) -> Result<BackendListVersionsResponse> {
        debug!("[vfox:{}] backend_list_versions", self.name);
        self.eval_async(chunk! {
            require "hooks/backend_list_versions"
            return PLUGIN:BackendListVersions($ctx)
        })
        .await
    }
}

impl IntoLua for BackendListVersionsContext {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("tool", self.tool)?;
        table.set("options", lua.to_value(&self.options)?)?;
        Ok(Value::Table(table))
    }
}

impl FromLua for BackendListVersionsResponse {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => Ok(BackendListVersionsResponse {
                versions: table.get::<Vec<BackendVersion>>("versions")?,
                aliases: table
                    .get::<Option<BTreeMap<String, String>>>("aliases")?
                    .unwrap_or_default(),
            }),
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "BackendListVersionsResponse".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use mlua::{IntoLua, Lua};

    use super::*;

    fn eval_response(lua_src: &str) -> BackendListVersionsResponse {
        let lua = Lua::new();
        let value: Value = lua.load(lua_src).eval().unwrap();
        BackendListVersionsResponse::from_lua(value, &lua).unwrap()
    }

    #[test]
    fn test_response_accepts_plain_strings() {
        let r = eval_response(r#"return { versions = { "1.0.0", "1.1.0" } }"#);
        assert_eq!(
            r.versions,
            vec![
                BackendVersion {
                    version: "1.0.0".into(),
                    created_at: None
                },
                BackendVersion {
                    version: "1.1.0".into(),
                    created_at: None
                },
            ]
        );
    }

    #[test]
    fn test_response_aliases_are_optional() {
        let r = eval_response(r#"return { versions = { "1.0.0" } }"#);
        assert!(r.aliases.is_empty());
        let r = eval_response(
            r#"return { versions = { "1.0.0" }, aliases = { recommended = "1.0.0" } }"#,
        );
        assert_eq!(r.aliases["recommended"], "1.0.0");
    }

    #[test]
    fn test_response_accepts_tables_and_mixed_entries() {
        let r = eval_response(
            r#"return { versions = {
                { version = "1.0.0", created_at = "2026-01-31T12:00:00Z" },
                { version = "1.1.0" },
                "1.2.0",
            } }"#,
        );
        assert_eq!(
            r.versions[0].created_at.as_deref(),
            Some("2026-01-31T12:00:00Z")
        );
        assert_eq!(r.versions[1].created_at, None);
        assert_eq!(r.versions[2].version, "1.2.0");
    }

    #[test]
    fn test_context_construction_and_clone() {
        let mut options = IndexMap::new();
        options.insert(
            "key1".to_string(),
            toml::Value::String("value1".to_string()),
        );
        options.insert(
            "key2".to_string(),
            toml::Value::String("value2".to_string()),
        );

        let ctx = BackendListVersionsContext {
            tool: "test-tool".to_string(),
            options: options.clone(),
        };

        assert_eq!(ctx.tool, "test-tool");
        assert_eq!(ctx.options.len(), 2);
        assert_eq!(
            ctx.options["key1"],
            toml::Value::String("value1".to_string())
        );
        assert_eq!(
            ctx.options["key2"],
            toml::Value::String("value2".to_string())
        );

        let cloned = ctx.clone();
        assert_eq!(cloned.tool, "test-tool");
        assert_eq!(cloned.options, options);
    }

    #[test]
    fn test_context_empty_options() {
        let ctx = BackendListVersionsContext {
            tool: "my-tool".to_string(),
            options: IndexMap::new(),
        };

        assert_eq!(ctx.tool, "my-tool");
        assert!(ctx.options.is_empty());
    }

    #[test]
    fn test_into_lua_serialization() {
        let lua = Lua::new();
        let mut options = IndexMap::new();
        options.insert(
            "arch".to_string(),
            toml::Value::String("x86_64".to_string()),
        );
        options.insert("os".to_string(), toml::Value::String("linux".to_string()));

        let ctx = BackendListVersionsContext {
            tool: "test-tool".to_string(),
            options,
        };

        let value = ctx.into_lua(&lua).unwrap();
        let table = value.as_table().unwrap();

        assert_eq!(table.get::<String>("tool").unwrap(), "test-tool");

        let opts_table = table.get::<mlua::Table>("options").unwrap();
        assert_eq!(opts_table.get::<String>("arch").unwrap(), "x86_64");
        assert_eq!(opts_table.get::<String>("os").unwrap(), "linux");
    }

    #[test]
    fn test_into_lua_empty_options() {
        let lua = Lua::new();
        let ctx = BackendListVersionsContext {
            tool: "empty-opts".to_string(),
            options: IndexMap::new(),
        };

        let value = ctx.into_lua(&lua).unwrap();
        let table = value.as_table().unwrap();

        assert_eq!(table.get::<String>("tool").unwrap(), "empty-opts");

        let opts_table = table.get::<mlua::Table>("options").unwrap();
        assert_eq!(opts_table.raw_len(), 0);
    }

    #[test]
    fn test_into_lua_array_options() {
        let lua = Lua::new();
        let mut options = IndexMap::new();
        options.insert(
            "channels".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("robostack-humble".to_string()),
                toml::Value::String("conda-forge".to_string()),
            ]),
        );
        options.insert(
            "name".to_string(),
            toml::Value::String("my-tool".to_string()),
        );

        let ctx = BackendListVersionsContext {
            tool: "test-tool".to_string(),
            options,
        };

        let value = ctx.into_lua(&lua).unwrap();
        let table = value.as_table().unwrap();

        assert_eq!(table.get::<String>("tool").unwrap(), "test-tool");

        let opts_table = table.get::<mlua::Table>("options").unwrap();

        // Array should be a proper Lua sequence table, not a flat string
        let channels = opts_table.get::<mlua::Table>("channels").unwrap();
        assert_eq!(channels.get::<String>(1).unwrap(), "robostack-humble");
        assert_eq!(channels.get::<String>(2).unwrap(), "conda-forge");
        assert_eq!(channels.raw_len(), 2);

        assert_eq!(opts_table.get::<String>("name").unwrap(), "my-tool");
    }
}
