use eyre::Result;
use toml_edit::DocumentMut;

use crate::config::settings::SettingsFile;
use crate::{env, file};

/// Clears a setting
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsUnset {
    /// The setting to remove
    pub setting: String,
}

impl SettingsUnset {
    pub fn run(self) -> Result<()> {
        let path = env::MISE_CONFIG_DIR.join("config.toml");
        let raw = file::read_to_string(&path)?;
        let mut config: DocumentMut = raw.parse()?;
        if !config.contains_key("settings") {
            return Ok(());
        }
        let settings = config["settings"].as_table_mut().unwrap();
        settings.remove(&self.setting);

        // validate
        let _: SettingsFile = toml::from_str(&config.to_string())?;

        file::write(&path, config.to_string())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings unset legacy_version_file</bold>
"#
);
