use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use color_eyre::eyre::eyre;
use color_eyre::{Result, Section};
use confique::Partial;
use console::style;
use eyre::WrapErr;
use tera::Context;
use toml_edit::{table, value, Array, Document, Item, Table, Value};

use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::config::settings::SettingsPartial;
use crate::config::{config_file, AliasMap};
use crate::errors::Error::UntrustedConfig;
use crate::file::{create_dir_all, display_path};
use crate::plugins::{unalias_plugin, PluginName};
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::toolset::{
    ToolSource, ToolVersionList, ToolVersionOptions, ToolVersionRequest, Toolset,
};
use crate::ui::prompt;
use crate::{dirs, env, file, parse_error};

#[derive(Default)]
pub struct RtxToml {
    context: Context,
    path: PathBuf,
    toolset: Toolset,
    env_file: Option<PathBuf>,
    env: HashMap<String, String>,
    env_remove: Vec<String>,
    path_dirs: Vec<PathBuf>,
    settings: Table,
    alias: AliasMap,
    doc: Document,
    plugins: HashMap<String, String>,
    is_trusted: Mutex<RefCell<bool>>,
}

impl RtxToml {
    pub fn init(path: &Path, is_trusted: bool) -> Self {
        let mut context = BASE_CONTEXT.clone();
        context.insert("config_root", path.parent().unwrap().to_str().unwrap());
        Self {
            path: path.to_path_buf(),
            context,
            is_trusted: Mutex::new(RefCell::new(is_trusted)),
            toolset: Toolset {
                source: Some(ToolSource::RtxToml(path.to_path_buf())),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn from_file(path: &Path, is_trusted: bool) -> Result<Self> {
        trace!("parsing: {}", path.display());
        let mut rf = Self::init(path, is_trusted);
        let body = file::read_to_string(path).suggestion("ensure file exists and can be read")?;
        rf.parse(&body)?;
        trace!("{rf}");
        Ok(rf)
    }

    fn parse(&mut self, s: &str) -> Result<()> {
        let doc: Document = s.parse().suggestion("ensure file is valid TOML")?;
        for (k, v) in doc.iter() {
            match k {
                "dotenv" => self.parse_env_file(k, v)?,
                "env_file" => self.parse_env_file(k, v)?,
                "env_path" => self.path_dirs = self.parse_path_env(k, v)?,
                "env" => self.parse_env(k, v)?,
                "alias" => self.alias = self.parse_alias(k, v)?,
                "tools" => self.toolset = self.parse_toolset(k, v)?,
                "settings" => self.settings = self.parse_settings(k, v)?,
                "plugins" => self.plugins = self.parse_plugins(k, v)?,
                _ => Err(eyre!("unknown key: {}", k))?,
            }
        }
        self.doc = doc;
        Ok(())
    }

    fn parse_env_file(&mut self, k: &str, v: &Item) -> Result<()> {
        self.trust_check()?;
        match v.as_str() {
            Some(filename) => {
                let path = self.path.parent().unwrap().join(filename);
                let dotenv = dotenvy::from_path_iter(&path).wrap_err_with(|| {
                    eyre!("failed to parse dotenv file: {}", display_path(&path))
                })?;
                for item in dotenv {
                    let (k, v) = item?;
                    self.env.insert(k, v);
                }
                self.env_file = Some(path);
            }
            _ => parse_error!(k, v, "string")?,
        }
        Ok(())
    }

    fn parse_env(&mut self, key: &str, v: &Item) -> Result<()> {
        self.trust_check()?;
        let mut v = v.clone();
        if let Some(table) = v.as_table_like_mut() {
            if table.contains_key("PATH") {
                return Err(eyre!("use 'env_path' instead of 'env.PATH'"));
            }
        }
        match v.as_table_like() {
            Some(table) => {
                for (k, v) in table.iter() {
                    let key = format!("{}.{}", key, k);
                    let k = self.parse_template(&key, k)?;
                    if let Some(v) = v.as_str() {
                        let v = self.parse_template(&key, v)?;
                        self.env.insert(k, v);
                    } else if let Some(v) = v.as_bool() {
                        if !v {
                            self.env_remove.push(k);
                        }
                    } else {
                        parse_error!(key, v, "string or bool")?;
                    }
                }
            }
            _ => parse_error!(key, v, "table")?,
        }
        Ok(())
    }

    fn parse_path_env(&self, k: &str, v: &Item) -> Result<Vec<PathBuf>> {
        self.trust_check()?;
        match v.as_array() {
            Some(array) => {
                let mut path = Vec::new();
                let config_root = self.path.parent().unwrap().to_path_buf();
                for v in array {
                    match v.as_str() {
                        Some(s) => {
                            let s = self.parse_template(k, s)?;
                            let s = match s.strip_prefix("./") {
                                Some(s) => config_root.join(s),
                                None => match s.strip_prefix("~/") {
                                    Some(s) => dirs::HOME.join(s),
                                    None => s.into(),
                                },
                            };
                            path.push(s);
                        }
                        _ => parse_error!(k, v, "string")?,
                    }
                }
                Ok(path)
            }
            _ => parse_error!(k, v, "array")?,
        }
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
                                        let from = self.parse_template(&k, from)?;
                                        let s = self.parse_template(&k, s)?;
                                        plugin_aliases.insert(from, s);
                                    }
                                    _ => parse_error!(format!("{}.{}", k, from), to, "string")?,
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

    fn parse_plugins(&self, key: &str, v: &Item) -> Result<HashMap<String, String>> {
        self.trust_check()?;
        self.parse_hashmap(key, v)
    }

    fn parse_hashmap(&self, key: &str, v: &Item) -> Result<HashMap<String, String>> {
        match v.as_table_like() {
            Some(table) => {
                let mut env = HashMap::new();
                for (k, v) in table.iter() {
                    match v.as_str() {
                        Some(s) => {
                            let k = self.parse_template(key, k)?;
                            let s = self.parse_template(key, s)?;
                            env.insert(k, s);
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
        let mut toolset = Toolset::new(self.toolset.source.clone().unwrap());

        match v.as_table_like() {
            Some(table) => {
                for (plugin, v) in table.iter() {
                    let k = format!("{}.{}", key, plugin);
                    let plugin_name = unalias_plugin(plugin).to_string();
                    let tvl = self.parse_tool_version_list(&k, v, &plugin_name)?;
                    toolset.versions.insert(plugin_name, tvl);
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
        let mut tool_version_list = ToolVersionList::new(plugin_name.to_string(), source);

        match v {
            Item::ArrayOfTables(v) => {
                for table in v.iter() {
                    for (tool, v) in table.iter() {
                        let k = format!("{}.{}", key, tool);
                        let (tvr, opts) = self.parse_tool_version(&k, v, plugin_name)?;
                        tool_version_list.requests.push((tvr, opts));
                    }
                }
            }
            v => match v.as_array() {
                Some(v) => {
                    for v in v.iter() {
                        let item = Item::Value(v.clone());
                        let (tvr, opts) = self.parse_tool_version(key, &item, plugin_name)?;
                        tool_version_list.requests.push((tvr, opts));
                    }
                }
                _ => {
                    tool_version_list.requests.push(self.parse_tool_version(
                        key,
                        v,
                        plugin_name,
                    )?);
                }
            },
        }

        for (tvr, _) in &tool_version_list.requests {
            if let ToolVersionRequest::Path(_, _) = tvr {
                // "path:" can be dangerous to run automatically
                self.trust_check()?;
            }
        }

        Ok(tool_version_list)
    }

    fn parse_tool_version(
        &self,
        key: &str,
        v: &Item,
        plugin_name: &PluginName,
    ) -> Result<(ToolVersionRequest, ToolVersionOptions)> {
        let mut tv = ToolVersionRequest::new(plugin_name.clone(), "system");
        let mut opts = ToolVersionOptions::default();

        match v.as_table_like() {
            Some(table) => {
                if let Some(v) = table.get("version") {
                    match v {
                        Item::Value(v) => {
                            tv = self.parse_tool_version_request(key, v, plugin_name)?;
                        }
                        _ => parse_error!(format!("{}.version", key), v, "string")?,
                    }
                } else if let Some(path) = table.get("path") {
                    match path.as_str() {
                        Some(s) => {
                            let s = self.parse_template(key, s)?;
                            tv = ToolVersionRequest::Path(plugin_name.clone(), s.into());
                        }
                        _ => parse_error!(format!("{}.path", key), v, "string")?,
                    }
                } else if let Some(prefix) = table.get("prefix") {
                    match prefix.as_str() {
                        Some(s) => {
                            let s = self.parse_template(key, s)?;
                            tv = ToolVersionRequest::Prefix(plugin_name.clone(), s);
                        }
                        _ => parse_error!(format!("{}.prefix", key), v, "string")?,
                    }
                } else if let Some(r) = table.get("ref") {
                    match r.as_str() {
                        Some(s) => {
                            let s = self.parse_template(key, s)?;
                            tv = ToolVersionRequest::Ref(plugin_name.clone(), s);
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
                    let s = if let Some(s) = v.as_str() {
                        self.parse_template(key, s)?
                    } else if let Some(b) = v.as_bool() {
                        b.to_string()
                    } else {
                        parse_error!(key, v, "string or bool")?
                    };
                    opts.insert(k.into(), s);
                }
            }
            _ => match v {
                Item::Value(v) => {
                    tv = self.parse_tool_version_request(key, v, plugin_name)?;
                }
                _ => parse_error!(key, v, "value")?,
            },
        }

        Ok((tv, opts))
    }

    fn parse_tool_version_request(
        &self,
        key: &str,
        v: &Value,
        plugin_name: &PluginName,
    ) -> Result<ToolVersionRequest> {
        match v.as_str() {
            Some(s) => {
                let s = self.parse_template(key, s)?;
                Ok(ToolVersionRequest::new(plugin_name.clone(), &s))
            }
            _ => parse_error!(key, v, "string")?,
        }
    }

    fn parse_settings(&self, _k: &str, v: &Item) -> Result<Table> {
        match v.as_table() {
            Some(table) => Ok(table.clone()),
            None => parse_error!("settings", v, "table"),
        }
    }

    pub fn set_alias(&mut self, plugin: &str, from: &str, to: &str) {
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

    pub fn remove_alias(&mut self, plugin: &str, from: &str) {
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
    }

    fn parse_bool(&self, k: &str, v: &Item) -> Result<bool> {
        match v.as_value().map(|v| v.as_bool()) {
            Some(Some(v)) => Ok(v),
            _ => parse_error!(k, v, "boolean")?,
        }
    }

    fn parse_string(&self, k: &str, v: &Item) -> Result<String> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => Ok(v.to_string()),
            _ => parse_error!(k, v, "string")?,
        }
    }

    fn parse_usize(&self, k: &str, v: &Item) -> Result<usize> {
        match v.as_value().map(|v| v.as_integer()) {
            Some(Some(v)) => Ok(v as usize),
            _ => parse_error!(k, v, "usize")?,
        }
    }

    fn parse_path(&self, k: &str, v: &Item) -> Result<PathBuf> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => {
                let v = self.parse_template(k, v)?;
                Ok(v.into())
            }
            _ => parse_error!(k, v, "path")?,
        }
    }

    fn parse_paths(&self, k: &str, v: &Item) -> Result<Vec<PathBuf>> {
        match v.as_value().map(|v| v.as_array()) {
            Some(Some(v)) => {
                let mut paths = vec![];
                for (i, v) in v.iter().enumerate() {
                    let k = format!("{}.{}", k, i);
                    match v.as_str() {
                        Some(v) => {
                            let v = self.parse_template(&k, v)?;
                            paths.push(v.into());
                        }
                        _ => parse_error!(k, v, "path")?,
                    }
                }
                Ok(paths)
            }
            _ => parse_error!(k, v, "array of paths")?,
        }
    }

    fn parse_string_array(&self, k: &String, v: &Item) -> Result<Vec<String>> {
        match v
            .as_array()
            .map(|v| v.iter().map(|v| v.as_str().unwrap().to_string()).collect())
        {
            Some(v) => Ok(v),
            _ => parse_error!(k, v, "array of strings")?,
        }
    }

    pub fn update_setting<V: Into<Value>>(&mut self, key: &str, value: V) {
        let key = key.split('.').collect::<Vec<&str>>();
        let mut settings = self
            .doc
            .entry("settings")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap();
        for (i, k) in key.iter().enumerate() {
            if i == key.len() - 1 {
                settings.insert(k, toml_edit::value(value));
                break;
            } else {
                settings = settings
                    .entry(k)
                    .or_insert(toml_edit::table())
                    .as_table_mut()
                    .unwrap();
            }
        }
    }

    pub fn remove_setting(&mut self, key: &str) {
        let mut settings = self
            .doc
            .entry("settings")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap();
        let key = key.split('.').collect::<Vec<&str>>();
        for (i, k) in key.iter().enumerate() {
            if i == key.len() - 1 {
                settings.remove(k);
                break;
            } else {
                settings = settings
                    .entry(k)
                    .or_insert(toml_edit::table())
                    .as_table_mut()
                    .unwrap();
            }
        }
        if settings.is_empty() {
            self.doc.as_table_mut().remove("settings");
        }
    }

    pub fn update_env<V: Into<Value>>(&mut self, key: &str, value: V) {
        let env_tbl = self
            .doc
            .entry("env")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap();
        env_tbl.insert(key, toml_edit::value(value));
    }

    pub fn remove_env(&mut self, key: &str) {
        let env_tbl = self
            .doc
            .entry("env")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap();
        env_tbl.remove(key);
    }

    fn parse_template(&self, k: &str, input: &str) -> Result<String> {
        if !input.contains("{{") && !input.contains("{%") && !input.contains("{#") {
            return Ok(input.to_string());
        }
        self.trust_check()?;
        let dir = self.path.parent().unwrap();
        let output = get_tera(dir)
            .render_str(input, &self.context)
            .wrap_err_with(|| eyre!("failed to parse template: {k}='{input}'"))?;
        Ok(output)
    }

    fn trust_check(&self) -> Result<()> {
        let default_cmd = String::new();
        let args = env::ARGS.read().unwrap();
        let cmd = args.get(1).unwrap_or(&default_cmd).as_str();
        if self.get_is_trusted() || cmd == "trust" || cmd == "completion" || cfg!(test) {
            return Ok(());
        }
        if cmd != "hook-env" {
            let ans = prompt::confirm(&format!(
                "{} {} is not trusted. Trust it?",
                style("rtx").yellow().for_stderr(),
                display_path(&self.path)
            ))?;
            if ans {
                config_file::trust(self.path.as_path())?;
                self.is_trusted.lock().unwrap().replace(true);
                return Ok(());
            }
        }
        Err(UntrustedConfig())?
    }

    fn get_is_trusted(&self) -> bool {
        *self.is_trusted.lock().unwrap().borrow()
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

    fn env_remove(&self) -> Vec<String> {
        self.env_remove.clone()
    }

    fn path_dirs(&self) -> Vec<PathBuf> {
        self.path_dirs.clone()
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
            plugin.requests = versions
                .iter()
                .map(|s| {
                    (
                        ToolVersionRequest::new(plugin_name.clone(), s),
                        Default::default(),
                    )
                })
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
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }
        file::write(&self.path, contents)
    }

    fn dump(&self) -> String {
        self.doc.to_string()
    }

    fn to_toolset(&self) -> &Toolset {
        &self.toolset
    }

    fn settings(&self) -> Result<SettingsPartial> {
        let mut s = SettingsPartial::empty();

        for (config_key, v) in self.settings.iter() {
            let k = format!("settings.{config_key}");
            match config_key.to_lowercase().as_str() {
                "experimental" => s.experimental = Some(self.parse_bool(&k, v)?),
                "legacy_version_file" => s.legacy_version_file = Some(self.parse_bool(&k, v)?),
                "legacy_version_file_disable_tools" => {
                    s.legacy_version_file_disable_tools =
                        Some(self.parse_string_array(&k, v)?.into_iter().collect())
                }
                "always_keep_download" => s.always_keep_download = Some(self.parse_bool(&k, v)?),
                "always_keep_install" => s.always_keep_install = Some(self.parse_bool(&k, v)?),
                "plugin_autoupdate_last_check_duration" => {
                    s.plugin_autoupdate_last_check_duration = match v.as_integer() {
                        Some(i) => Some(format!("{}m", i)),
                        None => Some(self.parse_string(&k, v)?),
                    }
                }
                "trusted_config_paths" => {
                    s.trusted_config_paths = Some(self.parse_paths(&k, v)?.into_iter().collect());
                }
                "verbose" => s.verbose = Some(self.parse_bool(&k, v)?),
                "asdf_compat" => s.asdf_compat = Some(self.parse_bool(&k, v)?),
                "jobs" => s.jobs = Some(self.parse_usize(&k, v)?),
                "shorthands_file" => s.shorthands_file = Some(self.parse_path(&k, v)?),
                "disable_default_shorthands" => {
                    s.disable_default_shorthands = Some(self.parse_bool(&k, v)?)
                }
                "disable_tools" => {
                    s.disable_tools = Some(self.parse_string_array(&k, v)?.into_iter().collect());
                }
                "raw" => s.raw = Some(self.parse_bool(&k, v)?),
                "yes" => s.yes = Some(self.parse_bool(&k, v)?),
                _ => Err(eyre!("Unknown config setting: {}", k))?,
            };
        }

        Ok(s)
    }

    fn aliases(&self) -> AliasMap {
        self.alias.clone()
    }

    fn watch_files(&self) -> Vec<PathBuf> {
        match &self.env_file {
            Some(env_file) => vec![self.path.clone(), env_file.clone()],
            None => vec![self.path.clone()],
        }
    }
}

impl Debug for RtxToml {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("RtxToml");
        d.field("path", &self.path)
            .field("toolset", &self.toolset.to_string())
            .field("is_trusted", &self.is_trusted);
        if let Ok(settings) = self.settings() {
            d.field("settings", &settings);
        }
        if let Some(env_file) = &self.env_file {
            d.field("env_file", env_file);
        }
        if !self.env.is_empty() {
            d.field("env", &self.env);
        }
        if !self.env_remove.is_empty() {
            d.field("env_remove", &self.env_remove);
        }
        if !self.path_dirs.is_empty() {
            d.field("path_dirs", &self.path_dirs);
        }
        if !self.alias.is_empty() {
            d.field("alias", &self.alias);
        }
        if !self.plugins.is_empty() {
            d.field("plugins", &self.plugins);
        }
        d.finish()
    }
}

impl Clone for RtxToml {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            path: self.path.clone(),
            toolset: self.toolset.clone(),
            env_file: self.env_file.clone(),
            env: self.env.clone(),
            env_remove: self.env_remove.clone(),
            path_dirs: self.path_dirs.clone(),
            settings: self.settings.clone(),
            alias: self.alias.clone(),
            doc: self.doc.clone(),
            plugins: self.plugins.clone(),
            is_trusted: Mutex::new(RefCell::new(*self.is_trusted.lock().unwrap().borrow())),
        }
    }
}

#[cfg(test)]
mod tests {
    use insta::{assert_debug_snapshot, assert_display_snapshot, assert_snapshot};

    use crate::dirs;
    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_fixture() {
        let cf = RtxToml::from_file(&dirs::HOME.join("fixtures/.rtx.toml"), true).unwrap();

        assert_debug_snapshot!(cf.env());
        assert_debug_snapshot!(cf.settings());
        assert_debug_snapshot!(cf.plugins());
        assert_snapshot!(replace_path(&format!("{:#?}", cf.toolset)));
        assert_debug_snapshot!(cf.alias);

        assert_snapshot!(replace_path(&cf.to_string()));
    }

    #[test]
    fn test_env() {
        let mut cf = RtxToml::init(PathBuf::from("/tmp/.rtx.toml").as_path(), true);
        cf.parse(&formatdoc! {r#"
        [env]
        foo="bar"
        "#})
            .unwrap();

        assert_debug_snapshot!(cf.env(), @r###"
        {
            "foo": "bar",
        }
        "###);
        assert_debug_snapshot!(cf.path_dirs(), @"[]");
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_path_dirs() {
        let p = dirs::HOME.join("fixtures/.rtx.toml");
        let mut cf = RtxToml::init(&p, true);
        cf.parse(&formatdoc! {r#"
        env_path=["/foo", "./bar"]
        [env]
        foo="bar"
        "#})
            .unwrap();

        assert_debug_snapshot!(cf.env(), @r###"
        {
            "foo": "bar",
        }
        "###);
        assert_snapshot!(replace_path(&format!("{:?}", cf.path_dirs())), @r###"["/foo", "~/fixtures/bar"]"###);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_set_alias() {
        let mut cf = RtxToml::init(PathBuf::from("/tmp/.rtx.toml").as_path(), true);
        cf.parse(&formatdoc! {r#"
        [alias.node]
        16 = "16.0.0"
        18 = "18.0.0"
        "#})
            .unwrap();

        cf.set_alias("node", "18", "18.0.1");
        cf.set_alias("node", "20", "20.0.0");
        cf.set_alias("python", "3.10", "3.10.0");

        assert_debug_snapshot!(cf.alias);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_remove_alias() {
        let mut cf = RtxToml::init(PathBuf::from("/tmp/.rtx.toml").as_path(), true);
        cf.parse(&formatdoc! {r#"
        [alias.node]
        16 = "16.0.0"
        18 = "18.0.0"

        [alias.python]
        "3.10" = "3.10.0"
        "#})
            .unwrap();
        cf.remove_alias("node", "16");
        cf.remove_alias("python", "3.10");

        assert_debug_snapshot!(cf.alias);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_replace_versions() {
        let mut cf = RtxToml::init(PathBuf::from("/tmp/.rtx.toml").as_path(), true);
        cf.parse(&formatdoc! {r#"
        [tools]
        node = ["16.0.0", "18.0.0"]
        "#})
            .unwrap();
        cf.replace_versions(
            &PluginName::from("node"),
            &["16.0.1".into(), "18.0.1".into()],
        );

        assert_debug_snapshot!(cf.toolset);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_remove_plugin() {
        let mut cf = RtxToml::init(PathBuf::from("/tmp/.rtx.toml").as_path(), true);
        cf.parse(&formatdoc! {r#"
        [tools]
        node = ["16.0.0", "18.0.0"]
        "#})
            .unwrap();
        cf.remove_plugin(&PluginName::from("node"));

        assert_debug_snapshot!(cf.toolset);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_update_setting() {
        let mut cf = RtxToml::init(PathBuf::from("/tmp/.rtx.toml").as_path(), true);
        cf.parse(&formatdoc! {r#"
        [settings]
        legacy_version_file = true
        [alias.node]
        18 = "18.0.0"
        "#})
            .unwrap();
        cf.update_setting("legacy_version_file", false);
        assert_display_snapshot!(cf.dump(), @r###"
        [settings]
        legacy_version_file = false
        [alias.node]
        18 = "18.0.0"
        "###);
    }

    #[test]
    fn test_remove_setting() {
        let mut cf = RtxToml::init(PathBuf::from("/tmp/.rtx.toml").as_path(), true);
        cf.parse(&formatdoc! {r#"
        [settings]
        legacy_version_file = true
        "#})
            .unwrap();
        cf.remove_setting("legacy_version_file");
        assert_display_snapshot!(cf.dump(), @r###"
        "###);
    }

    #[test]
    fn test_fail_with_unknown_key() {
        let mut cf = RtxToml::init(PathBuf::from("/tmp/.rtx.toml").as_path(), true);
        let err = cf
            .parse(&formatdoc! {r#"
        invalid_key = true
        "#})
            .unwrap_err();
        assert_snapshot!(err.to_string(), @"unknown key: invalid_key");
    }
}
