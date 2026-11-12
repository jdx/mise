//! Bootstrap settings plugins: `[bootstrap.settings.<plugin>]` sections whose values a plugin
//! reads and writes, such as a desktop's appearance settings.
//!
//! mise owns the comparison. It asks the plugin's `SettingsRead` hook for the current value of
//! every declared key, and calls `SettingsWrite` only with the keys that differ, so a dry run or a
//! status check never runs plugin code that can change anything.

use std::sync::Arc;

use eyre::{Result, bail};
use indexmap::IndexMap;
use serde_json::Value as Json;
use vfox::{SettingChange, SettingsReadContext, SettingsWriteContext};

use crate::config::Config;
use crate::plugins::PluginType;
use crate::plugins::mise_plugin_toml::MisePluginToml;
use crate::plugins::system_plugins;
use crate::plugins::vfox_plugin::VfoxPlugin;

/// The merged `[bootstrap.settings.<plugin>]` section of one plugin, flattened to dotted keys.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsSection {
    pub plugin: String,
    pub values: IndexMap<String, Json>,
}

/// Every plugin's section, merged across config files: a more local file's value for a key
/// replaces the others, and tables merge key by key.
pub fn sections_from_config(config: &Config) -> Vec<SettingsSection> {
    let mut merged: IndexMap<String, toml::Table> = IndexMap::new();
    for cf in config.config_files.values().rev() {
        let Some(bootstrap) = cf.bootstrap_config() else {
            continue;
        };
        for (plugin, table) in bootstrap.settings {
            mise_system::merge_settings_table(merged.entry(plugin).or_default(), table);
        }
    }
    merged
        .into_iter()
        .map(|(plugin, table)| {
            let mut values = IndexMap::new();
            flatten("", table, &mut values);
            SettingsSection { plugin, values }
        })
        .filter(|section| !section.values.is_empty())
        .collect()
}

fn flatten(prefix: &str, table: toml::Table, out: &mut IndexMap<String, Json>) {
    for (key, value) in table {
        let key = if prefix.is_empty() {
            key
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(table) => flatten(&key, table, out),
            value => {
                out.insert(key, toml_to_json(value));
            }
        }
    }
}

fn toml_to_json(value: toml::Value) -> Json {
    match value {
        toml::Value::String(s) => Json::String(s),
        toml::Value::Integer(i) => Json::from(i),
        toml::Value::Float(f) => Json::from(f),
        toml::Value::Boolean(b) => Json::Bool(b),
        toml::Value::Datetime(d) => Json::String(d.to_string()),
        toml::Value::Array(items) => Json::Array(items.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => Json::Object(
            table
                .into_iter()
                .map(|(k, v)| (k, toml_to_json(v)))
                .collect(),
        ),
    }
}

/// Whether two values are the same, comparing numbers by value: Lua often returns `4.0` for a
/// TOML `4`. Otherwise types must match, so `4` and `"4"` differ.
fn same_value(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(a), Json::Number(b)) => a.as_f64() == b.as_f64(),
        (Json::Array(a), Json::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_value(a, b))
        }
        (Json::Object(a), Json::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same_value(a, b)))
        }
        (a, b) => a == b,
    }
}

fn display(value: Option<&Json>) -> String {
    value.map_or_else(|| "(unset)".to_string(), Json::to_string)
}

/// A declared key and the value the plugin reports for it now.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyStatus {
    pub key: String,
    pub declared: Json,
    /// `None` when the key is unset.
    pub current: Option<Json>,
}

impl KeyStatus {
    pub fn differs(&self) -> bool {
        !self
            .current
            .as_ref()
            .is_some_and(|current| same_value(current, &self.declared))
    }

    pub fn current_display(&self) -> String {
        display(self.current.as_ref())
    }
}

#[derive(Debug, Default)]
pub struct ApplyReport {
    pub changed: Vec<KeyStatus>,
    pub follow_up: Vec<String>,
}

/// An installed bootstrap settings plugin that can run on this machine.
pub struct BootstrapSettingsPlugin {
    name: String,
    plugin: Arc<VfoxPlugin>,
}

impl BootstrapSettingsPlugin {
    /// The plugin named `name`, or why it cannot run here.
    pub fn load(name: &str) -> std::result::Result<Self, String> {
        let path = system_plugins::resolve(name, &mise_util::dirs::PLUGINS);
        match PluginType::from_plugin_path(&path) {
            Some(PluginType::BootstrapSettings) => {}
            Some(_) => return Err(format!("plugin {name} is not a bootstrap settings plugin")),
            None => {
                return Err(format!(
                    "no bootstrap settings plugin named {name} is installed"
                ));
            }
        }
        let config = MisePluginToml::from_file(&path.join("mise.plugin.toml"))
            .map_err(|err| format!("{err:#}"))?
            .bootstrap_settings;
        let os = crate::config::Settings::get().os().to_string();
        if let Some(oses) = &config.os
            && !oses.contains(&os)
        {
            return Err(format!("{name} is not available on {os}"));
        }
        if let Some(binary) = crate::system::package_plugin::missing_binary(&config.requires) {
            return Err(format!(
                "{name} needs '{binary}', which was not found; add it to [tools] or install it"
            ));
        }
        Ok(Self {
            name: name.to_string(),
            plugin: Arc::new(VfoxPlugin::new(name.to_string(), path)),
        })
    }

    async fn vfox(&self) -> Result<vfox::Vfox> {
        let env = crate::system::package_plugin::bootstrap_plugin_env().await?;
        let (mut vfox, _) = self.plugin.vfox()?;
        // A system plugin lives outside the user's plugins directory, and vfox finds a plugin by
        // name inside `plugin_dir`.
        if let Some(dir) = self.plugin.plugin_path.parent() {
            vfox.plugin_dir = dir.to_path_buf();
        }
        vfox.cmd_env = Some(env);
        Ok(vfox)
    }

    async fn read(&self, keys: Vec<String>) -> Result<IndexMap<String, Json>> {
        let response = self
            .vfox()
            .await?
            .settings_read(&self.name, SettingsReadContext { keys })
            .await?;
        Ok(response.values)
    }

    /// Each declared key with its current value.
    pub async fn status(&self, section: &SettingsSection) -> Result<Vec<KeyStatus>> {
        let current = self.read(section.values.keys().cloned().collect()).await?;
        Ok(section
            .values
            .iter()
            .map(|(key, declared)| KeyStatus {
                key: key.clone(),
                declared: declared.clone(),
                current: current.get(key).cloned(),
            })
            .collect())
    }

    /// Writes the keys that differ, then reads them again to confirm the plugin wrote them.
    pub async fn apply(&self, section: &SettingsSection, dry_run: bool) -> Result<ApplyReport> {
        let changed: Vec<KeyStatus> = self
            .status(section)
            .await?
            .into_iter()
            .filter(KeyStatus::differs)
            .collect();
        for status in &changed {
            info!(
                "{}{}: {} {} -> {}",
                if dry_run { "would set " } else { "" },
                self.name,
                status.key,
                status.current_display(),
                status.declared
            );
        }
        if dry_run || changed.is_empty() {
            return Ok(ApplyReport {
                changed,
                follow_up: vec![],
            });
        }
        let response = self
            .vfox()
            .await?
            .settings_write(
                &self.name,
                SettingsWriteContext {
                    changes: changed
                        .iter()
                        .map(|status| SettingChange {
                            key: status.key.clone(),
                            from: status.current.clone(),
                            to: status.declared.clone(),
                        })
                        .collect(),
                },
            )
            .await?;
        let after = self
            .read(changed.iter().map(|status| status.key.clone()).collect())
            .await?;
        for status in &changed {
            let current = after.get(&status.key);
            if !current.is_some_and(|current| same_value(current, &status.declared)) {
                bail!(
                    "{}: {} is {} after writing {}",
                    self.name,
                    status.key,
                    display(current),
                    status.declared
                );
            }
        }
        Ok(ApplyReport {
            changed,
            follow_up: response.follow_up,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tables_merge_key_by_key_and_flatten_to_dotted_keys() {
        let mut base: toml::Table = toml::from_str(
            r#"
            [appearance]
            blur = false
            size = 2
            [monitors]
            list = ["a"]
            "#,
        )
        .unwrap();
        let higher: toml::Table = toml::from_str(
            r#"
            [appearance]
            blur = true
            [monitors]
            list = ["b", "c"]
            "#,
        )
        .unwrap();
        mise_system::merge_settings_table(&mut base, higher);
        let mut values = IndexMap::new();
        flatten("", base, &mut values);
        assert_eq!(values["appearance.blur"], json!(true));
        assert_eq!(values["appearance.size"], json!(2));
        assert_eq!(values["monitors.list"], json!(["b", "c"]));
    }

    #[test]
    fn values_compare_by_type_and_numbers_by_value() {
        let status = KeyStatus {
            key: "size".into(),
            declared: json!(4),
            current: Some(json!("4")),
        };
        assert!(status.differs());
        let unset = KeyStatus {
            current: None,
            ..status.clone()
        };
        assert!(unset.differs());
        assert_eq!(unset.current_display(), "(unset)");
        let same = KeyStatus {
            current: Some(json!(4.0)),
            ..status
        };
        assert!(!same.differs());
    }
}
