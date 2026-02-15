use crate::Result;
use crate::config::Config;
use crate::config::env_directive::EnvResults;
use crate::dirs;
use crate::plugins::Plugin;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::ui::multi_progress_report::MultiProgressReport;
use heck::ToKebabCase;
use indexmap::IndexMap;
use std::path::PathBuf;
use std::sync::Arc;
use toml::Value;

impl EnvResults {
    pub async fn module(
        r: &mut EnvResults,
        config: &Arc<Config>,
        source: PathBuf,
        name: String,
        value: &Value,
        redact: Option<bool>,
        env: IndexMap<String, String>,
    ) -> Result<()> {
        let path = dirs::PLUGINS.join(name.to_kebab_case());
        let plugin = VfoxPlugin::new(name, path.clone());
        plugin
            .ensure_installed(config, &MultiProgressReport::get(), false, false)
            .await?;
        if let Some(response) = plugin.mise_env(value, &env).await? {
            // Track cacheability
            if !response.cacheable {
                r.has_uncacheable = true;
            }

            // Add plugin directory to watch files for cache invalidation
            // This ensures cache invalidates when plugin is updated
            r.watch_files.push(path);

            // Add watch files for cache invalidation
            // Absolutize relative paths to ensure consistent cache validation
            // regardless of which directory mise is run from
            let cwd = std::env::current_dir().unwrap_or_default();
            for watch_file in response.watch_files {
                if watch_file.is_absolute() {
                    r.watch_files.push(watch_file);
                } else {
                    r.watch_files.push(cwd.join(watch_file));
                }
            }

            // Add env vars
            // User's explicit redact setting takes priority, otherwise use plugin's preference
            let should_redact = redact.unwrap_or(response.redact);
            for (k, v) in response.env {
                if should_redact {
                    r.redactions.push(k.clone());
                }
                r.env.insert(k, (v, source.clone()));
            }
        }
        if let Some(path) = plugin.mise_path(value, &env).await? {
            for p in path {
                r.env_paths.push(p.into());
            }
        }
        Ok(())
    }
}
