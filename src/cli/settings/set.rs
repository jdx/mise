use eyre::{Result, bail, eyre};
use toml_edit::DocumentMut;

use crate::config::settings::{SETTINGS_META, SettingsFile, SettingsType};
use crate::{config, duration, file};

/// Add/update a setting
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["create"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsSet {
    /// The setting to set
    #[clap()]
    pub setting: String,
    /// The value to set
    pub value: String,
    /// Use the local config file instead of the global one
    #[clap(long, short)]
    pub local: bool,
}

impl SettingsSet {
    pub fn run(self) -> Result<()> {
        set(&self.setting, &self.value, false, self.local)
    }
}

pub fn set(mut key: &str, value: &str, add: bool, local: bool) -> Result<()> {
    let raw = value;

    let toml_value = if let Some(meta) = SETTINGS_META.get(key) {
        match meta.type_ {
            SettingsType::Bool => parse_bool(raw)?,
            SettingsType::Integer => parse_i64(raw)?,
            SettingsType::Duration => parse_duration(raw)?,
            SettingsType::Url | SettingsType::Path | SettingsType::String => raw.into(),
            SettingsType::ListString => parse_list_by_comma(raw)?,
            SettingsType::ListPath => parse_list_by_colon(raw)?,
        }
    } else {
        bail!("Unknown setting: {}", key);
    };

    let path = if local {
        config::local_toml_config_path()
    } else {
        config::global_config_path()
    };
    file::create_dir_all(path.parent().unwrap())?;

    let raw_toml = file::read_to_string(&path).unwrap_or_default();
    let mut document: DocumentMut = raw_toml.parse()?;

    if !document.contains_key("settings") {
        document["settings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    if let Some(mut settings) = document["settings"].as_table_mut() {
        if let Some((parent, child)) = key.split_once('.') {
            key = child;
            settings = settings
                .entry(parent)
                .or_insert({
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    toml_edit::Item::Table(t)
                })
                .as_table_mut()
                .unwrap();
        }

        let new_item: toml_edit::Value = if add {
            if let Some(existing_arr) = settings.get(key).and_then(|it| it.as_array()).cloned() {
                let mut arr = existing_arr;

                // only push `raw` if not already in the list
                if !arr.iter().any(|item| item.as_str() == Some(raw)) {
                    arr.push::<V>(raw.into())
                }
                toml_edit::Value::Array(arr)
            } else {
                toml_value.clone()
            }
        } else {
            toml_value.clone()
        };

        settings.insert(key, new_item.into());

        // validate
        let _: SettingsFile = toml::from_str(&document.to_string())?;

        file::write(&path, document.to_string())?;
    }

    Ok(())
}

fn parse_list_by_comma(value: &str) -> Result<toml_edit::Value> {
    if value.is_empty() || value == "[]" {
        return Ok(toml_edit::Array::new().into());
    }
    Ok(value.split(',').map(|s| s.trim().to_string()).collect())
}

fn parse_list_by_colon(value: &str) -> Result<toml_edit::Value> {
    if value.is_empty() || value == "[]" {
        return Ok(toml_edit::Array::new().into());
    }
    Ok(value.split(':').map(|s| s.trim().to_string()).collect())
}

fn parse_bool(value: &str) -> Result<toml_edit::Value> {
    match value.to_lowercase().as_str() {
        "1" | "true" | "yes" | "y" => Ok(true.into()),
        "0" | "false" | "no" | "n" => Ok(false.into()),
        _ => Err(eyre!("{} must be true or false", value)),
    }
}

fn parse_i64(value: &str) -> Result<toml_edit::Value> {
    match value.parse::<i64>() {
        Ok(value) => Ok(value.into()),
        Err(_) => Err(eyre!("{} must be a number", value)),
    }
}

fn parse_duration(value: &str) -> Result<toml_edit::Value> {
    duration::parse_duration(value)?;
    Ok(value.into())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings idiomatic_version_file=true</bold>
"#
);
