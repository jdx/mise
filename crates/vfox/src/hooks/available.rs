use mlua::prelude::LuaError;
use mlua::{FromLua, Lua, Value};

use crate::Plugin;
use crate::error::Result;

impl Plugin {
    #[tokio::main(flavor = "current_thread")]
    pub async fn available(&self) -> Result<Vec<AvailableVersion>> {
        self.available_async().await
    }

    pub async fn available_async(&self) -> Result<Vec<AvailableVersion>> {
        debug!("[vfox:{}] available_async", self.name);
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
    /// If true, this version is a rolling release (like "nightly") that should
    /// always be considered potentially outdated for `mise up` purposes
    pub rolling: bool,
    /// Checksum of the release asset, used to detect changes in rolling releases
    pub checksum: Option<String>,
}

impl FromLua for AvailableVersion {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => {
                let rolling = table.get::<Option<bool>>("rolling")?.unwrap_or(false);
                let checksum = table.get::<Option<String>>("checksum")?;
                Ok(AvailableVersion {
                    version: table.get::<String>("version")?,
                    note: table.get::<Option<String>>("note")?,
                    rolling,
                    checksum,
                })
            }
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "AvailableVersion".to_string(),
                message: Some("Expected table".to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Plugin;
    use crate::embedded_plugins;
    use crate::hooks::available::AvailableVersion;
    use mlua::{FromLua, Lua, Value};
    use std::sync::Arc;
    use url::Url;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_available_version_rejects_non_table() {
        let lua = Lua::new();
        let result = AvailableVersion::from_lua(Value::Boolean(true), &lua);
        assert!(result.is_err(), "a non-table response must not panic");
    }

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

    #[tokio::test]
    async fn chromedriver_returns_newest_version_first() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/versions.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "versions": [
                    {"version": "115.0.5763.0", "downloads": {"chromedriver": [{}]}},
                    {"version": "152.0.7999.0"},
                    {"version": "153.0.8009.0", "downloads": {"chromedriver": [{}]}}
                ]
            })))
            .mount(&server)
            .await;

        let embedded = embedded_plugins::get_embedded_plugin("chromedriver").unwrap();
        let plugin = Plugin::from_embedded("chromedriver", embedded).unwrap();
        let mock_url = Url::parse(&format!("{}/versions.json", server.uri())).unwrap();
        plugin
            .set_url_rewriter(Arc::new(move |url| *url = mock_url.clone()))
            .unwrap();

        let versions = plugin.available_async().await.unwrap();
        assert_eq!(
            versions
                .into_iter()
                .map(|version| version.version)
                .collect::<Vec<_>>(),
            ["153.0.8009.0", "115.0.5763.0"]
        );
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
