use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{eyre, Context};
use color_eyre::{Result, Section, SectionExt};
use indexmap::IndexMap;
use toml::Value;

use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::config::settings::{MissingRuntimeBehavior, Settings, SettingsBuilder};
use crate::config::AliasMap;
use crate::config::PluginSource;
use crate::plugins::PluginName;

const ENV_SUGGESTION: &str = r#"
[env]
FOO = "bar"
"#;

#[derive(Debug, Default)]
pub struct RTXFile {
    pub path: PathBuf,
    pub plugins: IndexMap<String, Plugin>,
    pub env: HashMap<String, String>,
    edit: Option<toml_edit::Document>,
    settings: SettingsBuilder,
}

#[derive(Debug, PartialEq, Eq, Hash, Default)]
pub struct Plugin {
    pub name: String,
    pub versions: Vec<String>,
}

impl RTXFile {
    pub fn init(filename: &Path) -> RTXFile {
        RTXFile {
            path: filename.to_path_buf(),
            ..Default::default()
        }
    }

    pub fn from_file(filename: &Path) -> Result<RTXFile> {
        trace!("parsing rtxrc: {}", filename.display());
        let body = fs::read_to_string(filename).suggestion("ensure file exists and can be read")?;
        let mut rf = RTXFile::from_str(body).wrap_err("error parsing toml")?;
        rf.path = filename.into();

        Ok(rf)
    }

    pub fn from_str(s: String) -> Result<RTXFile> {
        let mut rf = RTXFile::default();

        match s
            .parse::<Value>()
            .suggestion("Ensure .rtxrc is valid TOML.")?
        {
            Value::Table(table) => {
                for (k, v) in table.iter() {
                    rf.parse_toplevel_key(k, v)
                        .with_section(|| format!("[{k}]\n{v}").header("TOML:"))?;
                }
                Ok(())
            }
            _ => Err(eyre!("Invalid TOML: {}", s)),
        }?;

        Ok(rf)
    }

    pub fn settings(&self) -> Settings {
        self.settings.build()
    }

    fn parse_toplevel_key(&mut self, k: &String, v: &Value) -> Result<()> {
        match k.to_lowercase().as_str() {
            "env" => self.parse_env(v).with_suggestion(|| ENV_SUGGESTION)?,
            "missing_runtime_behavior" => {
                self.settings.missing_runtime_behavior =
                    Some(self.parse_missing_runtime_behavior(v)?)
            }
            "legacy_version_file" => {
                self.settings.legacy_version_file = Some(self.parse_bool(k, v)?)
            }
            "always_keep_download" => {
                self.settings.always_keep_download = Some(self.parse_bool(k, v)?)
            }
            "plugin_autoupdate_last_check_duration" => {
                self.settings.plugin_autoupdate_last_check_duration =
                    Some(self.parse_duration_minutes(k, v)?)
            }
            "plugin_repository_last_check_duration" => {
                self.settings.plugin_repository_last_check_duration =
                    Some(self.parse_duration_minutes(k, v)?)
            }
            "disable_plugin_short_name_repository" => {
                self.settings.disable_plugin_short_name_repository = Some(self.parse_bool(k, v)?)
            }
            "verbose" => self.settings.verbose = Some(self.parse_bool(k, v)?),
            "alias" => self.settings.aliases = Some(self.parse_aliases(v)?),
            "get_path" => {}
            _ => self.parse_plugin(k, v)?,
        };
        Ok(())
    }

    fn parse_env(&mut self, v: &Value) -> Result<()> {
        match v {
            Value::Table(table) => {
                for (k, v) in table.iter() {
                    match v {
                        Value::String(s) => {
                            self.env.insert(k.into(), s.into());
                        }
                        _ => Err(eyre!("expected [env] value to be a string, got: {v}"))?,
                    }
                }
                Ok(())
            }
            _ => Err(eyre!("expected [env] to be a table, got: {v}")),
        }
    }

    fn parse_plugin(&mut self, k: &String, v: &Value) -> Result<()> {
        let versions = self.parse_plugin_versions(v)?;
        self.plugins.insert(
            k.into(),
            Plugin {
                name: k.into(),
                versions,
            },
        );
        Ok(())
    }

    fn parse_plugin_versions(&self, v: &Value) -> Result<Vec<String>> {
        match v {
            Value::String(s) => Ok(vec![s.to_string()]),
            Value::Array(a) => a
                .iter()
                .map(|v| match v {
                    Value::String(s) => Ok(s.to_string()),
                    _ => Err(eyre!("Invalid TOML: {}", v)),
                })
                .collect(),
            Value::Table(t) => t
                .iter()
                // TODO: from_file value
                .map(|(k, _v)| Ok(k.into()))
                .collect(),
            _ => Err(eyre!(
                "expected plugin to be a string, array, or table, got: {v}"
            )),
        }
    }

    fn parse_duration_minutes(&self, k: &str, v: &Value) -> Result<Duration> {
        match v {
            Value::Integer(i) => {
                let duration = Duration::from_secs(*i as u64 * 60);
                Ok(duration)
            }
            _ => Err(eyre!("expected {k} to be an integer, got: {v}")),
        }
    }

    fn parse_bool(&self, k: &str, v: &Value) -> Result<bool> {
        match v {
            Value::Boolean(v) => Ok(*v),
            _ => Err(eyre!("expected {k} to be a boolean, got: {v}")),
        }
    }

    fn parse_string(&self, k: &str, v: &Value) -> Result<String> {
        match v {
            Value::String(v) => Ok(v.clone()),
            _ => Err(eyre!("expected {k} to be a string, got: {v}")),
        }
    }

    fn parse_missing_runtime_behavior(&mut self, v: &Value) -> Result<MissingRuntimeBehavior> {
        let v = self.parse_string("missing_runtime_behavior", v)?;
        match v.to_lowercase().as_str() {
            "warn" => Ok(MissingRuntimeBehavior::Warn),
            "ignore" => Ok(MissingRuntimeBehavior::Ignore),
            "prompt" => Ok(MissingRuntimeBehavior::Prompt),
            "autoinstall" => Ok(MissingRuntimeBehavior::AutoInstall),
            _ => Err(eyre!("expected missing_runtime_behavior to be one of: 'warn', 'ignore', 'prompt', 'autoinstall'. Got: {v}")),
        }
    }

    fn parse_aliases(&mut self, v: &Value) -> Result<AliasMap> {
        match v {
            Value::Table(table) => {
                let mut aliases = AliasMap::new();
                for (plugin, table) in table.iter() {
                    let plugin_aliases = aliases.entry(plugin.into()).or_default();
                    match table {
                        Value::Table(table) => {
                            for (from, to) in table.iter() {
                                match to {
                                    Value::String(s) => {
                                        plugin_aliases.insert(from.into(), s.into());
                                    }
                                    _ => Err(eyre!(
                                        "expected [aliases] value to be a string, got: {v}"
                                    ))?,
                                }
                            }
                        }
                        _ => Err(eyre!("expected [aliases] value to be a table, got: {v}"))?,
                    }
                }
                Ok(aliases)
            }
            _ => Err(eyre!("expected [aliases] to be a table, got: {v}")),
        }
    }

    fn get_or_create_edit(&mut self) -> &mut toml_edit::Document {
        if self.edit.is_none() {
            if !self.path.exists() {
                let dir = self.path.parent().unwrap();
                fs::create_dir_all(dir).expect("could not create directory");
                fs::write(&self.path, "").expect("could not create new config.toml file");
            }
            let body = fs::read_to_string(&self.path)
                .suggestion("ensure file exists and can be read")
                .unwrap();
            self.edit = Some(body.parse::<toml_edit::Document>().unwrap());
        }
        self.edit.as_mut().unwrap()
    }

    fn get_edit(&self) -> Result<toml_edit::Document> {
        match &self.edit {
            Some(doc) => Ok(doc.clone()),
            None => {
                let body = fs::read_to_string(&self.path)
                    .suggestion("ensure file exists and can be read")
                    .unwrap();
                Ok(body.parse::<toml_edit::Document>()?)
            }
        }
    }

    pub fn update_setting<V: Into<toml_edit::Value>>(&mut self, key: &str, value: V) {
        let doc = self.get_or_create_edit();
        let key = key.split('.').collect::<Vec<&str>>();
        let mut table = doc.as_table_mut();
        for (i, k) in key.iter().enumerate() {
            if i == key.len() - 1 {
                table[k] = toml_edit::value(value);
                break;
            } else {
                table = table
                    .entry(k)
                    .or_insert(toml_edit::table())
                    .as_table_mut()
                    .unwrap();
            }
        }
    }

    pub fn remove_setting(&mut self, key: &str) {
        let doc = self.get_or_create_edit();
        let key = key.split('.').collect::<Vec<&str>>();
        let mut table = doc.as_table_mut();
        for (i, k) in key.iter().enumerate() {
            if i == key.len() - 1 {
                table.remove(k);
                break;
            } else {
                table = table
                    .entry(k)
                    .or_insert(toml_edit::table())
                    .as_table_mut()
                    .unwrap();
            }
        }
    }

    pub fn set_alias(&mut self, plugin: &str, from: &str, to: &str) {
        let doc = self.get_or_create_edit();
        let aliases = doc
            .as_table_mut()
            .entry("aliases")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        let plugin_aliases = aliases
            .entry(plugin)
            .or_insert(toml_edit::table())
            .as_table_mut()
            .unwrap();
        plugin_aliases[from] = toml_edit::value(to);
    }
}

impl Display for RTXFile {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.dump())
    }
}

impl ConfigFile for RTXFile {
    fn get_type(&self) -> ConfigFileType {
        ConfigFileType::RtxRc
    }

    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn source(&self) -> PluginSource {
        PluginSource::RtxRc(self.path.clone())
    }

    fn plugins(&self) -> IndexMap<String, Vec<String>> {
        self.plugins
            .iter()
            .map(|(k, v)| (k.clone(), v.versions.clone()))
            .collect()
    }

    fn env(&self) -> HashMap<String, String> {
        self.env.clone()
    }

    fn remove_plugin(&mut self, plugin: &PluginName) {
        self.plugins.remove(plugin);
        self.get_or_create_edit().as_table_mut().remove(plugin);
    }

    fn add_version(&mut self, plugin: &PluginName, version: &str) {
        self.plugins
            .entry(plugin.into())
            .or_default()
            .versions
            .push(version.to_string());

        self.get_or_create_edit()
            .entry(plugin)
            .or_insert_with(toml_edit::array)
            .as_array_mut()
            .unwrap()
            .push(version);
    }

    fn replace_versions(&mut self, plugin_name: &PluginName, versions: &[String]) {
        let plugin = self.plugins.entry(plugin_name.into()).or_default();
        plugin.versions.clear();
        self.get_or_create_edit()
            .entry(plugin_name)
            .or_insert_with(toml_edit::array)
            .as_array_mut()
            .unwrap()
            .clear();
        for version in versions {
            self.add_version(plugin_name, version);
        }
    }

    fn save(&self) -> Result<()> {
        let contents = self.dump();
        Ok(fs::write(&self.path, contents)?)
    }

    fn dump(&self) -> String {
        self.get_edit().expect("unable to parse toml").to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::io::*;

    use indoc::writedoc;
    use insta::assert_display_snapshot;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_from_str() {
        let cf = RTXFile::from_str(
            r#"
nodejs = ["18.0.0", "20.0.0"]
"#
            .to_string(),
        )
        .unwrap();

        assert_eq!(cf.plugins.len(), 1);
        assert!(cf.plugins.contains_key("nodejs"));
        assert_eq!(cf.plugins["nodejs"].versions, vec!["18.0.0", "20.0.0"]);
    }

    #[test]
    fn test_parse() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writedoc!(
            f,
            r#"
            nodejs = ["18.0.0", "20.0.0"] # comments
        "#
        )
        .unwrap();
        let cf = RTXFile::from_file(f.path()).unwrap();

        assert_eq!(cf.plugins.len(), 1);
        assert!(cf.plugins.contains_key("nodejs"));
        assert_eq!(cf.plugins["nodejs"].versions, vec!["18.0.0", "20.0.0"]);
        assert_display_snapshot!(cf, @r###"
        nodejs = ["18.0.0", "20.0.0"] # comments
        "###);
    }

    #[test]
    fn test_single_version() {
        let cf = RTXFile::from_str(
            r#"
nodejs = "18.0.0"
"#
            .to_string(),
        )
        .unwrap();

        assert_eq!(cf.plugins.len(), 1);
        assert!(cf.plugins.contains_key("nodejs"));
        assert_eq!(cf.plugins["nodejs"].versions, vec!["18.0.0"]);
    }

    #[test]
    fn test_plugin_hash() {
        let cf = RTXFile::from_str(
            r#"
[nodejs.20]
packages = ["urchin"]
"#
            .to_string(),
        )
        .unwrap();

        assert_eq!(cf.plugins.len(), 1);
        assert!(cf.plugins.contains_key("nodejs"));
        assert_eq!(cf.plugins["nodejs"].versions, vec!["20"]);
    }

    #[test]
    fn test_env() {
        let cf = RTXFile::from_str(
            r#"
[env]
foo="bar"
"#
            .to_string(),
        )
        .unwrap();

        assert_eq!(cf.env["foo"], "bar");
    }

    #[test]
    fn test_invalid_env() {
        let err = RTXFile::from_str(
            r#"
env=[1,2,3]
"#
            .to_string(),
        )
        .unwrap_err();

        assert_display_snapshot!(err, @"expected [env] to be a table, got: [1, 2, 3]");
    }

    #[test]
    fn test_invalid_env_value() {
        let err = RTXFile::from_str(
            r#"
[env]
foo=[1,2,3]
"#
            .to_string(),
        )
        .unwrap_err();

        assert_display_snapshot!(err, @"expected [env] value to be a string, got: [1, 2, 3]");
    }

    #[test]
    fn test_invalid_plugin() {
        let err = RTXFile::from_str(
            r#"
nodejs=1
"#
            .to_string(),
        )
        .unwrap_err();

        assert_display_snapshot!(err, @"expected plugin to be a string, array, or table, got: 1");
    }

    #[test]
    fn test_invalid_plugin_2() {
        let err = RTXFile::from_str(
            r#"
nodejs=[true]
"#
            .to_string(),
        )
        .unwrap_err();

        assert_display_snapshot!(err, @"Invalid TOML: true");
    }

    #[test]
    fn test_update_setting() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writedoc!(
            f,
            r#"
            legacy_version_file = true
            [aliases.nodejs]
            18 = "18.0.0"
        "#
        )
        .unwrap();
        let mut cf = RTXFile::from_file(f.path()).unwrap();
        cf.update_setting("legacy_version_file", false);
        cf.update_setting("something_else", "foo");
        cf.update_setting("something.nested.very.deeply", 123);
        cf.update_setting("aliases.nodejs.20", "20.0.0");
        cf.update_setting("aliases.python.3", "3.9.0");
        assert_display_snapshot!(cf.dump(), @r###"
        legacy_version_file = false
        something_else = "foo"
        [aliases.nodejs]
        18 = "18.0.0"
        20 = "20.0.0"

        [aliases.python]
        3 = "3.9.0"

        [something]

        [something.nested]

        [something.nested.very]
        deeply = 123
        "###);
    }

    #[test]
    fn test_remove_setting() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writedoc!(
            f,
            r#"
        [something]

        [something.nested]
        other = "foo"

        [something.nested.very]
        deeply = 123
        "#
        )
        .unwrap();
        let mut cf = RTXFile::from_file(f.path()).unwrap();
        cf.remove_setting("something.nested.other");
        assert_display_snapshot!(cf.dump(), @r###"
        [something]

        [something.nested]

        [something.nested.very]
        deeply = 123
        "###);
    }

    #[test]
    fn test_set_alias() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writedoc!(
            f,
            r#"
            [aliases.nodejs]
            16 = "16.0.0"
            18 = "18.0.0"
        "#
        )
        .unwrap();
        let mut cf = RTXFile::from_file(f.path()).unwrap();
        cf.set_alias("nodejs", "18", "18.0.1");
        cf.set_alias("nodejs", "20", "20.0.0");
        cf.set_alias("python", "3.10", "3.10.0");
        assert_display_snapshot!(cf.dump(), @r###"
        [aliases.nodejs]
        16 = "16.0.0"
        18 = "18.0.1"
        20 = "20.0.0"

        [aliases.python]
        "3.10" = "3.10.0"
        "###);
    }

    #[test]
    fn test_edit_when_file_does_not_exist() {
        let mut cf = RTXFile::from_str("".to_string()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        cf.path = dir.path().join("subdir").join("does-not-exist.toml");
        cf.set_alias("nodejs", "18", "18.0.1");
        cf.save().unwrap();
    }
}
