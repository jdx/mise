use indexmap::IndexMap;
use mlua::{LuaSerdeExt, UserData, UserDataFields};

#[derive(Debug)]
pub(crate) struct Context {
    pub args: Vec<String>,
    pub(crate) version: Option<String>,
    pub(crate) options: IndexMap<String, toml::Value>,
    // pub(crate) runtime_version: String,
}

impl UserData for Context {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("args", |_, t| Ok(t.args.clone()));
        fields.add_field_method_get("version", |_, t| Ok(t.version.clone()));
        fields.add_field_method_get("options", |lua, t| lua.to_value(&t.options));
        // fields.add_field_method_get("runtimeVersion", |_, t| Ok(t.runtime_version.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_options_to_lua() {
        let lua = mlua::Lua::new();
        let options = IndexMap::from([
            ("enabled".to_string(), toml::Value::Boolean(true)),
            ("retries".to_string(), toml::Value::Integer(3)),
            (
                "channels".to_string(),
                toml::Value::Array(vec![
                    toml::Value::String("stable".to_string()),
                    toml::Value::String("beta".to_string()),
                ]),
            ),
        ]);
        lua.globals()
            .set(
                "ctx",
                Context {
                    args: vec![],
                    version: Some("1.0.0".to_string()),
                    options,
                },
            )
            .unwrap();

        lua.load(
            r#"
            assert(ctx.options.enabled == true)
            assert(ctx.options.retries == 3)
            assert(ctx.options.channels[1] == "stable")
            assert(ctx.options.channels[2] == "beta")
            "#,
        )
        .exec()
        .unwrap();
    }
}
