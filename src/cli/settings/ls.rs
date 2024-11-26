use crate::config;
use crate::config::{Settings, SETTINGS};
use eyre::Result;

/// Show current settings
///
/// This is the contents of ~/.config/mise/config.toml
///
/// Note that aliases are also stored in this file
/// but managed separately with `mise aliases`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsLs {
    /// List keys under this key
    pub key: Option<String>,

    /// Use the local config file instead of the global one
    #[clap(long, short)]
    pub local: bool,

    /// Only display key names for each setting
    #[clap(long, verbatim_doc_comment, alias = "keys")]
    pub names: bool,
}

impl SettingsLs {
    pub fn run(self) -> Result<()> {
        let mut settings = if self.local {
            let partial = Settings::parse_settings_file(&config::local_toml_config_path())
                .unwrap_or_default();
            Settings::partial_as_dict(&partial)?
        } else {
            SETTINGS.as_dict()?
        };
        if let Some(key) = &self.key {
            settings = settings.remove(key).unwrap().try_into()?
        }
        for k in Settings::hidden_configs() {
            settings.remove(*k);
        }
        for (k, v) in settings.clone().iter() {
            if v.as_table().is_some_and(|t| t.is_empty()) {
                settings.remove(k);
            }
        }
        if self.names {
            return self.print_names(&settings);
        }
        miseprintln!("{}", settings);
        Ok(())
    }

    fn print_names(&self, settings: &toml::Table) -> Result<()> {
        for (k, v) in settings {
            miseprintln!("{k}");
            if let toml::Value::Table(t) = v {
                for (subkey, _) in t {
                    miseprintln!("{k}.{subkey}");
                }
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings ls</bold>
    idiomatic_version_file = false
    ...

    $ <bold>mise settings ls python</bold>
    default_packages_file = "~/.default-python-packages"
    ...
"#
);
