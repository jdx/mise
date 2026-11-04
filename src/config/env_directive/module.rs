use crate::Result;
use crate::config::env_directive::{EnvDirective, EnvResults};
use crate::config::{Config, Settings, SettingsExt};
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
    pub(crate) async fn module(
        r: &mut EnvResults,
        config: &Arc<Config>,
        source: PathBuf,
        name: String,
        value: &Value,
        redact: Option<bool>,
        env: IndexMap<String, String>,
    ) -> Result<()> {
        let config_root = crate::config::config_file::config_root::config_root(&source);
        let (plugin, path) = env_plugin(&name);
        if let Some(err) = plugin.missing_source(config) {
            // Config loading resolves [env], so failing here would break every
            // command, including the `mise plugins install` that fixes it.
            // Only a plugin with no source at all is skipped: a known source
            // that fails to install (network, 404, paranoid, safe mode) still
            // errors, so a command never runs without an env it expects.
            debug!("env plugin {name}: {err:#}");
            warn_once!(
                "skipping env plugin {name}: it is not installed and has no source \
                 (a registry name, owner/repo, or URL)\n\
                 Install it with `mise plugins install {name} <git-url>`, \
                 or set its URL in [plugins]"
            );
            r.has_uncacheable = true;
            return Ok(());
        }
        plugin
            .ensure_installed(config, &MultiProgressReport::get(), false, false)
            .await?;
        if let Some(response) = plugin.mise_env(value, &env, Some(&config_root)).await? {
            // Track cacheability
            if !response.cacheable {
                r.has_uncacheable = true;
            }

            // Add plugin directory to watch files for cache invalidation
            // This ensures cache invalidates when plugin is updated
            r.watch_files.push(path);

            // Add watch files for cache invalidation
            // Absolutize relative paths relative to config_root for consistent cache validation
            for watch_file in response.watch_files {
                if watch_file.is_absolute() {
                    r.watch_files.push(watch_file);
                } else {
                    r.watch_files.push(config_root.join(watch_file));
                }
            }

            // Add env vars
            // User's explicit redact setting takes priority, otherwise use plugin's preference
            let should_redact = redact.unwrap_or(response.redact);
            for (k, v) in response.env {
                r.track_redaction_override(&k, redact);
                if should_redact {
                    r.redactions.push(k.clone());
                }
                r.env.insert(k, (v, source.clone()));
            }
        }
        if let Some(path) = plugin.mise_path(value, &env, Some(&config_root)).await? {
            for p in path {
                r.env_paths.push(p.into());
            }
        }
        Ok(())
    }
}

fn env_plugin(name: &str) -> (VfoxPlugin, PathBuf) {
    let path = dirs::PLUGINS.join(name.to_kebab_case());
    (VfoxPlugin::new(name.to_string(), path.clone()), path)
}

/// `[env] _.<name>` modules that are skipped because their plugin is not
/// installed and has no source, with the config file that declares each.
/// Modules that safe mode ignores are left out: they are never loaded.
pub fn skipped_env_modules(config: &Config) -> Vec<(String, PathBuf)> {
    config
        .config_files
        .iter()
        .flat_map(|(source, cf)| {
            cf.env_entries()
                .unwrap_or_default()
                .into_iter()
                .filter(|directive| {
                    !(Settings::safe_mode() && super::dropped_in_safe_mode(directive, source))
                })
                .filter_map(move |directive| match directive {
                    EnvDirective::Module(name, ..) => Some((name, source.clone())),
                    _ => None,
                })
        })
        .filter(|(name, _)| env_plugin(name).0.missing_source(config).is_some())
        .collect()
}
