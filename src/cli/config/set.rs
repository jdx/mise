use crate::config::config_file::mise_toml::MiseToml;
use crate::config::settings::{SETTINGS_META, SettingsType};
use crate::config::top_toml_config;
use crate::toml::dedup_toml_array;
use clap::ValueEnum;
use eyre::bail;
use std::path::PathBuf;

/// Set the value of a setting in a mise.toml file
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct ConfigSet {
    /// The path of the config to display
    pub key: String,

    /// The value to set the key to (optional if provided as KEY=VALUE)
    pub value: Option<String>,

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
    #[value()]
    Set,
}

impl ConfigSet {
    pub fn run(self) -> eyre::Result<()> {
        let (full_key, value) = match self.value {
            Some(v) => (self.key, v),
            None => {
                let (k, v) = self.key.split_once('=').ok_or_else(|| {
                    eyre::eyre!(
                        "Usage: mise config set <KEY>=<VALUE> or mise config set <KEY> <VALUE>"
                    )
                })?;
                (k.to_string(), v.to_string())
            }
        };
        let mut file = self.file;
        if file.is_none() {
            file = top_toml_config();
        }
        let Some(file) = file else {
            bail!("No mise.toml file found");
        };
        let mut config: toml_edit::DocumentMut = std::fs::read_to_string(&file)?.parse()?;
        let mut container = config.as_item_mut();
        let parts = full_key.split('.').collect::<Vec<&str>>();
        let last_key = parts.last().unwrap();
        for (idx, part) in parts.iter().take(parts.len() - 1).enumerate() {
            container = container
                .as_table_like_mut()
                .unwrap()
                .entry(part)
                .or_insert({
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    toml_edit::Item::Table(t)
                });
            // if the key is a tool with a simple value, we want to convert it to a inline table preserving the version
            let is_simple_tool_version =
                full_key.starts_with("tools.") && idx == 1 && !container.is_table_like();
            if is_simple_tool_version {
                let mut inline_table = toml_edit::InlineTable::new();
                inline_table.insert("version", container.as_value().unwrap().clone());
                *container = toml_edit::Item::Value(toml_edit::Value::InlineTable(inline_table));
            }
        }

        let infer_bool_or_string = |value: &str| match value {
            "true" | "yes" | "1" => TomlValueTypes::Bool,
            "false" | "no" | "0" => TomlValueTypes::Bool,
            _ => TomlValueTypes::String,
        };
        let type_to_use = match self.type_ {
            TomlValueTypes::Infer => {
                let expected_type = if !full_key.starts_with("settings.") {
                    None
                } else {
                    SETTINGS_META.get(*last_key)
                };
                match expected_type {
                    Some(meta) => match meta.type_ {
                        SettingsType::Bool => TomlValueTypes::Bool,
                        SettingsType::BoolOrString => infer_bool_or_string(&value),
                        SettingsType::String => TomlValueTypes::String,
                        SettingsType::Integer => TomlValueTypes::Integer,
                        SettingsType::Duration => TomlValueTypes::String,
                        SettingsType::Path => TomlValueTypes::String,
                        SettingsType::Url => TomlValueTypes::String,
                        SettingsType::ListString => TomlValueTypes::List,
                        SettingsType::ListPath => TomlValueTypes::List,
                        SettingsType::SetString => TomlValueTypes::Set,
                        SettingsType::IndexMap => TomlValueTypes::String,
                    },
                    None => infer_bool_or_string(&value),
                }
            }
            _ => self.type_,
        };

        let value = match type_to_use {
            TomlValueTypes::String => toml_edit::value(value),
            TomlValueTypes::Integer => toml_edit::value(value.parse::<i64>()?),
            TomlValueTypes::Float => toml_edit::value(value.parse::<f64>()?),
            TomlValueTypes::Bool => toml_edit::value(value.parse::<bool>()?),
            TomlValueTypes::List => {
                let mut list = toml_edit::Array::new();
                for item in value.split(',').map(|s| s.trim()) {
                    list.push(item);
                }
                toml_edit::Item::Value(toml_edit::Value::Array(list))
            }
            TomlValueTypes::Set => {
                let set = toml_edit::Array::new();
                toml_edit::Item::Value(toml_edit::Value::Array(dedup_toml_array(&set)))
            }
            TomlValueTypes::Infer => bail!("Type not found"),
        };

        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        let mut table = toml_edit::Item::Table(t);
        container
            .as_table_like_mut()
            .unwrap_or_else(|| table.as_table_like_mut().unwrap())
            .insert(last_key, value);

        let raw = config.to_string();
        MiseToml::from_str(&raw, &file)?;
        std::fs::write(&file, raw)?;
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
