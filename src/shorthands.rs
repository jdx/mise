use std::collections::HashMap;
use std::path::PathBuf;

use eyre::Result;
use itertools::Itertools;
use toml::Table;

use crate::config::Settings;
use crate::registry::REGISTRY;
use crate::{dirs, file};

pub(crate) type Shorthands = HashMap<String, Vec<String>>;

/// The asdf and vfox backends of every registry tool that has one. Walking the
/// whole registry through `backends()` would check every tool for a
/// MISE_BACKENDS_* override and filter every backend it lists, so unless an
/// override may apply this asks each tool for its plugin backends directly.
fn registry_shorthands(has_backend_overrides: bool) -> Shorthands {
    let is_plugin_backend = |full: &str| full.starts_with("asdf:") || full.starts_with("vfox:");
    REGISTRY
        .iter()
        .map(|(id, rt)| {
            let fulls = if has_backend_overrides {
                rt.backends()
                    .into_iter()
                    .filter(|f| is_plugin_backend(f))
                    .collect_vec()
            } else {
                rt.registry_backends_where(is_plugin_backend)
            };
            (
                id.to_string(),
                fulls.into_iter().map(|f| f.to_string()).collect_vec(),
            )
        })
        .filter(|(_, fulls)| !fulls.is_empty())
        .collect()
}

pub(crate) fn get_shorthands(settings: &Settings) -> Shorthands {
    let mut shorthands = HashMap::new();
    if !settings.disable_default_registry {
        shorthands.extend(registry_shorthands(crate::registry::has_backend_overrides()));
    };
    if let Some(f) = &settings.shorthands_file {
        match parse_shorthands_file(f.clone()) {
            Ok(custom) => {
                shorthands.extend(custom);
            }
            Err(err) => {
                warn!("Failed to read shorthands file: {} {:#}", &f.display(), err);
            }
        }
    }
    shorthands
}

fn parse_shorthands_file(mut f: PathBuf) -> Result<Shorthands> {
    if f.starts_with("~") {
        f = dirs::HOME.join(f.strip_prefix("~")?);
    }
    let raw = file::read_to_string(&f)?;
    let toml = raw.parse::<Table>()?;

    let mut shorthands = HashMap::new();
    for (k, v) in toml {
        if let Some(v) = v.as_str() {
            shorthands.insert(k, vec![v.to_string()]);
        }
    }
    Ok(shorthands)
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    #[cfg(unix)]
    use pretty_assertions::assert_str_eq;

    use crate::config::{Config, SettingsExt};

    use super::*;

    #[tokio::test]
    #[cfg(unix)]
    async fn test_get_shorthands() {
        use crate::config::Config;

        let _settings = crate::test::SettingsGuard::lock();
        let _config = Config::get().await.unwrap();
        Settings::reset(None);
        let mut settings = Settings::get().deref().clone();
        settings.shorthands_file = Some("../fixtures/shorthands.toml".into());
        let shorthands = get_shorthands(&settings);
        assert_str_eq!(shorthands["aapt2"][0], "vfox:mise-plugins/vfox-aapt2");
        assert_str_eq!(shorthands["scala"][0], "vfox:mise-plugins/vfox-scala");
        assert_str_eq!(shorthands["groovy"][0], "vfox:jdx/vfox-groovy");
        assert_str_eq!(shorthands["mongodb"][0], "vfox:jdx/vfox-mongod");
        assert_str_eq!(shorthands["emsdk"][0], "vfox:jdx/vfox-emsdk");
        assert_str_eq!(
            shorthands["teleport-community"][0],
            "vfox:jdx/vfox-teleport-community"
        );
        assert_str_eq!(shorthands["tinytex"][0], "vfox:jdx/vfox-tinytex");
        assert_str_eq!(shorthands["node"][0], "https://node");
        assert_str_eq!(shorthands["xxxxxx"][0], "https://xxxxxx");
    }

    #[tokio::test]
    async fn test_registry_shorthands_matches_every_registry_tool() {
        let _settings = crate::test::SettingsGuard::lock();
        let _config = Config::get().await.unwrap();
        Settings::reset(None);
        // Every registry tool's allowed backends, filtered to plugin backends
        // afterwards: what `backends()` returns when no override is set. The
        // shortcut must agree without depending on this process's overrides.
        let expected: Shorthands = REGISTRY
            .iter()
            .map(|(id, rt)| {
                (
                    id.to_string(),
                    rt.registry_backends_where(|_| true)
                        .iter()
                        .filter(|f| f.starts_with("asdf:") || f.starts_with("vfox:"))
                        .map(|f| f.to_string())
                        .collect_vec(),
                )
            })
            .filter(|(_, fulls)| !fulls.is_empty())
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(registry_shorthands(false), expected);
    }

    #[tokio::test]
    async fn test_get_shorthands_missing_file() {
        let _settings = crate::test::SettingsGuard::lock();
        let _config = Config::get().await.unwrap();
        Settings::reset(None);
        let mut settings = Settings::get().deref().clone();
        settings.shorthands_file = Some("test/fixtures/missing.toml".into());
        let shorthands = get_shorthands(&settings);
        assert!(!shorthands.is_empty());
    }
}
