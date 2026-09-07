use eyre::{Result, eyre};
use toml_edit::DocumentMut;

use crate::config::settings::SettingsFile;
use crate::{config, file};

/// Clear a setting
///
/// This modifies ~/.config/mise/config.toml by default, or the local config with `--local`.
#[derive(Debug, usage_rs::Args)]
#[usage(visible_aliases = ["rm", "remove", "delete", "del"], example(r###"mise settings unset jobs"###), verbatim_doc_comment)]
pub(super) struct SettingsUnset {
    /// The setting to remove
    pub key: String,

    /// Use the local config file instead of the global one
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
    let raw = file::read_to_string(path)?;
    let mut config: DocumentMut = raw.parse()?;
    if let Some(settings) = config["settings"].as_table_like_mut() {
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
