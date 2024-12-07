use crate::config;
use crate::config::settings::SettingsPartial;
use crate::config::{Settings, ALL_TOML_CONFIG_FILES, SETTINGS};
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
/// but managed separately with `mise aliases`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsLs {
    /// List keys under this key
    pub key: Option<String>,

    /// Display settings set to the default
    #[clap(long, short)]
    pub all: bool,

    /// Use the local config file instead of the global one
    #[clap(long, short)]
    pub local: bool,

    /// Output in JSON format
    #[clap(long, short = 'J', group = "output")]
    pub json: bool,

    /// Output in JSON format with sources
    #[clap(long, group = "output")]
    pub json_extended: bool,

    /// Output in TOML format
    #[clap(long, short = 'T', group = "output")]
    pub toml: bool,
}

impl SettingsLs {
    pub fn run(self) -> Result<()> {
        let mut rows: Vec<Row> = if self.local {
            let source = config::local_toml_config_path();
            let partial = Settings::parse_settings_file(&source).unwrap_or_default();
            Row::from_partial(&partial, &source)?
        } else {
            let mut rows = vec![];
            if self.all {
                for (k, v) in SETTINGS.as_dict()? {
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
        if let Some(key) = &self.key {
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

    fn print_json(&self, rows: Vec<Row>) -> Result<()> {
        let mut table = serde_json::Map::new();
        for row in rows {
            if let Some((key, subkey)) = row.key.split_once('.') {
                let subtable = table
                    .entry(key)
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                let subtable = subtable.as_object_mut().unwrap();
                subtable.insert(subkey.to_string(), toml_value_to_json_value(row.toml_value));
            } else {
                table.insert(row.key, toml_value_to_json_value(row.toml_value));
            }
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
            if let Some(source) = row.source {
                entry.insert("source".to_string(), source.to_string_lossy().into());
            }
            if let Some((key, subkey)) = row.key.split_once('.') {
                let subtable = table
                    .entry(key)
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                let subtable = subtable.as_object_mut().unwrap();
                subtable.insert(subkey.to_string(), entry.into());
            } else {
                table.insert(row.key, entry.into());
            }
        }
        miseprintln!("{}", serde_json::to_string_pretty(&table)?);
        Ok(())
    }

    fn print_toml(&self, rows: Vec<Row>) -> Result<()> {
        let mut table = toml::Table::new();
        for row in rows {
            if let Some((key, subkey)) = row.key.split_once('.') {
                let subtable = table
                    .entry(key)
                    .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                let subtable = subtable.as_table_mut().unwrap();
                subtable.insert(subkey.to_string(), row.toml_value);
                continue;
            } else {
                table.insert(row.key, row.toml_value);
            }
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
    #[tabled(display_with = "Self::display_option_path")]
    source: Option<PathBuf>,
    #[tabled(skip)]
    toml_value: toml::Value,
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
        let mut rows = vec![];
        if let Some(table) = v.as_table() {
            if !table.is_empty() {
                for (subkey, subvalue) in table {
                    rows.push(Row {
                        key: format!("{k}.{subkey}"),
                        value: subvalue.to_string(),
                        source: source.clone(),
                        toml_value: subvalue.clone(),
                    });
                }
            }
        } else {
            rows.push(Row {
                key: k,
                value: v.to_string(),
                source,
                toml_value: v,
            });
        }
        rows
    }
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
