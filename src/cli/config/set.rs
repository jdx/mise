use crate::config::config_file::mise_toml::MiseToml;
use crate::config::settings::{SETTINGS_META, SettingsType};
use crate::config::{
    ConfigPathOptions, resolve_target_config_path, system_config_path, top_toml_config,
};
use crate::file::display_path;
use crate::toml::dedup_toml_array;
use eyre::bail;
use std::path::PathBuf;

/// Set the value of a setting in a mise.toml file
#[derive(Debug, usage_rs::Args)]
#[usage(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub(super) struct ConfigSet {
    /// The path of the config to display
    pub key: String,

    /// The value to set the key to (optional if provided as KEY=VALUE)
    pub value: Option<String>,

    /// The path to the mise.toml file to edit
    ///
    /// Can be a file path or directory. If a directory is provided, the config file in that directory is used.
    ///
    /// If not provided, the nearest mise.toml file will be used
    #[usage(short, long, visible_alias = "path", value_hint = usage_rs::ValueHint::AnyPath)]
    pub file: Option<PathBuf>,

    /// Edit the global config file.
    #[usage(long, short = 'g', conflicts = ["file", "system"])]
    pub global: bool,

    /// Edit the system config file.
    #[usage(long, conflicts = ["file", "global"])]
    pub system: bool,

    /// Append the value without duplicating an existing entry.
    #[usage(long, conflicts = "remove")]
    pub append: bool,

    /// Remove the value from an existing collection.
    #[usage(long, conflicts = "append")]
    pub remove: bool,

    #[usage(value_enum, short, long, default = "infer")]
    pub type_: TomlValueTypes,
}

#[derive(usage_rs::ValueEnum, Default, Clone, Debug)]
pub(super) enum TomlValueTypes {
    #[default]
    Infer,
    #[usage()]
    String,
    #[usage()]
    Integer,
    #[usage()]
    Float,
    #[usage()]
    Bool,
    #[usage()]
    List,
    #[usage()]
    Set,
}

impl ConfigSet {
    pub(super) fn run(self) -> eyre::Result<()> {
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
        // Only an explicitly named target goes through the shared resolver — the default is a
        // different rule (the top TOML config of the loaded set, not the nearest writable one).
        let file = match self.file {
            Some(path) => Some(resolve_target_config_path(ConfigPathOptions {
                path: Some(path),
                prefer_toml: true,
                ..Default::default()
            })?),
            None if self.global => Some(resolve_target_config_path(ConfigPathOptions {
                global: true,
                prefer_toml: true,
                ..Default::default()
            })?),
            None if self.system => Some(system_config_path()),
            None => top_toml_config(),
        };
        let Some(file) = file else {
            bail!("No mise.toml file found");
        };
        if !file.to_string_lossy().ends_with(".toml") {
            bail!(
                "config set requires a TOML config file, but {} is not TOML",
                display_path(&file)
            );
        }
        if !file.exists() && !self.global && !self.system {
            bail!("config file not found: {}", display_path(&file));
        }
        let raw = match std::fs::read_to_string(&file) {
            Ok(raw) => raw,
            Err(error)
                if (self.global || self.system) && error.kind() == std::io::ErrorKind::NotFound =>
            {
                String::new()
            }
            Err(error) => return Err(error.into()),
        };
        let mut config: toml_edit::DocumentMut = raw.parse()?;
        let mut container = config.as_item_mut();
        let parts = full_key.split('.').collect::<Vec<&str>>();
        let last_key = parts.last().unwrap();
        for (idx, part) in parts.iter().take(parts.len() - 1).enumerate() {
            container = container
                .as_table_like_mut()
                .ok_or_else(|| {
                    eyre::eyre!(
                        "cannot set '{full_key}': '{}' is already set to a non-table value",
                        parts[..idx].join(".")
                    )
                })?
                .entry(part)
                .or_insert({
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    toml_edit::Item::Table(t)
                });
            // if the key is a tool with a simple value, we want to convert it to an inline table preserving the version
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
                let expected_type = full_key
                    .strip_prefix("settings.")
                    .and_then(|key| SETTINGS_META.get(key));
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
                let mut set = toml_edit::Array::new();
                let value = value.trim();
                if value != "[]" {
                    for item in value.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                        set.push(item);
                    }
                }
                toml_edit::Item::Value(toml_edit::Value::Array(dedup_toml_array(&set)))
            }
            TomlValueTypes::Infer => bail!("Type not found"),
        };

        let table = container.as_table_like_mut().ok_or_else(|| {
            eyre::eyre!(
                "cannot set '{full_key}': '{}' is already set to a non-table value",
                parts[..parts.len() - 1].join(".")
            )
        })?;
        if self.append {
            append_value(table, last_key, value)?;
        } else if self.remove {
            remove_value(table, last_key, &value)?;
        } else {
            table.insert(last_key, value);
        }

        let raw = config.to_string();
        MiseToml::from_str(&raw, &file)?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file, raw)?;
        Ok(())
    }
}

fn values(item: toml_edit::Item) -> eyre::Result<Vec<toml_edit::Value>> {
    match item {
        toml_edit::Item::Value(toml_edit::Value::Array(array)) => Ok(array.into_iter().collect()),
        toml_edit::Item::Value(value) => Ok(vec![value]),
        _ => bail!("collection updates require scalar or array values"),
    }
}

fn values_equal(left: &toml_edit::Value, right: &toml_edit::Value) -> bool {
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (left.as_integer(), right.as_integer()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (left.as_float(), right.as_float()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (left.as_bool(), right.as_bool()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (left.as_datetime(), right.as_datetime()) {
        return left == right;
    }
    left.to_string().trim() == right.to_string().trim()
}

fn append_value(
    table: &mut dyn toml_edit::TableLike,
    key: &str,
    value: toml_edit::Item,
) -> eyre::Result<()> {
    let additions = values(value)?;
    let Some(existing) = table.get_mut(key) else {
        let mut array = toml_edit::Array::new();
        for value in additions {
            array.push(value);
        }
        table.insert(key, toml_edit::value(array));
        return Ok(());
    };
    if !existing.is_array() {
        let original = existing
            .as_value()
            .cloned()
            .ok_or_else(|| eyre::eyre!("cannot append to '{key}': value is not scalar or array"))?;
        let mut array = toml_edit::Array::new();
        array.push(original);
        *existing = toml_edit::value(array);
    }
    let array = existing
        .as_array_mut()
        .expect("scalar values were converted to arrays");
    for value in additions {
        if !array.iter().any(|existing| values_equal(existing, &value)) {
            array.push(value);
        }
    }
    Ok(())
}

fn remove_value(
    table: &mut dyn toml_edit::TableLike,
    key: &str,
    value: &toml_edit::Item,
) -> eyre::Result<()> {
    let removals = values(value.clone())?;
    let Some(existing) = table.get_mut(key) else {
        return Ok(());
    };
    if let Some(array) = existing.as_array_mut() {
        array.retain(|existing| !removals.iter().any(|value| values_equal(value, existing)));
        if array.is_empty() {
            table.remove(key);
        }
    } else if existing
        .as_value()
        .is_some_and(|existing| removals.iter().any(|value| values_equal(value, existing)))
    {
        table.remove(key);
    }
    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise config set tools.python 3.12</bold>
    $ <bold>mise config set settings.always_keep_download true</bold>
    $ <bold>mise config set env.TEST_ENV_VAR ABC</bold>
    $ <bold>mise config set settings.disable_tools node,rust</bold>
    $ <bold>mise config set --append env._.path ~/.local/bin</bold>
    $ <bold>mise config set --remove env._.path ~/.local/bin</bold>

    # Type for `settings` is inferred
    $ <bold>mise config set settings.jobs 4</bold>
"#
);
