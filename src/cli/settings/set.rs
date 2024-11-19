use eyre::{bail, eyre, Result};
use toml_edit::DocumentMut;

use crate::config::settings::{SettingsFile, SettingsType, SETTINGS_META};
use crate::{env, file};

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
}

impl SettingsSet {
    pub fn run(self) -> Result<()> {
        set(&self.setting, &self.value, false)
    }
}

pub fn set(mut key: &str, value: &str, add: bool) -> Result<()> {
    let value = if let Some(meta) = SETTINGS_META.get(key) {
        match meta.type_ {
            SettingsType::Bool => parse_bool(value)?,
            SettingsType::Integer => parse_i64(value)?,
            SettingsType::Duration => parse_duration(value)?,
            SettingsType::Url | SettingsType::Path | SettingsType::String => value.into(),
            SettingsType::ListString => parse_list_by_comma(value)?,
            SettingsType::ListPath => parse_list_by_colon(value)?,
        }
    } else {
        bail!("Unknown setting: {}", key);
    };

    let path = &*env::MISE_GLOBAL_CONFIG_FILE;
    file::create_dir_all(path.parent().unwrap())?;
    let raw = file::read_to_string(path).unwrap_or_default();
    let mut config: DocumentMut = raw.parse()?;
    if !config.contains_key("settings") {
        config["settings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let mut settings = config["settings"].as_table_mut().unwrap();
    if key.contains(".") {
        let (parent_key, child_key) = key.split_once('.').unwrap();

        key = child_key;
        settings = settings
            .entry(parent_key)
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut()
            .unwrap();
    }

    let value = match settings.get(key).map(|c| c.as_array()) {
        Some(Some(array)) if add => {
            let mut array = array.clone();
            array.extend(value.as_array().unwrap().iter().cloned());
            array.into()
        }
        _ => value,
    };
    settings.insert(key, value.into());

    // validate
    let _: SettingsFile = toml::from_str(&config.to_string())?;

    file::write(path, config.to_string())
}

fn parse_list_by_comma(value: &str) -> Result<toml_edit::Value> {
    Ok(value.split(',').map(|s| s.trim().to_string()).collect())
}

fn parse_list_by_colon(value: &str) -> Result<toml_edit::Value> {
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
    humantime::parse_duration(value)?;
    Ok(value.into())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings set legacy_version_file true</bold>
"#
);
