use std::collections::{BTreeMap, HashMap};
use std::fmt::{Debug, Formatter};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;

use eyre::WrapErr;
use itertools::Itertools;
use serde::de::Visitor;
use serde::{de, Deserializer};
use serde_derive::Deserialize;
use tera::Context as TeraContext;
use toml_edit::{table, value, Array, Document, Item, Value};
use versions::Versioning;

use crate::cli::args::ForgeArg;
use crate::config::config_file::{trust_check, ConfigFile, ConfigFileType, TaskConfig};
use crate::config::env_directive::EnvDirective;
use crate::config::AliasMap;
use crate::file::{create_dir_all, display_path};
use crate::task::Task;
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::toolset::{
    ToolSource, ToolVersionList, ToolVersionOptions, ToolVersionRequest, Toolset,
};
use crate::ui::style;
use crate::{dirs, file, parse_error};

#[derive(Default, Deserialize)]
// #[serde(deny_unknown_fields)]
pub struct MiseToml {
    #[serde(default, deserialize_with = "deserialize_version")]
    min_version: Option<Versioning>,
    #[serde(skip)]
    context: TeraContext,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    toolset: Toolset,
    #[serde(default, alias = "dotenv", deserialize_with = "deserialize_arr")]
    env_file: Vec<PathBuf>,
    #[serde(default)]
    env: EnvList,
    #[serde(default, deserialize_with = "deserialize_arr")]
    env_path: Vec<PathBuf>,
    #[serde(default, deserialize_with = "deserialize_alias")]
    alias: AliasMap,
    #[serde(skip)]
    doc: Document,
    #[serde(default)]
    plugins: HashMap<String, String>,
    #[serde(default)]
    pub task_config: TaskConfig,
    #[serde(skip)]
    tasks: Vec<Task>,
    #[serde(skip)]
    is_trusted: Mutex<Option<bool>>,
    #[serde(skip)]
    project_root: Option<PathBuf>,
    #[serde(skip)]
    config_root: PathBuf,
}

#[derive(Debug, Default, Clone)]
pub struct EnvList(pub(crate) Vec<EnvDirective>);

impl MiseToml {
    pub fn init(path: &Path) -> Self {
        let mut context = BASE_CONTEXT.clone();
        context.insert("config_root", path.parent().unwrap().to_str().unwrap());
        let filename = path.file_name().unwrap_or_default().to_string_lossy();
        let project_root = match path.parent() {
            Some(dir) => match dir {
                dir if dir.starts_with(*dirs::CONFIG) => None,
                dir if dir.starts_with(*dirs::SYSTEM) => None,
                dir if dir == *dirs::HOME => None,
                dir if !filename.starts_with('.') && dir.ends_with(".mise") => dir.parent(),
                dir if !filename.starts_with('.') && dir.ends_with(".config/mise") => {
                    dir.parent().unwrap().parent()
                }
                dir => Some(dir),
            },
            None => None,
        };
        let config_root = project_root
            .or_else(|| path.parent())
            .or_else(|| dirs::CWD.as_ref().map(|p| p.as_path()))
            .unwrap_or_else(|| *dirs::HOME)
            .to_path_buf();
        Self {
            path: path.to_path_buf(),
            context,
            is_trusted: Mutex::new(None),
            toolset: Toolset {
                source: Some(ToolSource::MiseToml(path.to_path_buf())),
                ..Default::default()
            },
            project_root: project_root.map(|p| p.to_path_buf()),
            config_root,
            ..Default::default()
        }
    }

    pub fn from_file(path: &Path) -> eyre::Result<Self> {
        trace!("parsing: {}", display_path(path));
        let mut rf = Self::init(path);
        let body = file::read_to_string(path)?; // .suggestion("ensure file exists and can be read")?;
        rf.parse(&body)?;
        trace!("{}", rf.dump());
        Ok(rf)
    }

    fn parse(&mut self, s: &str) -> eyre::Result<()> {
        let cfg: MiseToml = toml::from_str(s)?;
        self.alias = cfg.alias;
        self.env = cfg.env;
        self.env_file = cfg.env_file;
        self.env_path = cfg.env_path;
        self.min_version = cfg.min_version;
        self.plugins = cfg.plugins;
        self.task_config = cfg.task_config;

        // TODO: right now some things are parsed with serde (above) and some not (below) everything
        // should be moved to serde eventually

        let doc: Document = s.parse()?; // .suggestion("ensure file is valid TOML")?;
        for (k, v) in doc.iter() {
            match k {
                "tools" => self.toolset = self.parse_toolset(k, v)?,
                "tasks" => self.tasks = self.parse_tasks(k, v)?,
                "alias" | "dotenv" | "env_file" | "env_path" | "min_version" | "settings"
                | "env" | "plugins" | "task_config" => {}
                _ => bail!("unknown key: {}", style::ered(k)),
            }
        }
        self.doc = doc;
        Ok(())
    }

    fn parse_tasks(&self, key: &str, v: &Item) -> eyre::Result<Vec<Task>> {
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

    fn parse_task(&self, key: &str, v: &Item, name: &str) -> eyre::Result<Task> {
        let mut task = Task::new(name.into(), self.path.clone());
        if v.as_str().is_some() {
            task.run = self.parse_string_or_array(key, v)?;
            return Ok(task);
        }
        if v.as_array().is_some() {
            task.run = self.parse_string_array(key, v)?;
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

    fn parse_hashmap(&self, key: &str, v: &Item) -> eyre::Result<HashMap<String, String>> {
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

    fn parse_toolset(&self, key: &str, v: &Item) -> eyre::Result<Toolset> {
        let mut toolset = Toolset::new(self.toolset.source.clone().unwrap());

        match v.as_table_like() {
            Some(table) => {
                for (plugin, v) in table.iter() {
                    let k = format!("{}.{}", key, plugin);
                    let fa: ForgeArg = plugin.parse()?;
                    let tvl = self.parse_tool_version_list(&k, v, fa.clone())?;
                    toolset.versions.insert(fa, tvl);
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
        fa: ForgeArg,
    ) -> eyre::Result<ToolVersionList> {
        let source = ToolSource::MiseToml(self.path.clone());
        let mut tool_version_list = ToolVersionList::new(fa.clone(), source);

        match v {
            Item::ArrayOfTables(v) => {
                for table in v.iter() {
                    for (tool, v) in table.iter() {
                        let k = format!("{}.{}", key, tool);
                        let (tvr, opts) = self.parse_tool_version(&k, v, fa.clone())?;
                        tool_version_list.requests.push((tvr, opts));
                    }
                }
            }
            v => match v.as_array() {
                Some(v) => {
                    for v in v.iter() {
                        let item = Item::Value(v.clone());
                        let (tvr, opts) = self.parse_tool_version(key, &item, fa.clone())?;
                        tool_version_list.requests.push((tvr, opts));
                    }
                }
                _ => {
                    tool_version_list
                        .requests
                        .push(self.parse_tool_version(key, v, fa)?);
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
        fa: ForgeArg,
    ) -> eyre::Result<(ToolVersionRequest, ToolVersionOptions)> {
        match v.as_table_like() {
            Some(table) => {
                let tv = if let Some(v) = table.get("version") {
                    match v {
                        Item::Value(v) => self.parse_tool_version_request(key, v, fa)?,
                        _ => parse_error!(format!("{}.version", key), v, "string"),
                    }
                } else if let Some(path) = table.get("path") {
                    match path.as_str() {
                        Some(s) => {
                            let s = self.parse_template(key, s)?;
                            ToolVersionRequest::Path(fa, s.into())
                        }
                        _ => parse_error!(format!("{}.path", key), v, "string"),
                    }
                } else if let Some(prefix) = table.get("prefix") {
                    match prefix.as_str() {
                        Some(s) => {
                            let s = self.parse_template(key, s)?;
                            ToolVersionRequest::Prefix(fa, s)
                        }
                        _ => parse_error!(format!("{}.prefix", key), v, "string"),
                    }
                } else if let Some(r) = table.get("ref") {
                    match r.as_str() {
                        Some(s) => {
                            let s = self.parse_template(key, s)?;
                            ToolVersionRequest::Ref(fa, s)
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
                    let tv = self.parse_tool_version_request(key, v, fa)?;
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
        fa: ForgeArg,
    ) -> eyre::Result<ToolVersionRequest> {
        match v.as_str() {
            Some(s) => {
                let s = self.parse_template(key, s)?;
                Ok(ToolVersionRequest::new(fa, &s))
            }
            _ => parse_error!(key, v, "string"),
        }
    }

    pub fn set_alias(&mut self, fa: &ForgeArg, from: &str, to: &str) {
        self.alias
            .entry(fa.clone())
            .or_default()
            .insert(from.into(), to.into());
        self.doc
            .entry("alias")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .entry(&fa.to_string())
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .insert(from, value(to));
    }

    pub fn remove_alias(&mut self, fa: &ForgeArg, from: &str) {
        if let Some(aliases) = self.doc.get_mut("alias").and_then(|v| v.as_table_mut()) {
            if let Some(plugin_aliases) = aliases
                .get_mut(&fa.to_string())
                .and_then(|v| v.as_table_mut())
            {
                self.alias.get_mut(fa).unwrap().remove(from);
                plugin_aliases.remove(from);
                if plugin_aliases.is_empty() {
                    aliases.remove(&fa.to_string());
                    self.alias.remove(fa);
                }
            }
            if aliases.is_empty() {
                self.doc.as_table_mut().remove("alias");
            }
        }
    }

    fn parse_bool(&self, k: &str, v: &Item) -> eyre::Result<bool> {
        match v.as_value().map(|v| v.as_bool()) {
            Some(Some(v)) => Ok(v),
            _ => parse_error!(k, v, "boolean"),
        }
    }

    fn parse_string(&self, k: &str, v: &Item) -> eyre::Result<String> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => Ok(v.to_string()),
            _ => parse_error!(k, v, "string"),
        }
    }

    fn parse_path(&self, k: &str, v: &Item) -> eyre::Result<PathBuf> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => {
                let v = self.parse_template(k, v)?;
                Ok(v.into())
            }
            _ => parse_error!(k, v, "path"),
        }
    }

    fn parse_string_or_array(&self, k: &str, v: &Item) -> eyre::Result<Vec<String>> {
        match v.as_value().map(|v| v.as_str()) {
            Some(Some(v)) => {
                let v = self.parse_template(k, v)?;
                Ok(vec![v])
            }
            _ => self.parse_string_array(k, v),
        }
    }

    fn parse_string_array(&self, k: &str, v: &Item) -> eyre::Result<Vec<String>> {
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

    fn parse_template(&self, k: &str, input: &str) -> eyre::Result<String> {
        if !input.contains("{{") && !input.contains("{%") && !input.contains("{#") {
            return Ok(input.to_string());
        }
        trust_check(&self.path)?;
        let dir = self.path.parent();
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

    fn min_version(&self) -> &Option<Versioning> {
        &self.min_version
    }

    fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    fn plugins(&self) -> HashMap<String, String> {
        self.plugins.clone()
    }

    fn env_entries(&self) -> Vec<EnvDirective> {
        let env_entries = self.env.0.iter().cloned();
        let path_entries = self
            .env_path
            .iter()
            .map(|p| EnvDirective::Path(p.clone()))
            .collect_vec();
        let env_files = self
            .env_file
            .iter()
            .map(|p| EnvDirective::File(p.clone()))
            .collect_vec();
        path_entries
            .into_iter()
            .chain(env_files)
            .chain(env_entries)
            .collect()
    }

    fn tasks(&self) -> Vec<&Task> {
        self.tasks.iter().collect()
    }

    fn remove_plugin(&mut self, fa: &ForgeArg) {
        self.toolset.versions.shift_remove(fa);
        if let Some(tools) = self.doc.get_mut("tools") {
            if let Some(tools) = tools.as_table_like_mut() {
                tools.remove(&fa.to_string());
                if tools.is_empty() {
                    self.doc.as_table_mut().remove("tools");
                }
            }
        }
    }

    fn replace_versions(&mut self, fa: &ForgeArg, versions: &[String]) {
        if let Some(plugin) = self.toolset.versions.get_mut(fa) {
            plugin.requests = versions
                .iter()
                .map(|s| (ToolVersionRequest::new(fa.clone(), s), Default::default()))
                .collect();
        }
        let tools = self
            .doc
            .entry("tools")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();

        if versions.len() == 1 {
            tools.insert(&fa.to_string(), value(versions[0].clone()));
        } else {
            let mut arr = Array::new();
            for v in versions {
                arr.push(v);
            }
            tools.insert(&fa.to_string(), Item::Value(Value::Array(arr)));
        }
    }

    fn save(&self) -> eyre::Result<()> {
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

    fn task_config(&self) -> &TaskConfig {
        &self.task_config
    }
}

impl Debug for MiseToml {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let tools = self.toolset.to_string();
        let title = format!("MiseToml({}): {tools}", &display_path(&self.path));
        let mut d = f.debug_struct(&title);
        // d.field("is_trusted", &self.is_trusted);
        if let Some(min_version) = &self.min_version {
            d.field("min_version", &min_version.to_string());
        }
        if !self.env_file.is_empty() {
            d.field("env_file", &self.env_file);
        }
        let env = self.env_entries();
        if !env.is_empty() {
            d.field("env", &env);
        }
        if !self.alias.is_empty() {
            d.field("alias", &self.alias);
        }
        if !self.plugins.is_empty() {
            d.field("plugins", &self.plugins);
        }
        if self.task_config.includes.is_some() {
            d.field("task_config", &self.task_config);
        }
        d.finish()
    }
}

impl Clone for MiseToml {
    fn clone(&self) -> Self {
        Self {
            min_version: self.min_version.clone(),
            context: self.context.clone(),
            path: self.path.clone(),
            toolset: self.toolset.clone(),
            env_file: self.env_file.clone(),
            env: self.env.clone(),
            env_path: self.env_path.clone(),
            alias: self.alias.clone(),
            doc: self.doc.clone(),
            plugins: self.plugins.clone(),
            tasks: self.tasks.clone(),
            task_config: self.task_config.clone(),
            is_trusted: Mutex::new(*self.is_trusted.lock().unwrap()),
            project_root: self.project_root.clone(),
            config_root: self.config_root.clone(),
        }
    }
}

fn deserialize_version<'de, D>(deserializer: D) -> Result<Option<Versioning>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = serde::Deserialize::deserialize(deserializer)?;

    match s {
        Some(s) => Ok(Some(
            Versioning::new(&s)
                .ok_or(versions::Error::IllegalVersioning(s))
                .map_err(serde::de::Error::custom)?,
        )),
        None => Ok(None),
    }
}

fn deserialize_arr<'de, D, T>(deserializer: D) -> eyre::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    <T as FromStr>::Err: std::fmt::Display,
{
    struct ArrVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T> Visitor<'de> for ArrVisitor<T>
    where
        T: FromStr,
        <T as FromStr>::Err: std::fmt::Display,
    {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("string or array of strings")
        }

        fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            let v = v.parse().map_err(de::Error::custom)?;
            Ok(vec![v])
        }

        fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            let mut v = vec![];
            while let Some(s) = seq.next_element::<String>()? {
                v.push(s.parse().map_err(de::Error::custom)?);
            }
            Ok(v)
        }
    }

    deserializer.deserialize_any(ArrVisitor(std::marker::PhantomData))
}

impl<'de> de::Deserialize<'de> for EnvList {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct EnvManVisitor;

        impl<'de> Visitor<'de> for EnvManVisitor {
            type Value = EnvList;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("env table or array of env tables")
            }

            fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
            where
                S: de::SeqAccess<'de>,
            {
                let mut env = vec![];
                while let Some(list) = seq.next_element::<EnvList>()? {
                    env.extend(list.0);
                }
                Ok(EnvList(env))
            }
            fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut env = vec![];
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "_" | "mise" => {
                            #[derive(Deserialize)]
                            #[serde(deny_unknown_fields)]
                            struct EnvDirectives {
                                #[serde(default, deserialize_with = "deserialize_arr")]
                                path: Vec<PathBuf>,
                                #[serde(default, deserialize_with = "deserialize_arr")]
                                file: Vec<PathBuf>,
                                #[serde(default, deserialize_with = "deserialize_arr")]
                                source: Vec<PathBuf>,
                            }
                            let directives = map.next_value::<EnvDirectives>()?;
                            // TODO: parse these in the order they're defined somehow
                            for path in directives.path {
                                env.push(EnvDirective::Path(path));
                            }
                            for file in directives.file {
                                env.push(EnvDirective::File(file));
                            }
                            for source in directives.source {
                                env.push(EnvDirective::Source(source));
                            }
                        }
                        _ => {
                            enum Val {
                                Int(i64),
                                Str(String),
                                Bool(bool),
                            }

                            impl<'de> de::Deserialize<'de> for Val {
                                fn deserialize<D>(
                                    deserializer: D,
                                ) -> std::result::Result<Self, D::Error>
                                where
                                    D: de::Deserializer<'de>,
                                {
                                    struct ValVisitor;

                                    impl<'de> Visitor<'de> for ValVisitor {
                                        type Value = Val;
                                        fn expecting(
                                            &self,
                                            formatter: &mut Formatter,
                                        ) -> std::fmt::Result
                                        {
                                            formatter.write_str("env value")
                                        }

                                        fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
                                        where
                                            E: de::Error,
                                        {
                                            match v {
                                                true => Err(de::Error::custom(
                                                    "env values cannot be true",
                                                )),
                                                false => Ok(Val::Bool(v)),
                                            }
                                        }

                                        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
                                        where
                                            E: de::Error,
                                        {
                                            Ok(Val::Int(v))
                                        }

                                        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                                        where
                                            E: de::Error,
                                        {
                                            Ok(Val::Str(v.to_string()))
                                        }
                                    }

                                    deserializer.deserialize_any(ValVisitor)
                                }
                            }

                            let value = map.next_value::<Val>()?;
                            match value {
                                Val::Int(i) => {
                                    env.push(EnvDirective::Val(key, i.to_string()));
                                }
                                Val::Str(s) => {
                                    env.push(EnvDirective::Val(key, s));
                                }
                                Val::Bool(_) => env.push(EnvDirective::Rm(key)),
                            }
                        }
                    }
                }
                Ok(EnvList(env))
            }
        }

        deserializer.deserialize_any(EnvManVisitor)
    }
}

fn deserialize_alias<'de, D>(deserializer: D) -> Result<AliasMap, D::Error>
where
    D: Deserializer<'de>,
{
    struct AliasMapVisitor;

    impl<'de> Visitor<'de> for AliasMapVisitor {
        type Value = AliasMap;
        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("alias table")
        }

        fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            let mut aliases = AliasMap::new();
            while let Some(plugin) = map.next_key::<String>()? {
                let fa: ForgeArg = plugin.parse().map_err(de::Error::custom)?;
                let plugin_aliases = aliases.entry(fa).or_default();
                for (from, to) in map.next_value::<BTreeMap<String, String>>()? {
                    plugin_aliases.insert(from, to);
                }
            }
            Ok(aliases)
        }
    }

    deserializer.deserialize_map(AliasMapVisitor)
}

#[cfg(test)]
mod tests {
    use crate::dirs;
    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_fixture() {
        let cf = MiseToml::from_file(&dirs::HOME.join("fixtures/.mise.toml")).unwrap();

        assert_debug_snapshot!(cf.env_entries());
        assert_debug_snapshot!(cf.plugins());
        assert_snapshot!(replace_path(&format!("{:#?}", cf.toolset)));
        assert_debug_snapshot!(cf.alias);

        assert_snapshot!(replace_path(&format!("{:#?}", &cf)));
    }

    #[test]
    fn test_env() {
        let cf = MiseToml::init(PathBuf::from("/tmp/.mise.toml").as_path());
        let env = parse_env(formatdoc! {r#"
        min_version = "2024.1.1"
        [env]
        foo="bar"
        "#});

        assert_debug_snapshot!(env, @r###""foo=bar""###);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        with_settings!({
            assert_snapshot!(cf.dump());
            assert_display_snapshot!(cf);
            assert_debug_snapshot!(cf);
        });
    }

    #[test]
    fn test_env_array_valid() {
        let env = parse_env(formatdoc! {r#"
        [[env]]
        foo="bar"

        [[env]]
        bar="baz"
        "#});

        assert_snapshot!(env, @r###"
        foo=bar
        bar=baz
        "###);
    }

    #[test]
    fn test_path_dirs() {
        let env = parse_env(formatdoc! {r#"
            env_path=["/foo", "./bar"]
            [env]
            foo="bar"
            "#});

        assert_snapshot!(env, @r###"
        path_add /foo
        path_add ./bar
        foo=bar
        "###);

        let env = parse_env(formatdoc! {r#"
            env_path="./bar"
            "#});
        assert_snapshot!(env, @"path_add ./bar");

        let env = parse_env(formatdoc! {r#"
            [env]
            _.path = "./bar"
            "#});
        assert_debug_snapshot!(env, @r###""path_add ./bar""###);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.path = ["/foo", "./bar"]
            "#});
        assert_snapshot!(env, @r###"
        path_add /foo
        path_add ./bar
        "###);

        let env = parse_env(formatdoc! {r#"
            [[env]]
            _.path = "/foo"
            [[env]]
            _.path = "./bar"
            "#});
        assert_snapshot!(env, @r###"
        path_add /foo
        path_add ./bar
        "###);

        let env = parse_env(formatdoc! {r#"
            env_path = "/foo"
            [env]
            _.path = "./bar"
            "#});
        assert_snapshot!(env, @r###"
        path_add /foo
        path_add ./bar
        "###);
    }

    #[test]
    fn test_env_file() {
        let env = parse_env(formatdoc! {r#"
            env_file = ".env"
            "#});

        assert_debug_snapshot!(env, @r###""dotenv .env""###);

        let env = parse_env(formatdoc! {r#"
            env_file=[".env", ".env2"]
            "#});
        assert_debug_snapshot!(env, @r###""dotenv .env\ndotenv .env2""###);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.file = ".env"
            "#});
        assert_debug_snapshot!(env, @r###""dotenv .env""###);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.file = [".env", ".env2"]
            "#});
        assert_debug_snapshot!(env, @r###""dotenv .env\ndotenv .env2""###);

        let env = parse_env(formatdoc! {r#"
            dotenv = ".env"
            [env]
            _.file = ".env2"
            "#});
        assert_debug_snapshot!(env, @r###""dotenv .env\ndotenv .env2""###);
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

        let node = "node".parse().unwrap();
        let python = "python".parse().unwrap();
        cf.set_alias(&node, "18", "18.0.1");
        cf.set_alias(&node, "20", "20.0.0");
        cf.set_alias(&python, "3.10", "3.10.0");

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
        let node = "node".parse().unwrap();
        let python = "python".parse().unwrap();
        cf.remove_alias(&node, "16");
        cf.remove_alias(&python, "3.10");

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
        let node = "node".parse().unwrap();
        cf.replace_versions(&node, &["16.0.1".into(), "18.0.1".into()]);

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
        cf.remove_plugin(&"node".parse().unwrap());

        assert_debug_snapshot!(cf.toolset);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump());
        assert_display_snapshot!(cf);
        assert_debug_snapshot!(cf);
    }

    #[test]
    fn test_fail_with_unknown_key() {
        let mut cf = MiseToml::init(PathBuf::from("/tmp/.mise.toml").as_path());
        let _ = cf
            .parse(&formatdoc! {r#"
        invalid_key = true
        "#})
            .unwrap_err();
    }

    #[test]
    fn test_env_entries() {
        let toml = formatdoc! {r#"
        [env]
        foo1="1"
        rm=false
        _.path="/foo"
        foo2="2"
        _.file=".env"
        foo3="3"
        "#};
        assert_snapshot!(parse_env(toml), @r###"
        foo1=1
        unset rm
        path_add /foo
        dotenv .env
        foo2=2
        foo3=3
        "###);
    }

    #[test]
    fn test_env_arr() {
        let toml = formatdoc! {r#"
        [[env]]
        foo1="1"
        rm=false
        _.path="/foo"
        foo2="2"
        _.file=".env"
        foo3="3"
        _.source="/baz1"

        [[env]]
        foo4="4"
        rm=false
        _.file=".env2"
        foo5="5"
        _.path="/bar"
        foo6="6"
        _.source="/baz2"
        "#};
        assert_snapshot!(parse_env(toml), @r###"
        foo1=1
        unset rm
        path_add /foo
        dotenv .env
        source /baz1
        foo2=2
        foo3=3
        foo4=4
        unset rm
        path_add /bar
        dotenv .env2
        source /baz2
        foo5=5
        foo6=6
        "###);
    }

    fn parse(s: String) -> MiseToml {
        let mut cf = MiseToml::init(PathBuf::from("/tmp/.mise.toml").as_path());
        cf.parse(&s).unwrap();
        cf
    }

    fn parse_env(toml: String) -> String {
        parse(toml).env_entries().into_iter().join("\n")
    }
}
