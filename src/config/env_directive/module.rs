use crate::Result;
use crate::config::env_directive::EnvResults;
use crate::dirs;
use crate::plugins::vfox_plugin::VfoxPlugin;
use heck::ToKebabCase;
use std::path::PathBuf;
use toml::Value;

impl EnvResults {
    pub async fn module(
        r: &mut EnvResults,
        source: PathBuf,
        name: String,
        value: &Value,
        redact: bool,
    ) -> Result<()> {
        let path = dirs::PLUGINS.join(name.to_kebab_case());
        let plugin = VfoxPlugin::new(name, path);
        if let Some(env) = plugin.mise_env(value).await? {
            for (k, v) in env {
                if redact {
                    r.redactions.push(k.clone());
                }
                r.env.insert(k, (v, source.clone()));
            }
        }
        if let Some(path) = plugin.mise_path(value).await? {
            for p in path {
                r.env_paths.push(p.into());
            }
        }
        Ok(())
    }
}
