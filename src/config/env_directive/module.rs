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
        if let Some(response) = plugin.mise_env(value).await? {
            // Track cacheability
            if !response.cacheable {
                r.has_uncacheable = true;
            }

            // Add watch files for cache invalidation
            r.watch_files.extend(response.watch_files);

            // Add env vars
            for (k, v) in response.env {
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
