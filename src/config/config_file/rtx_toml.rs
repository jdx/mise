use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::eyre;
use color_eyre::{Result, Section};
use log::LevelFilter;
use toml_edit::{table, value, Array, Item, Value};

use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::config::settings::SettingsBuilder;
use crate::config::{AliasMap, MissingRuntimeBehavior};
use crate::plugins::PluginName;
use crate::toolset::{ToolSource, ToolVersion, ToolVersionList, ToolVersionType, Toolset};

#[derive(Debug, Default)]
pub struct RtxToml {
    path: PathBuf,
    toolset: Toolset,
    env: HashMap<String, String>,
    settings: SettingsBuilder,
    alias: AliasMap,
    doc: toml_edit::Document,
    plugins: HashMap<String, String>,
}

#[macro_export]
macro_rules! parse_error {
    ($key:expr, $val:expr, $t:expr) => {{
        Err(eyre!(
            r#"expected value of "{}" to be a {}, got: {}"#,
            $key,
            $t,
            $val
        ))
    }};
}

#[allow(dead_code)] // TODO: remove
impl RtxToml {
    pub fn init(filename: &Path) -> Self {
        Self {
            path: filename.to_path_buf(),
            ..Default::default()
        }
    }

    pub fn from_file(filename: &Path) -> Result<Self> {
        trace!("parsing: {}", filename.display());
        let body = fs::read_to_string(filename).suggestion("ensure file exists and can be read")?;
        let mut rf = Self::from_str(body)?;
        rf.path = filename.to_path_buf();
        Ok(rf)
    }

    pub fn from_str(s: String) -> Result<Self> {
        Self {
            doc: s.parse().suggestion("ensure file is valid TOML")?,
            ..Default::default()
        }
        .parse()
    }

    fn parse(mut self) -> Result<Self> {
        for (k, v) in self.doc.iter() {
            match k {
                "env" => self.env = self.parse_hashmap(k, v)?,
                "alias" => self.alias = self.parse_alias(k, v)?,
                "tools" => self.toolset = self.parse_toolset(k, v)?,
                "settings" => self.settings = self.parse_settings(k, v)?,
                "plugins" => self.plugins = self.parse_hashmap(k, v)?,
                _ => warn!("unknown key: {}", k),
            }
        }
        Ok(self)
    }

    pub fn settings(&self) -> SettingsBuilder {
        SettingsBuilder::default()
    }

    fn parse_alias(&self, k: &str, v: &Item) -> Result<AliasMap> {
        match v.as_table_like() {
            Some(table) => {
                let mut aliases = AliasMap::new();
                for (plugin, table) in table.iter() {
                    let k = format!("{}.{}", k, plugin);
                    let plugin_aliases = aliases.entry(plugin.into()).or_default();
                    match table.as_table_like() {
                        Some(table) => {
                            for (from, to) in table.iter() {
                                match to.as_str() {
                                    Some(s) => {
                                        plugin_aliases.insert(from.into(), s.into());
                                    }
                                    _ => parse_error!(format!("{}.{}", k, from), v, "string")?,
                                }
                            }
                        }
                        _ => parse_error!(k, v, "table")?,
                    }
                }
                Ok(aliases)
            }
            _ => parse_error!(k, v, "table")?,
        }
    }

    fn parse_hashmap(&self, key: &str, v: &Item) -> Result<HashMap<String, String>> {
        match v.as_table_like() {
            Some(table) => {
                let mut env = HashMap::new();
                for (k, v) in table.iter() {
                    match v.as_str() {
                        Some(s) => {
                            env.insert(k.into(), s.into());
                        }
                        _ => parse_error!(key, v, "string")?,
                    }
                }
                Ok(env)
            }
            _ => parse_error!(key, v, "table"),
        }
    }

    fn parse_toolset(&self, key: &str, v: &Item) -> Result<Toolset> {
        let source = ToolSource::RtxToml(self.path.clone());
        let mut toolset = Toolset::new(source);

        match v.as_table_like() {
            Some(table) => {
                for (plugin, v) in table.iter() {
                    let k = format!("{}.{}", key, plugin);
                    let tvl = self.parse_tool_version_list(&k, v, &plugin.into())?;
                    toolset.versions.insert(plugin.into(), tvl);
                }
                Ok(toolset)
            }
            _ => parse_error!(key, v, "table")?,
        }
    }

    fn parse_tool_version_list(
        &self,
        key: &str,
        v: &Item,
        plugin_name: &PluginName,
    ) -> Result<ToolVersionList> {
        let source = ToolSource::RtxToml(self.path.clone());
        let mut tool_version_list = ToolVersionList::new(source);

        match v {
            Item::ArrayOfTables(v) => {
                for table in v.iter() {
                    for (tool, v) in table.iter() {
                        let k = format!("{}.{}", key, tool);
                        let tv = self.parse_tool_version(&k, v, plugin_name)?;
                        tool_version_list.versions.push(tv);
                    }
                }
            }
            v => match v.as_array() {
                Some(v) => {
                    for v in v.iter() {
                        let item = Item::Value(v.clone());
                        let tv = self.parse_tool_version(key, &item, plugin_name)?;
                        tool_version_list.versions.push(tv);
                    }
                }
                _ => {
                    tool_version_list
                        .versions
                        .push(self.parse_tool_version(key, v, plugin_name)?)
                }
            },
        }

        Ok(tool_version_list)
    }

    fn parse_tool_version(
        &self,
        key: &str,
        v: &Item,
        plugin_name: &PluginName,
    ) -> Result<ToolVersion> {
        let mut tv = ToolVersion::new(plugin_name.clone(), ToolVersionType::System);

        match v.as_table_like() {
            Some(table) => {
                if let Some(v) = table.get("version") {
                    match v {
                        Item::Value(v) => {
                            tv.r#type = self.parse_tool_version_value(key, v)?;
                        }
                        _ => parse_error!(format!("{}.version", key), v, "string")?,
                    }
                } else if let Some(path) = table.get("path") {
                    match path.as_str() {
                        Some(s) => {
                            tv.r#type = ToolVersionType::Path(s.into());
                        }
                        _ => parse_error!(format!("{}.path", key), v, "string")?,
                    }
                } else if let Some(prefix) = table.get("prefix") {
                    match prefix.as_str() {
                        Some(s) => {
                            tv.r#type = ToolVersionType::Prefix(s.into());
                        }
                        _ => parse_error!(format!("{}.prefix", key), v, "string")?,
                    }
                } else if let Some(r) = table.get("ref") {
                    match r.as_str() {
                        Some(s) => {
                            tv.r#type = ToolVersionType::Ref(s.into());
                        }
                        _ => parse_error!(format!("{}.ref", key), v, "string")?,
                    }
                } else {
                    parse_error!(key, v, "version, path, or prefix")?
                }
                for (k, v) in table.iter() {
                    if k == "version" || k == "path" || k == "prefix" || k == "ref" {
                        continue;
                    }
                    match v.as_str() {
                        Some(s) => {
                            tv.options.insert(k.into(), s.into());
                        }
                        _ => parse_error!(format!("{}.{}", key, k), v, "string")?,
                    }
                }
            }
            _ => match v {
                Item::Value(v) => {
                    tv.r#type = self.parse_tool_version_value(key, v)?;
                }
                _ => parse_error!(key, v, "value")?,
            },
        }

        Ok(tv)
    }

    fn parse_tool_version_value(&self, key: &str, v: &Value) -> Result<ToolVersionType> {
        match v.as_str() {
            Some(s) => match s.split_once(':') {
                Some(("prefix", v)) => Ok(ToolVersionType::Prefix(v.into())),
                Some(("path", v)) => Ok(ToolVersionType::Path(v.into())),
                Some(("ref", v)) => Ok(ToolVersionType::Ref(v.into())),
                Some((unknown, v)) => {
                    parse_error!(format!("{}.{}", key, unknown), v, "prefix, path, or ref")?
                }
                None => Ok(ToolVersionType::Version(s.into())),
            },
            _ => parse_error!(key, v, "string")?,
        }
    }

    fn parse_settings(&self, key: &str, v: &Item) -> Result<SettingsBuilder> {
        let mut settings = SettingsBuilder::default();

        match v.as_table_like() {
            Some(table) => {
                for (config_key, v) in table.iter() {
                    let k = format!("{}.{}", key, config_key);
                    match config_key.to_lowercase().as_str() {
                        "experimental" => settings.experimental = Some(self.parse_bool(&k, v)?),
                        "missing_runtime_behavior" => {
                            settings.missing_runtime_behavior =
                                Some(self.parse_missing_runtime_behavior(&k, v)?)
                        }
                        "legacy_version_file" => {
                            settings.legacy_version_file = Some(self.parse_bool(&k, v)?)
                        }
                        "always_keep_download" => {
                            settings.always_keep_download = Some(self.parse_bool(&k, v)?)
                        }
                        "plugin_autoupdate_last_check_duration" => {
                            settings.plugin_autoupdate_last_check_duration =
                                Some(self.parse_duration_minutes(&k, v)?)
                        }
                        "verbose" => settings.verbose = Some(self.parse_bool(&k, v)?),
                        "asdf_compat" => settings.asdf_compat = Some(self.parse_bool(&k, v)?),
                        "jobs" => settings.jobs = Some(self.parse_usize(&k, v)?),
                        "shorthands_file" => {
                            settings.shorthands_file = Some(self.parse_path(&k, v)?)
                        }
                        "disable_default_shorthands" => {
                            settings.disable_default_shorthands = Some(self.parse_bool(&k, v)?)
                        }
                        "log_level" => settings.log_level = Some(self.parse_log_level(&k, v)?),
                        "shims_dir" => settings.shims_dir = Some(self.parse_path(&k, v)?),
                        "raw" => settings.raw = Some(self.parse_bool(&k, v)?),
                        _ => parse_error!(k, v, "setting")?,
                    };
                }
            }
            None => parse_error!("settings", v, "table")?,
        }

        Ok(settings)
    }

    fn set_alias(&mut self, plugin: &str, from: &str, to: &str) {
        self.alias
            .entry(plugin.into())
            .or_default()
            .insert(from.into(), to.into());
        self.doc
            .entry("alias")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .entry(plugin)
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .insert(from, value(to));
    }

    pub fn remove_alias(&mut self, plugin: &str, from: &str) -> Result<()> {
        if let Some(aliases) = self.doc.get_mut("alias").and_then(|v| v.as_table_mut()) {
            if let Some(plugin_aliases) = aliases.get_mut(plugin).and_then(|v| v.as_table_mut()) {
                self.alias.get_mut(plugin).unwrap().remove(from);
                plugin_aliases.remove(from);
                if plugin_aliases.is_empty() {
                    aliases.remove(plugin);
                    self.alias.remove(plugin);
                }
            }
            if aliases.is_empty() {
                self.doc.as_table_mut().remove("alias");
            }
        }
        Ok(())
    }

    fn parse_duration_minutes(&self, k: &str, v: &Item) -> Result<Duration> {
        match v.as_value() {
            Some(Value::String(s)) => Ok(humantime::parse_duration(s.value())?),
            Some(Value::Integer(i)) => Ok(Duration::from_secs(*i.value() as u64 * 60)),
            _ => Err(eyre!("expected {k} to be an integer, got: {v}")),
        }
    }

    fn parse_bool(&self, k: &str, v: &Item) -> Result<bool> {
        match v.as_value().map(|v| v.as_bool()) {
            Some(Some(v)) => Ok(v),
            _ => parse_error!(k, v, "boolean")?,
        }
    }

    fn parse_usize(&self, k: &str, v: &Item) -> Result<usize> {
        match v.as_value().map(|v| v.as_integer()) {
            Some(Some(v)) => Ok(v as usize),
            _ => Err(eyre!("expected {k} to be an integer, got: {v}")),
        }
    }

    fn parse_path(&self, k: &str, v: &Item) -> Result<PathBuf> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => Ok(v.into()),
            _ => Err(eyre!("expected {k} to be a path, got: {v}")),
        }
    }

    fn parse_string(&self, k: &str, v: &Item) -> Result<String> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => Ok(v.into()),
            _ => Err(eyre!("expected {k} to be a string, got: {v}")),
        }
    }

    fn parse_missing_runtime_behavior(&self, k: &str, v: &Item) -> Result<MissingRuntimeBehavior> {
        let v = self.parse_string("missing_runtime_behavior", v)?;
        match v.to_lowercase().as_str() {
            "warn" => Ok(MissingRuntimeBehavior::Warn),
            "ignore" => Ok(MissingRuntimeBehavior::Ignore),
            "prompt" => Ok(MissingRuntimeBehavior::Prompt),
            "autoinstall" => Ok(MissingRuntimeBehavior::AutoInstall),
            _ => Err(eyre!(
                "expected {k} to be one of: 'warn', 'ignore', 'prompt', 'autoinstall'. Got: {v}"
            )),
        }
    }

    fn parse_log_level(&self, k: &str, v: &Item) -> Result<LevelFilter> {
        let level = self.parse_string(k, v)?.parse()?;
        Ok(level)
    }
}

impl Display for RtxToml {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.dump())
    }
}

impl ConfigFile for RtxToml {
    fn get_type(&self) -> ConfigFileType {
        ConfigFileType::RtxToml
    }

    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn plugins(&self) -> HashMap<PluginName, String> {
        self.plugins.clone()
    }

    fn env(&self) -> HashMap<String, String> {
        self.env.clone()
    }

    fn remove_plugin(&mut self, plugin: &PluginName) {
        self.toolset.versions.remove(plugin);
        if let Some(tools) = self.doc.get_mut("tools") {
            if let Some(tools) = tools.as_table_like_mut() {
                tools.remove(plugin);
                if tools.is_empty() {
                    self.doc.as_table_mut().remove("tools");
                }
            }
        }
    }

    fn replace_versions(&mut self, plugin_name: &PluginName, versions: &[String]) {
        if let Some(plugin) = self.toolset.versions.get_mut(plugin_name) {
            plugin.versions = versions
                .iter()
                .map(|v| ToolVersion::new(plugin_name.clone(), ToolVersionType::Version(v.clone())))
                .collect();
        }
        let tools = self
            .doc
            .entry("tools")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();

        if versions.len() == 1 {
            tools.insert(plugin_name, value(versions[0].clone()));
        } else {
            let mut arr = Array::new();
            for v in versions {
                arr.push(v);
            }
            tools.insert(plugin_name, Item::Value(Value::Array(arr)));
        }
    }

    fn save(&self) -> Result<()> {
        let contents = self.dump();
        Ok(fs::write(&self.path, contents)?)
    }

    fn dump(&self) -> String {
        self.doc.to_string()
    }

    fn to_toolset(&self) -> &Toolset {
        &self.toolset
    }

    fn settings(&self) -> SettingsBuilder {
        self.settings.clone()
    }

    fn aliases(&self) -> AliasMap {
        self.alias.clone()
    }
}

#[allow(dead_code)] // TODO: remove
const ENV_SUGGESTION: &str = r#"
[env]
FOO = "bar"
"#;

#[cfg(test)]
mod tests {
    use indoc::formatdoc;
    use insta::{assert_debug_snapshot, assert_display_snapshot};

    use crate::dirs;

    use super::*;

    #[test]
    fn test_fixture() {
        let cf = RtxToml::from_file(&dirs::HOME.join("fixtures/.rtx.toml")).unwrap();

        assert_debug_snapshot!(cf.env());
        assert_debug_snapshot!(cf.settings());
        assert_debug_snapshot!(cf.plugins());
        assert_debug_snapshot!(cf.toolset);
        assert_debug_snapshot!(cf.alias);

        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_env() {
        let cf = RtxToml::from_str(formatdoc! {r#"
        [env]
        foo="bar"
        "#})
        .unwrap();

        assert_debug_snapshot!(cf.env(), @r###"
        {
            "foo": "bar",
        }
        "###);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_set_alias() {
        let mut cf = RtxToml::from_str(formatdoc! {r#"
        [alias.nodejs]
        16 = "16.0.0"
        18 = "18.0.0"
        "#})
        .unwrap();

        cf.set_alias("nodejs", "18", "18.0.1");
        cf.set_alias("nodejs", "20", "20.0.0");
        cf.set_alias("python", "3.10", "3.10.0");

        assert_debug_snapshot!(cf.alias);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_remove_alias() {
        let mut cf = RtxToml::from_str(formatdoc! {r#"
        [alias.nodejs]
        16 = "16.0.0"
        18 = "18.0.0"

        [alias.python]
        "3.10" = "3.10.0"
        "#})
        .unwrap();
        cf.remove_alias("nodejs", "16").unwrap();
        cf.remove_alias("python", "3.10").unwrap();

        assert_debug_snapshot!(cf.alias);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_replace_versions() {
        let mut cf = RtxToml::from_str(formatdoc! {r#"
        [tools]
        nodejs = ["16.0.0", "18.0.0"]
        "#})
        .unwrap();
        cf.replace_versions(
            &PluginName::from("nodejs"),
            &["16.0.1".into(), "18.0.1".into()],
        );

        assert_debug_snapshot!(cf.toolset);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_remove_plugin() {
        let mut cf = RtxToml::from_str(formatdoc! {r#"
        [tools]
        nodejs = ["16.0.0", "18.0.0"]
        "#})
        .unwrap();
        cf.remove_plugin(&PluginName::from("nodejs"));

        assert_debug_snapshot!(cf.toolset);
        assert_display_snapshot!(cf);
    }
}
