use eyre::Result;
use toml_edit::DocumentMut;

use crate::config::settings::SettingsFile;
use crate::{config, file};

/// Clears a setting
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsUnset {
    /// The setting to remove
    pub key: String,

    /// Use the local config file instead of the global one
    #[clap(long, short)]
    pub local: bool,
}

impl SettingsUnset {
    pub fn run(self) -> Result<()> {
        unset(&self.key, self.local)
    }
}

pub fn unset(mut key: &str, local: bool) -> Result<()> {
    let path = if local {
        config::local_toml_config_path()
    } else {
        config::global_config_path()
    };
    let raw = file::read_to_string(&path)?;
    let mut config: DocumentMut = raw.parse()?;
    if let Some(mut settings) = config["settings"].as_table_mut() {
        if let Some((parent_key, child_key)) = key.split_once('.') {
            key = child_key;
            settings = settings
                .entry(parent_key)
                .or_insert({
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    toml_edit::Item::Table(t)
                })
                .as_table_mut()
                .unwrap();
        }
        settings.remove(key);
        // validate
        let _: SettingsFile = toml::from_str(&config.to_string())?;

        file::write(&path, config.to_string())?;
    }
    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings unset idiomatic_version_file</bold>
"#
);
