use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use eyre::{Result, WrapErr};
use serde_derive::Deserialize;
use tera::Context as TeraContext;
use toml_edit::{table, value, Array, Document, Item, Value};
use versions::Versioning;

use crate::config::config_file::{trust_check, ConfigFile, ConfigFileType};
use crate::config::AliasMap;
use crate::file::{create_dir_all, display_path};
use crate::plugins::{unalias_plugin, PluginName};
use crate::task::Task;
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::toolset::{
    ToolSource, ToolVersionList, ToolVersionOptions, ToolVersionRequest, Toolset,
};
use crate::ui::style;
use crate::{dirs, file, parse_error};

#[derive(Default, Deserialize)]
pub struct MiseToml {
    #[serde(skip)]
    context: TeraContext,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    toolset: Toolset,
    #[serde(skip)]
    env_files: Vec<PathBuf>,
    #[serde(skip)]
    env: HashMap<String, String>,
    #[serde(skip)]
    env_remove: Vec<String>,
    #[serde(skip)]
    path_dirs: Vec<PathBuf>,
    #[serde(skip)]
    alias: AliasMap,
    #[serde(skip)]
    doc: Document,
    #[serde(skip)]
    plugins: HashMap<String, String>,
    #[serde(skip)]
    tasks: Vec<Task>,
    #[serde(skip)]
    is_trusted: Mutex<Option<bool>>,
}

impl MiseToml {
    pub fn init(path: &Path) -> Self {
        let mut context = BASE_CONTEXT.clone();
        context.insert("config_root", path.parent().unwrap().to_str().unwrap());
        Self {
            path: path.to_path_buf(),
            context,
            is_trusted: Mutex::new(None),
            toolset: Toolset {
                source: Some(ToolSource::MiseToml(path.to_path_buf())),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        trace!("parsing: {}", display_path(path));
        let mut rf = Self::init(path);
        let body = file::read_to_string(path)?; // .suggestion("ensure file exists and can be read")?;
        rf.parse(&body)?;
        trace!("{}", rf.dump());
        Ok(rf)
    }

    fn parse(&mut self, s: &str) -> Result<()> {
        let doc: Document = s.parse()?; // .suggestion("ensure file is valid TOML")?;
        for (k, v) in doc.iter() {
            match k {
                "min_version" => self.parse_min_version(v)?,
                "dotenv" => self.parse_env_file(k, v, true)?,
                "env_file" => self.parse_env_file(k, v, true)?,
                "env_path" => self.path_dirs = self.parse_path_env(k, v)?,
                "env" => self.parse_env(k, v)?,
                "alias" => self.alias = self.parse_alias(k, v)?,
                "tools" => self.toolset = self.parse_toolset(k, v)?,
                "settings" => {}
                "plugins" => self.plugins = self.parse_plugins(k, v)?,
                "tasks" => self.tasks = self.parse_tasks(k, v)?,
                _ => bail!("unknown key: {}", style::ered(k)),
            }
        }
        self.doc = doc;
        Ok(())
    }

    fn parse_min_version(&self, v: &Item) -> Result<()> {
        match v.as_str() {
            Some(s) => {
                if let (Some(min), Some(cur)) = (
                    Versioning::new(s),
                    Versioning::new(env!("CARGO_PKG_VERSION")),
                ) {
                    ensure!(
                        cur >= min,
                        "mise version {} is required, but you are using {}",
                        style::eyellow(min),
                        style::eyellow(cur)
                    );
                }
                Ok(())
            }
            _ => parse_error!("min_version", v, "string"),
        }
    }

    fn parse_env_file(&mut self, k: &str, v: &Item, deprecate: bool) -> Result<()> {
        trust_check(&self.path)?;
        if deprecate {
            warn!("{k} is deprecated. Use 'env.mise.file' instead.");
        }
        if let Some(filename) = v.as_str() {
            let path = self.path.parent().unwrap().join(filename);
            self.parse_env_filename(path)?;
        } else if let Some(filenames) = v.as_array() {
            for filename in filenames {
                if let Some(filename) = filename.as_str() {
                    let path = self.path.parent().unwrap().join(filename);
                    self.parse_env_filename(path)?;
                } else {
                    parse_error!(k, v, "string");
                }
            }
        } else {
            parse_error!(k, v, "string or array of strings");
        }
        Ok(())
    }

    fn parse_env_filename(&mut self, path: PathBuf) -> Result<()> {
        let dotenv = dotenvy::from_path_iter(&path)
            .wrap_err_with(|| format!("failed to parse dotenv file: {}", display_path(&path)))?;
        for item in dotenv {
            let (k, v) = item?;
            self.env.insert(k, v);
        }
        self.env_files.push(path);
        Ok(())
    }

    fn parse_env(&mut self, key: &str, v: &Item) -> Result<()> {
        trust_check(&self.path)?;
        if let Some(table) = v.as_table_like() {
            ensure!(
                !table.contains_key("PATH"),
                "use 'env.mise.path' instead of 'env.PATH'"
            );
        }
        match v.as_table_like() {
            Some(table) => {
                for (k, v) in table.iter() {
                    let key = format!("{}.{}", key, k);
                    let k = self.parse_template(&key, k)?;
                    if k == "mise" {
                        self.parse_env_mise(&key, v)?;
                    } else if let Some(v) = v.as_str() {
                        let v = self.parse_template(&key, v)?;
                        self.env.insert(k, v);
                    } else if let Some(v) = v.as_integer() {
                        self.env.insert(k, v.to_string());
                    } else if let Some(v) = v.as_bool() {
                        if !v {
                            self.env_remove.push(k);
                        }
                    } else {
                        parse_error!(key, v, "string, num, or bool")
                    }
                }
            }
            _ => parse_error!(key, v, "table"),
        }
        Ok(())
    }

    fn parse_env_mise(&mut self, key: &str, v: &Item) -> Result<()> {
        match v.as_table_like() {
            Some(table) => {
                for (k, v) in table.iter() {
                    let key = format!("{}.{}", key, k);
                    match k {
                        "file" => self.parse_env_file(&key, v, false)?,
                        "path" => self.path_dirs = self.parse_path_env(&key, v)?,
                        _ => parse_error!(key, v, "file or path"),
                    }
                }
                Ok(())
            }
            _ => parse_error!(key, v, "table"),
        }
    }

    fn parse_path_env(&self, k: &str, v: &Item) -> Result<Vec<PathBuf>> {
        trust_check(&self.path)?;
        let config_root = self.path.parent().unwrap().to_path_buf();
        let get_path = |v: &Item| -> Result<PathBuf> {
            if let Some(s) = v.as_str() {
                let s = self.parse_template(k, s)?;
                let s = match s.strip_prefix("./") {
                    Some(s) => config_root.join(s),
                    None => match s.strip_prefix("~/") {
                        Some(s) => dirs::HOME.join(s),
                        None => s.into(),
                    },
                };
                Ok(s)
            } else {
                parse_error!(k, v, "string")
            }
        };
        if let Some(array) = v.as_array() {
            let mut path = Vec::new();
            for v in array {
                let item = Item::Value(v.clone());
                path.push(get_path(&item)?);
            }
            Ok(path)
        } else {
            Ok(vec![get_path(v)?])
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
                                    _ => parse_error!(format!("{}.{}", k, from), to, "string"),
                                }
                            }
                        }
                        _ => parse_error!(k, v, "table"),
                    }
                }
                Ok(aliases)
            }
            _ => parse_error!(k, v, "table"),
        }
    }

    fn parse_plugins(&self, key: &str, v: &Item) -> Result<HashMap<String, String>> {
        trust_check(&self.path)?;
        self.parse_hashmap(key, v)
    }

    fn parse_tasks(&self, key: &str, v: &Item) -> Result<Vec<Task>> {
        match v.as_table_like() {
            Some(table) => {
                let mut tasks = Vec::new();
                for (name, v) in table.iter() {
                    let k = format!("{}.{}", key, name);
                    let name = self.parse_template(&k, name)?;
                    let task = self.parse_task(&k, v, &name)?;
                    tasks.push(task);
                }
                Ok(tasks)
            }
            _ => parse_error!(key, v, "table"),
        }
    }

    fn parse_task(&self, key: &str, v: &Item, name: &str) -> Result<Task> {
        let mut task = Task::new(name.into(), self.path.clone());
        if v.as_str().is_some() {
            task.run = self.parse_string_or_array(key, v)?;
            return Ok(task);
        }
        match v.as_table_like() {
            Some(table) => {
                let mut task = Task::new(name.into(), self.path.clone());
                for (k, v) in table.iter() {
                    let key = format!("{key}.{k}");
                    match k {
                        "alias" => task.aliases = self.parse_string_or_array(&key, v)?,
                        // "args" => task.args = self.parse_string_array(&key, v)?,
                        "run" => task.run = self.parse_string_or_array(&key, v)?,
                        // "command" => task.command = Some(self.parse_string_tmpl(&key, v)?),
                        "depends" => task.depends = self.parse_string_array(&key, v)?,
                        "description" => task.description = self.parse_string(&key, v)?,
                        "env" => task.env = self.parse_hashmap(&key, v)?,
                        "file" => task.file = Some(self.parse_path(&key, v)?),
                        "hide" => task.hide = self.parse_bool(&key, v)?,
                        "dir" => task.dir = Some(self.parse_path(&key, v)?),
                        "outputs" => task.outputs = self.parse_string_array(&key, v)?,
                        "raw" => task.raw = self.parse_bool(&key, v)?,
                        // "script" => task.script = Some(self.parse_string_tmpl(&key, v)?),
                        "sources" => task.sources = self.parse_string_array(&key, v)?,
                        _ => parse_error!(key, v, "task property"),
                    }
                }
                Ok(task)
            }
            _ => parse_error!(key, v, "table"),
        }
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
                        _ => parse_error!(key, v, "string"),
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
            _ => parse_error!(key, v, "table"),
        }
    }

    fn parse_tool_version_list(
        &self,
        key: &str,
        v: &Item,
        plugin_name: &PluginName,
    ) -> Result<ToolVersionList> {
        let source = ToolSource::MiseToml(self.path.clone());
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
                trust_check(&self.path)?;
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
        match v.as_table_like() {
            Some(table) => {
                let tv = if let Some(v) = table.get("version") {
                    match v {
                        Item::Value(v) => self.parse_tool_version_request(key, v, plugin_name)?,
                        _ => parse_error!(format!("{}.version", key), v, "string"),
                    }
                } else if let Some(path) = table.get("path") {
                    match path.as_str() {
                        Some(s) => {
                            let s = self.parse_template(key, s)?;
                            ToolVersionRequest::Path(plugin_name.clone(), s.into())
                        }
                        _ => parse_error!(format!("{}.path", key), v, "string"),
                    }
                } else if let Some(prefix) = table.get("prefix") {
                    match prefix.as_str() {
                        Some(s) => {
                            let s = self.parse_template(key, s)?;
                            ToolVersionRequest::Prefix(plugin_name.clone(), s)
                        }
                        _ => parse_error!(format!("{}.prefix", key), v, "string"),
                    }
                } else if let Some(r) = table.get("ref") {
                    match r.as_str() {
                        Some(s) => {
                            let s = self.parse_template(key, s)?;
                            ToolVersionRequest::Ref(plugin_name.clone(), s)
                        }
                        _ => parse_error!(format!("{}.ref", key), v, "string"),
                    }
                } else {
                    parse_error!(key, v, "version, path, or prefix");
                };
                let mut opts = ToolVersionOptions::default();
                for (k, v) in table.iter() {
                    if k == "version" || k == "path" || k == "prefix" || k == "ref" {
                        continue;
                    }
                    let s = if let Some(s) = v.as_str() {
                        self.parse_template(key, s)?
                    } else if let Some(b) = v.as_bool() {
                        b.to_string()
                    } else {
                        parse_error!(key, v, "string or bool");
                    };
                    opts.insert(k.into(), s);
                }
                Ok((tv, opts))
            }
            _ => match v {
                Item::Value(v) => {
                    let tv = self.parse_tool_version_request(key, v, plugin_name)?;
                    Ok((tv, Default::default()))
                }
                _ => parse_error!(key, v, "value"),
            },
        }
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
            _ => parse_error!(key, v, "string"),
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
            _ => parse_error!(k, v, "boolean"),
        }
    }

    fn parse_string(&self, k: &str, v: &Item) -> Result<String> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => Ok(v.to_string()),
            _ => parse_error!(k, v, "string"),
        }
    }

    fn parse_path(&self, k: &str, v: &Item) -> Result<PathBuf> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => {
                let v = self.parse_template(k, v)?;
                Ok(v.into())
            }
            _ => parse_error!(k, v, "path"),
        }
    }

    fn parse_string_or_array(&self, k: &str, v: &Item) -> Result<Vec<String>> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => {
                let v = self.parse_template(k, v)?;
                Ok(vec![v])
            }
            _ => self.parse_string_array(k, v),
        }
    }

    fn parse_string_array(&self, k: &str, v: &Item) -> Result<Vec<String>> {
        match v
            .as_array()
            .map(|v| v.iter().map(|v| v.as_str().unwrap().to_string()).collect())
        {
            Some(v) => Ok(v),
            _ => parse_error!(k, v, "array of strings"),
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
        trust_check(&self.path)?;
        let dir = self.path.parent().unwrap();
        let output = get_tera(dir)
            .render_str(input, &self.context)
            .wrap_err_with(|| eyre!("failed to parse template: {k}='{input}'"))?;
        Ok(output)
    }
}

impl ConfigFile for MiseToml {
    fn get_type(&self) -> ConfigFileType {
        ConfigFileType::MiseToml
    }

    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn project_root(&self) -> Option<&Path> {
        let fp = self.get_path();
        let filename = fp.file_name().unwrap_or_default().to_string_lossy();
        match fp.parent() {
            Some(dir) => match dir {
                dir if dir.starts_with(*dirs::CONFIG) => None,
                dir if dir.starts_with(*dirs::SYSTEM) => None,
                dir if dir == *dirs::HOME => None,
                dir if !filename.starts_with('.') && dir.ends_with("/.mise") => dir.parent(),
                dir if !filename.starts_with('.') && dir.ends_with("/.config/mise") => {
                    dir.parent().unwrap().parent()
                }
                dir => Some(dir),
            },
            None => None,
        }
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

    fn env_path(&self) -> Vec<PathBuf> {
        self.path_dirs.clone()
    }

    fn tasks(&self) -> Vec<&Task> {
        self.tasks.iter().collect()
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

    fn aliases(&self) -> AliasMap {
        self.alias.clone()
    }

    fn watch_files(&self) -> Vec<PathBuf> {
        once(&self.path)
            .chain(self.env_files.iter())
            .cloned()
            .collect()
    }
}

impl Debug for MiseToml {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let tools = self.toolset.to_string();
        let title = format!("MiseToml({}): {tools}", &display_path(&self.path));
        let mut d = f.debug_struct(&title);
        // d.field("is_trusted", &self.is_trusted);
        if !self.env_files.is_empty() {
            d.field("env_files", &self.env_files);
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

impl Clone for MiseToml {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            path: self.path.clone(),
            toolset: self.toolset.clone(),
            env_files: self.env_files.clone(),
            env: self.env.clone(),
            env_remove: self.env_remove.clone(),
            path_dirs: self.path_dirs.clone(),
            alias: self.alias.clone(),
            doc: self.doc.clone(),
            plugins: self.plugins.clone(),
            tasks: self.tasks.clone(),
            is_trusted: Mutex::new(*self.is_trusted.lock().unwrap()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::dirs;
    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_fixture() {
        let cf = MiseToml::from_file(&dirs::HOME.join("fixtures/.mise.toml")).unwrap();

        assert_debug_snapshot!(cf.env());
        assert_debug_snapshot!(cf.plugins());
        assert_snapshot!(replace_path(&format!("{:#?}", cf.toolset)));
        assert_debug_snapshot!(cf.alias);

        assert_snapshot!(replace_path(&format!("{:#?}", &cf)));
    }

    #[test]
    fn test_env() {
        let mut cf = MiseToml::init(PathBuf::from("/tmp/.mise.toml").as_path());
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
        assert_debug_snapshot!(cf.env_path(), @"[]");
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        with_settings!({
            assert_snapshot!(cf.dump());
            assert_display_snapshot!(cf);
            assert_debug_snapshot!(cf);
        });
    }

    #[test]
    fn test_path_dirs() {
        with_settings!({
            let p = dirs::HOME.join("fixtures/.mise.toml");
            let mut cf = MiseToml::init(&p);
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
            assert_snapshot!(replace_path(&format!("{:?}", cf.env_path())), @r###"["/foo", "~/fixtures/bar"]"###);
            let cf: Box<dyn ConfigFile> = Box::new(cf);
            assert_snapshot!(cf.dump());
            assert_display_snapshot!(cf);
            assert_debug_snapshot!(cf);
        });
    }

    #[test]
    fn test_set_alias() {
        let mut cf = MiseToml::init(PathBuf::from("/tmp/.mise.toml").as_path());
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
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_display_snapshot!(cf);
    }

    #[test]
    fn test_remove_alias() {
        let mut cf = MiseToml::init(PathBuf::from("/tmp/.mise.toml").as_path());
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
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump());
        assert_display_snapshot!(cf);
        assert_debug_snapshot!(cf);
    }

    #[test]
    fn test_replace_versions() {
        let mut cf = MiseToml::init(PathBuf::from("/tmp/.mise.toml").as_path());
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
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump());
        assert_display_snapshot!(cf);
        assert_debug_snapshot!(cf);
    }

    #[test]
    fn test_remove_plugin() {
        let mut cf = MiseToml::init(PathBuf::from("/tmp/.mise.toml").as_path());
        cf.parse(&formatdoc! {r#"
        [tools]
        node = ["16.0.0", "18.0.0"]
        "#})
            .unwrap();
        cf.remove_plugin(&PluginName::from("node"));

        assert_debug_snapshot!(cf.toolset);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump());
        assert_display_snapshot!(cf);
        assert_debug_snapshot!(cf);
    }

    #[test]
    fn test_fail_with_unknown_key() {
        let mut cf = MiseToml::init(PathBuf::from("/tmp/.mise.toml").as_path());
        let err = cf
            .parse(&formatdoc! {r#"
        invalid_key = true
        "#})
            .unwrap_err();
        assert_snapshot!(err.to_string(), @"unknown key: invalid_key");
    }
}
