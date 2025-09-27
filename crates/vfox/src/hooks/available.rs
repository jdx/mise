use mlua::prelude::LuaError;
use mlua::{FromLua, Lua, Value};

use crate::error::Result;
use crate::Plugin;

impl Plugin {
    #[allow(clippy::needless_return)] // seems to be a clippy bug
    #[tokio::main(flavor = "current_thread")]
    pub async fn available(&self) -> Result<Vec<AvailableVersion>> {
        self.available_async().await
    }

    pub async fn available_async(&self) -> Result<Vec<AvailableVersion>> {
        debug!("[vfox:{}] available_async", &self.name);
        let ctx = self.context(None)?;
        let available = self
            .eval_async(chunk! {
                require "hooks/available"
                return PLUGIN:Available($ctx)
            })
            .await?;

        Ok(available)
    }
}

#[derive(Debug)]
pub struct AvailableVersion {
    pub version: String,
    pub note: Option<String>,
    // pub addition: Option<Table>,
}

impl FromLua for AvailableVersion {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => {
                // TODO: try to default this to an empty table or something
                // let addition = table.get::<Option<Table>>("addition")?;
                Ok(AvailableVersion {
                    version: table.get::<String>("version")?,
                    note: table.get::<Option<String>>("note")?,
                    // addition,
                })
            }
            _ => panic!("Expected table"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Plugin;

    #[test]
    fn dummy() {
        let versions = run("dummy");
        assert_debug_snapshot!(versions, @r###"
        [
            "1.0.0",
            "1.0.1",
        ]
        "###);
    }

    #[tokio::test]
    async fn dummy_async() {
        let versions = run_async("dummy").await;
        assert_debug_snapshot!(versions, @r###"
        [
            "1.0.0",
            "1.0.1",
        ]
        "###);
    }

    #[tokio::test]
    async fn test_nodejs_async() {
        let versions = run_async("test-nodejs").await;
        assert!(versions.contains(&"20.0.0".to_string()));
    }

    fn run(plugin: &str) -> Vec<String> {
        let p = Plugin::test(plugin);
        let r = p.available().unwrap();
        r.iter().map(|v| v.version.clone()).collect()
    }

    async fn run_async(plugin: &str) -> Vec<String> {
        let p = Plugin::test(plugin);
        let r = p.available_async().await.unwrap();
        r.iter().map(|v| v.version.clone()).collect()
    }
}
