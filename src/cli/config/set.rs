use crate::cli::config::top_toml_config;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::settings::{SettingsType, SETTINGS_META};
use clap::ValueEnum;
use eyre::bail;
use std::path::PathBuf;

/// Set the value of a setting in a mise.toml file
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct ConfigSet {
    /// The path of the config to display
    pub key: String,

    /// The value to set the key to
    pub value: String,

    /// The path to the mise.toml file to edit
    ///
    /// If not provided, the nearest mise.toml file will be used
    #[clap(short, long)]
    pub file: Option<PathBuf>,

    #[clap(value_enum, short, long, default_value_t)]
    pub type_: TomlValueTypes,
}

#[derive(ValueEnum, Default, Clone, Debug)]
pub enum TomlValueTypes {
    #[default]
    Infer,
    #[value()]
    String,
    #[value()]
    Integer,
    #[value()]
    Float,
    #[value()]
    Bool,
    #[value()]
    List,
}

impl ConfigSet {
    pub fn run(self) -> eyre::Result<()> {
        let mut file = self.file;
        if file.is_none() {
            file = top_toml_config();
        }
        if let Some(file) = file {
            let mut config: toml_edit::DocumentMut = std::fs::read_to_string(&file)?.parse()?;
            let mut container = config.as_item_mut();
            let parts = self.key.split('.').collect::<Vec<&str>>();
            for key in parts.iter().take(parts.len() - 1) {
                container = container
                    .as_table_mut()
                    .unwrap()
                    .entry(key)
                    .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
            }
            let last_key = parts.last().unwrap();

            let type_to_use = match self.type_ {
                TomlValueTypes::Infer => {
                    let expected_type = if !self.key.starts_with("settings.") {
                        None
                    } else {
                        SETTINGS_META.get(&(*last_key).to_string())
                    };
                    match expected_type {
                        Some(meta) => match meta.type_ {
                            SettingsType::Bool => TomlValueTypes::Bool,
                            SettingsType::String => TomlValueTypes::String,
                            SettingsType::Integer => TomlValueTypes::Integer,
                            SettingsType::Duration => TomlValueTypes::String,
                            SettingsType::Path => TomlValueTypes::String,
                            SettingsType::Url => TomlValueTypes::String,
                            SettingsType::ListString => TomlValueTypes::List,
                            SettingsType::ListPath => TomlValueTypes::List,
                        },
                        None => TomlValueTypes::String,
                    }
                }
                _ => self.type_,
            };

            let value = match type_to_use {
                TomlValueTypes::String => toml_edit::value(self.value),
                TomlValueTypes::Integer => toml_edit::value(self.value.parse::<i64>()?),
                TomlValueTypes::Float => toml_edit::value(self.value.parse::<f64>()?),
                TomlValueTypes::Bool => toml_edit::value(self.value.parse::<bool>()?),
                TomlValueTypes::List => {
                    let mut list = toml_edit::Array::new();
                    for item in self.value.split(',').map(|s| s.trim()) {
                        list.push(item);
                    }
                    toml_edit::Item::Value(toml_edit::Value::Array(list))
                }
                TomlValueTypes::Infer => bail!("Type not found"),
            };

            container.as_table_mut().unwrap().insert(last_key, value);

            let raw = config.to_string();
            MiseToml::from_str(&raw, &file)?;
            std::fs::write(&file, raw)?;
        } else {
            bail!("No mise.toml file found");
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise config set tools.python 3.12</bold>
    $ <bold>mise config set settings.always_keep_download true</bold>
    $ <bold>mise config set env.TEST_ENV_VAR ABC</bold>
    $ <bold>mise config set settings.disable_tools --type list node,rust</bold>

    # Type for `settings` is inferred
    $ <bold>mise config set settings.jobs 4</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;

    #[test]
    fn test_config_set() {
        reset();
        assert_cli_snapshot!("config", "set", "env.TEST_ENV_VAR", "ABC", @"");
        assert_cli_snapshot!("config", "get", "env.TEST_ENV_VAR", @"ABC");

        assert_cli_snapshot!("config", "set", "settings.ruby.default_packages_file", "abc", @"");
        assert_cli_snapshot!("config", "get", "settings.ruby.default_packages_file", @"abc");

        assert_cli_snapshot!("config", "set", "settings.always_keep_download", "--type", "bool", "true", @"");
        assert_cli_snapshot!("config", "get", "settings.always_keep_download", @"true");

        assert_cli_snapshot!("config", "set", "settings.jobs", "--type", "integer", "4", @"");
        assert_cli_snapshot!("config", "get", "settings.jobs", @"4");

        assert_cli_snapshot!("config", "set", "settings.jobs", "4", @"");
        assert_cli_snapshot!("config", "get", "settings.jobs", @"4");

        assert_cli_snapshot!("config", "set", "settings.disable_tools", "--type", "list", "node,rust", @"");
        assert_cli_snapshot!("config", "get", "settings.disable_tools", @"[\"node\", \"rust\"]");
    }
}
