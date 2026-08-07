use crate::config;
use crate::config::settings::{SETTINGS_META, SettingsPartial, SettingsType};
use crate::config::{ALL_TOML_CONFIG_FILES, Settings};
use crate::file::display_path;
use crate::ui::table;
use eyre::Result;
use std::path::{Path, PathBuf};
use tabled::{Table, Tabled};

/// Show current settings
///
/// This is the contents of ~/.config/mise/config.toml
///
/// Note that aliases are also stored in this file
/// but managed separately with `mise tool-alias`
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsLs {
    /// Name of setting
    pub setting: Option<String>,

    /// List all settings
    #[clap(long, short)]
    all: bool,

    /// Output in JSON format
    #[clap(long, short = 'J', group = "output")]
    json: bool,

    /// Use the local config file instead of the global one
    #[clap(long, short, global = true)]
    pub local: bool,

    /// Output in TOML format
    #[clap(long, short = 'T', group = "output")]
    toml: bool,

    /// Print all settings with descriptions for shell completions
    #[clap(long, hide = true)]
    complete: bool,

    /// Output in JSON format with sources
    #[clap(long, group = "output")]
    json_extended: bool,
}

fn settings_type_to_string(st: &SettingsType) -> String {
    match st {
        SettingsType::Bool => "boolean".to_string(),
        SettingsType::BoolOrString => "boolean | string".to_string(),
        SettingsType::String => "string".to_string(),
        SettingsType::Integer => "number".to_string(),
        SettingsType::Duration => "number".to_string(),
        SettingsType::Path => "string".to_string(),
        SettingsType::Url => "string".to_string(),
        SettingsType::ListString => "array".to_string(),
        SettingsType::ListPath => "array".to_string(),
        SettingsType::SetString => "array".to_string(),
        SettingsType::IndexMap => "object".to_string(),
    }
}

impl SettingsLs {
    pub fn run(self) -> Result<()> {
        if self.complete {
            return self.complete();
        }
        let mut rows: Vec<Row> = if self.local {
            let source = config::local_toml_config_path();
            let partial = Settings::parse_settings_file(&source).unwrap_or_default();
            Row::from_partial(&partial, &source)?
        } else {
            let mut rows = vec![];
            if self.all {
                for (k, v) in Settings::get().as_dict()? {
                    rows.extend(Row::from_toml(k.to_string(), v, None));
                }
            }
            rows.extend(ALL_TOML_CONFIG_FILES.iter().rev().flat_map(|source| {
                match Settings::parse_settings_file(source) {
                    Ok(partial) => match Row::from_partial(&partial, source) {
                        Ok(rows) => rows,
                        Err(e) => {
                            warn!("Error parsing {}: {}", display_path(source), e);
                            vec![]
                        }
                    },
                    Err(e) => {
                        warn!("Error parsing {}: {}", display_path(source), e);
                        vec![]
                    }
                }
            }));
            rows
        };
        if let Some(key) = &self.setting {
            rows.retain(|r| &r.key == key || r.key.starts_with(&format!("{key}.")));
        }
        for k in Settings::hidden_configs() {
            rows.retain(|r| &r.key != k || r.key.starts_with(&format!("{k}.")));
        }
        if self.json {
            self.print_json(rows)?;
        } else if self.json_extended {
            self.print_json_extended(rows)?;
        } else if self.toml {
            self.print_toml(rows)?;
        } else {
            let mut table = Table::new(rows);
            table::default_style(&mut table, false);
            miseprintln!("{}", table.to_string());
        }
        Ok(())
    }

    fn complete(&self) -> Result<()> {
        for (k, sm) in SETTINGS_META.iter() {
            println!("{k}:{}", sm.description.replace(":", "\\:"));
        }
        Ok(())
    }

    fn print_json(&self, rows: Vec<Row>) -> Result<()> {
        let mut table = serde_json::Map::new();
        for row in rows {
            insert_json_value(
                &mut table,
                &row.path,
                toml_value_to_json_value(row.toml_value),
            );
        }
        miseprintln!("{}", serde_json::to_string_pretty(&table)?);
        Ok(())
    }

    fn print_json_extended(&self, rows: Vec<Row>) -> Result<()> {
        let mut table = serde_json::Map::new();
        for row in rows {
            let mut entry = serde_json::Map::new();
            entry.insert(
                "value".to_string(),
                toml_value_to_json_value(row.toml_value),
            );
            entry.insert("type".to_string(), row.type_.into());
            if let Some(description) = row.description {
                entry.insert("description".to_string(), description.into());
            }
            if let Some(source) = row.source {
                entry.insert("source".to_string(), source.to_string_lossy().into());
            }
            insert_json_value(&mut table, &row.path, entry.into());
        }
        miseprintln!("{}", serde_json::to_string_pretty(&table)?);
        Ok(())
    }

    fn print_toml(&self, rows: Vec<Row>) -> Result<()> {
        let mut table = toml::Table::new();
        for row in rows {
            insert_toml_value(&mut table, &row.path, row.toml_value);
        }
        miseprintln!("{}", toml::to_string(&table)?);
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

#[derive(Debug, Tabled)]
#[tabled(rename_all = "PascalCase")]
struct Row {
    key: String,
    value: String,
    #[tabled(display = "Self::display_option_path")]
    source: Option<PathBuf>,
    #[tabled(skip)]
    toml_value: toml::Value,
    #[tabled(skip)]
    description: Option<String>,
    #[tabled(skip)]
    type_: String,
    #[tabled(skip)]
    path: Vec<String>,
}

impl Row {
    fn display_option_path(o: &Option<PathBuf>) -> String {
        o.as_ref().map(display_path).unwrap_or_default()
    }

    fn from_partial(p: &SettingsPartial, source: &Path) -> Result<Vec<Self>> {
        let rows = Settings::partial_as_dict(p)?
            .into_iter()
            .flat_map(|(k, v)| Self::from_toml(k.to_string(), v, Some(source.to_path_buf())))
            .collect();
        Ok(rows)
    }

    fn from_toml(k: String, v: toml::Value, source: Option<PathBuf>) -> Vec<Self> {
        Self::from_toml_path(vec![k], v, source)
    }

    fn from_toml_path(path: Vec<String>, v: toml::Value, source: Option<PathBuf>) -> Vec<Self> {
        let k = path.join(".");
        let mut rows = vec![];
        if let Some(table) = v.as_table() {
            if !table.is_empty() {
                rows.reserve(table.len());
                let meta = SETTINGS_META.get(k.as_str());
                for (subkey, subvalue) in table {
                    let mut child_path = path.clone();
                    child_path.push(subkey.clone());
                    if let Some(meta) = meta {
                        rows.push(Row {
                            key: child_path.join("."),
                            value: subvalue.to_string(),
                            type_: settings_type_to_string(&meta.type_),
                            source: source.clone(),
                            toml_value: subvalue.clone(),
                            description: Some(meta.description.to_string()),
                            path: child_path,
                        });
                    } else {
                        rows.extend(Self::from_toml_path(
                            child_path,
                            subvalue.clone(),
                            source.clone(),
                        ));
                    }
                }
            }
        } else {
            let meta = SETTINGS_META.get(k.as_str());
            rows.push(Row {
                key: k.clone(),
                value: v.to_string(),
                type_: meta
                    .map(|sm| settings_type_to_string(&sm.type_))
                    .unwrap_or_default(),
                source,
                toml_value: v,
                description: meta.map(|sm| sm.description.to_string()),
                path,
            });
        }
        rows
    }
}

fn insert_json_value(
    table: &mut serde_json::Map<String, serde_json::Value>,
    path: &[String],
    value: serde_json::Value,
) {
    let Some((key, rest)) = path.split_first() else {
        return;
    };
    if rest.is_empty() {
        table.insert(key.clone(), value);
        return;
    }
    let subtable = table
        .entry(key.clone())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    insert_json_value(subtable.as_object_mut().unwrap(), rest, value);
}

fn insert_toml_value(table: &mut toml::Table, path: &[String], value: toml::Value) {
    let Some((key, rest)) = path.split_first() else {
        return;
    };
    if rest.is_empty() {
        table.insert(key.clone(), value);
        return;
    }
    let subtable = table
        .entry(key.clone())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    insert_toml_value(subtable.as_table_mut().unwrap(), rest, value);
}

fn toml_value_to_json_value(v: toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => s.into(),
        toml::Value::Integer(i) => i.into(),
        toml::Value::Boolean(b) => b.into(),
        toml::Value::Float(f) => f.into(),
        toml::Value::Table(t) => {
            let mut table = serde_json::Map::new();
            for (k, v) in t {
                table.insert(k, toml_value_to_json_value(v));
            }
            table.into()
        }
        toml::Value::Array(a) => a.into_iter().map(toml_value_to_json_value).collect(),
        v => v.to_string().into(),
    }
}
