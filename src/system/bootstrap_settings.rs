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
/// replaces the others, and tables merge key by key. `desk_plugin` and `desk-plugin` are one
/// plugin, so their sections merge under the kebab-case name. Independent `[bootstrap].config_roots` must
/// not set the same key to different values.
pub fn sections_from_config(config: &Config) -> Result<Vec<SettingsSection>> {
    let mut sections: IndexMap<String, IndexMap<String, Json>> = IndexMap::new();
    for config_files in config.bootstrap_config_maps() {
        let mut merged: IndexMap<String, toml::Table> = IndexMap::new();
        for cf in config_files.values().rev() {
            let Some(bootstrap) = cf.bootstrap_config() else {
                continue;
            };
            for (plugin, table) in bootstrap.settings {
                let plugin = system_plugins::identity(&plugin);
                mise_system::merge_settings_table(merged.entry(plugin).or_default(), table);
            }
        }
        for (plugin, table) in merged {
            let mut values = IndexMap::new();
            flatten("", table, &mut values);
            let section = sections.entry(plugin.clone()).or_default();
            for (key, value) in values {
                match section.get(&key) {
                    Some(existing) if !same_value(existing, &value) => bail!(
                        "[bootstrap.settings.{plugin}]: {key} is {existing} in one bootstrap \
                         config root and {value} in another",
                    ),
                    Some(_) => {}
                    None => {
                        section.insert(key, value);
                    }
                }
            }
        }
    }
    Ok(sections
        .into_iter()
        .filter(|(_, values)| !values.is_empty())
        .map(|(plugin, values)| SettingsSection { plugin, values })
        .collect())
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
        (Json::Number(a), Json::Number(b)) => same_number(a, b),
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

/// Integers compare exactly, so two integers too large for an `f64` stay distinct. An integer and
/// a float are equal only when the float is that integer exactly.
fn same_number(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    fn int(n: &serde_json::Number) -> Option<i128> {
        n.as_i64()
            .map(i128::from)
            .or_else(|| n.as_u64().map(i128::from))
    }
    fn float_as_int(f: f64) -> Option<i128> {
        // 2^127 bounds i128; beyond it no integer from JSON can match.
        (f.fract() == 0.0 && f.abs() < 1.7e38).then_some(f as i128)
    }
    match (int(a), int(b)) {
        (Some(a), Some(b)) => a == b,
        (Some(i), None) => b.as_f64().and_then(float_as_int) == Some(i),
        (None, Some(i)) => a.as_f64().and_then(float_as_int) == Some(i),
        (None, None) => a.as_f64() == b.as_f64(),
    }
}

/// Whether a plugin's Lua sees `value` exactly: Lua 5.1 numbers are doubles, so integers beyond
/// 2^53 would reach the hook rounded.
fn fits_in_lua(value: &Json) -> bool {
    const MAX: u64 = 1 << 53;
    match value {
        Json::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => i.unsigned_abs() <= MAX,
            (None, Some(u)) => u <= MAX,
            _ => true,
        },
        Json::Array(values) => values.iter().all(fits_in_lua),
        Json::Object(values) => values.values().all(fits_in_lua),
        _ => true,
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

/// Why a bootstrap settings plugin cannot run here.
#[derive(Debug)]
pub struct Unavailable {
    pub reason: String,
    /// The plugin is for another OS: one config can hold every platform's sections, so this is
    /// not drift on this machine.
    pub other_platform: bool,
}

impl From<String> for Unavailable {
    fn from(reason: String) -> Self {
        Self {
            reason,
            other_platform: false,
        }
    }
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

/// An installed bootstrap settings plugin that can run on this machine.
pub struct BootstrapSettingsPlugin {
    name: String,
    plugin: Arc<VfoxPlugin>,
    /// The environment its hooks run with, including the `PATH` its `requires` were found on.
    env: IndexMap<String, String>,
}

impl BootstrapSettingsPlugin {
    /// The plugin named `name`, or why it cannot run here.
    pub async fn load(name: &str) -> std::result::Result<Self, Unavailable> {
        let path = system_plugins::resolve(name, &mise_util::dirs::PLUGINS);
        match PluginType::from_plugin_path(&path) {
            Some(PluginType::BootstrapSettings) => {}
            Some(_) => {
                return Err(format!("plugin {name} is not a bootstrap settings plugin").into());
            }
            None => {
                return Err(
                    format!("no bootstrap settings plugin named {name} is installed").into(),
                );
            }
        }
        let config = MisePluginToml::from_file(&path.join("mise.plugin.toml"))
            .map_err(|err| format!("{err:#}"))?
            .bootstrap_settings;
        let os = crate::config::Settings::get().os().to_string();
        if let Some(oses) = &config.os
            && !oses.contains(&os)
        {
            return Err(Unavailable {
                reason: format!("{name} is not available on {os}"),
                other_platform: true,
            });
        }
        // Look for `requires` on the PATH the hooks will get, which includes global [tools].
        let env = crate::system::package_plugin::bootstrap_plugin_env()
            .await
            .map_err(|err| format!("{err:#}"))?;
        if let Some(binary) = crate::system::package_plugin::missing_in_env(&config.requires, &env)
        {
            return Err(format!(
                "{name} needs '{binary}', which was not found; add it to [tools] or install it"
            )
            .into());
        }
        // vfox loads a plugin by the name of its directory, which can differ from `name` in case
        // style (`foo_bar` in config, `foo-bar` on disk).
        let dir_name = path
            .file_name()
            .map_or_else(|| name.to_string(), |n| n.to_string_lossy().into_owned());
        Ok(Self {
            name: name.to_string(),
            plugin: Arc::new(VfoxPlugin::new(dir_name, path)),
            env,
        })
    }

    fn vfox(&self) -> Result<vfox::Vfox> {
        let (mut vfox, _) = self.plugin.vfox()?;
        // A system plugin lives outside the user's plugins directory, and vfox finds a plugin by
        // name inside `plugin_dir`.
        if let Some(dir) = self.plugin.plugin_path.parent() {
            vfox.plugin_dir = dir.to_path_buf();
        }
        vfox.cmd_env = Some(self.env.clone());
        Ok(vfox)
    }

    async fn read(&self, keys: Vec<String>) -> Result<IndexMap<String, Json>> {
        let response = self
            .vfox()?
            .settings_read(&self.plugin.name, SettingsReadContext { keys })
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
        if let Some(status) = changed.iter().find(|status| !fits_in_lua(&status.declared)) {
            bail!(
                "{}: {} is {}, an integer Lua cannot hold exactly; settings plugins see numbers \
                 only up to 2^53",
                self.name,
                status.key,
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
            .vfox()?
            .settings_write(
                &self.plugin.name,
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
        // The write may already have taken effect, so a retry would find nothing to change and
        // return no follow-up steps: keep them in any error from here on.
        let follow_up = response
            .follow_up
            .iter()
            .map(|item| format!("\n  follow-up: {item}"))
            .collect::<String>();
        let after = match self
            .read(changed.iter().map(|status| status.key.clone()).collect())
            .await
        {
            Ok(after) => after,
            Err(err) if follow_up.is_empty() => return Err(err),
            Err(err) => {
                return Err(err.wrap_err(format!(
                    "{}: reading the settings back after writing them failed{follow_up}",
                    self.name
                )));
            }
        };
        for status in &changed {
            let current = after.get(&status.key);
            if !current.is_some_and(|current| same_value(current, &status.declared)) {
                // Some changes show only after a step such as logging out; keep those steps.
                bail!(
                    "{}: {} is {} after writing {}{follow_up}",
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
    fn a_higher_nested_value_beats_a_lower_dotted_key() {
        let mut base = toml::Table::new();
        let lower: toml::Table = toml::from_str(
            r#"
            appearance = { size = 2 }
            "appearance.blur" = false
            "#,
        )
        .unwrap();
        let higher: toml::Table = toml::from_str("appearance = { blur = true }").unwrap();
        mise_system::merge_settings_table(&mut base, lower);
        mise_system::merge_settings_table(&mut base, higher);
        let mut values = IndexMap::new();
        flatten("", base, &mut values);
        assert_eq!(values["appearance.blur"], json!(true));
        assert_eq!(values["appearance.size"], json!(2));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn large_integers_compare_exactly() {
        let a = json!(9_007_199_254_740_992_i64);
        let b = json!(9_007_199_254_740_993_i64);
        assert!(!same_value(&a, &b));
        assert!(same_value(&b, &b));
        assert!(same_value(&json!(9_007_199_254_740_992.0_f64), &a));
        assert!(!same_value(&json!(9_007_199_254_740_992.0_f64), &b));
        assert!(!same_value(&json!(4.5), &json!(4)));
        assert!(same_value(&json!(u64::MAX), &json!(u64::MAX)));
    }

    #[test]
    fn integers_beyond_2_53_do_not_fit_in_lua() {
        assert!(fits_in_lua(&json!(9_007_199_254_740_992_i64)));
        assert!(fits_in_lua(&json!(-9_007_199_254_740_992_i64)));
        assert!(!fits_in_lua(&json!(9_007_199_254_740_993_i64)));
        assert!(!fits_in_lua(&json!([1, 9_007_199_254_740_993_i64])));
        assert!(!fits_in_lua(&json!(u64::MAX)));
        assert!(fits_in_lua(&json!(1e300)));
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
