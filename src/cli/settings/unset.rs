use eyre::{Result, eyre};
use toml_edit::DocumentMut;

use crate::config::settings::SettingsFile;
use crate::{config, file};

/// Remove a setting from the global or project config
///
/// Edits ~/.config/mise/config.toml, or the nearest project config with
/// `--local`. The setting then falls back to its default or to a value from
/// another config file.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_aliases = ["rm", "remove", "delete", "del"],
    example(r###"mise settings unset jobs"###, help = "Remove jobs from the global config"),
    verbatim_doc_comment
)]
pub(super) struct SettingsUnset {
    /// The setting to remove
    pub key: String,

    /// Write to the nearest project config instead of the global config
    ///
    /// The nearest project config is the lowest-precedence TOML file in the
    /// nearest directory that has one, or ./mise.toml.
    #[usage(long, short)]
    pub local: bool,
}

impl SettingsUnset {
    pub(super) fn run(self) -> Result<()> {
        unset(&self.key, self.local)
    }
}

pub(super) fn unset(key: &str, local: bool) -> Result<()> {
    let paths = if local {
        vec![config::local_toml_config_path()]
    } else if key == "history.sync" && crate::env::MISE_GLOBAL_CONFIG_FILE.is_none() {
        vec![
            config::global_config_path(),
            crate::cli::dotfiles::track::declaration_file(true)?,
        ]
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
    } else {
        vec![config::global_config_path()]
    };
    // Parse and validate every affected layer before changing any of them.
    let updates = paths
        .iter()
        .map(|path| remove_from_file(key, path).map(|contents| (path, contents)))
        .collect::<Result<Vec<_>>>()?;
    for (path, contents) in updates {
        if let Some(contents) = contents {
            file::write(path, contents)?;
        }
    }
    Ok(())
}

fn remove_from_file(mut key: &str, path: &std::path::Path) -> Result<Option<String>> {
    key = super::canonical_setting(key);
    let raw = file::read_to_string(path)?;
    let mut config: DocumentMut = raw.parse()?;
    if let Some(settings) = config["settings"].as_table_like_mut() {
        let removed_legacy = super::remove_legacy_pypi_setting(settings, key);
        if removed_legacy && let Some((parent, leaf)) = key.split_once('.') {
            if let Some(preferred) = settings
                .get_mut(parent)
                .and_then(toml_edit::Item::as_table_like_mut)
            {
                preferred.remove(leaf);
            }
            let _: SettingsFile = toml::from_str(&config.to_string())?;
            return Ok(Some(config.to_string()));
        }

        let settings: &mut dyn toml_edit::TableLike =
            if let Some((parent_key, child_key)) = key.split_once('.') {
                key = child_key;
                let Some(parent) = settings.get_mut(parent_key) else {
                    return Ok(None);
                };
                parent
                    .as_table_like_mut()
                    .ok_or_else(|| eyre!("Setting [{parent_key}] is not a table"))?
            } else {
                settings
            };
        if settings.remove(key).is_none() {
            return Ok(None);
        }
        // validate
        let _: SettingsFile = toml::from_str(&config.to_string())?;

        return Ok(Some(config.to_string()));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_setting_does_not_rewrite_unrelated_layers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for contents in ["[settings]\njobs=2\n", "[settings.history]\nnotify=false\n"] {
            std::fs::write(&path, contents).unwrap();
            assert!(remove_from_file("history.sync", &path).unwrap().is_none());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
        }
    }
}
