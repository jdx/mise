use eyre::{WrapErr, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use serde::de::Visitor;
use serde::{Deserializer, de};
use serde_derive::Deserialize;
use std::fmt::{Debug, Formatter};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, MutexGuard},
};
use tera::Context as TeraContext;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Key, Value, table, value};
use versions::Versioning;

use crate::cli::args::{BackendArg, ToolVersionType};
use crate::config::config_file::{ConfigFile, TaskConfig, config_trust_root, trust, trust_check};
use crate::config::config_file::{config_root, toml::deserialize_arr};
use crate::config::env_directive::{AgeFormat, EnvDirective, EnvDirectiveOptions, RequiredValue};
use crate::config::settings::SettingsPartial;
use crate::config::{Alias, AliasMap, Config};
use crate::file::{create_dir_all, display_path};
use crate::hooks::{Hook, Hooks};
use crate::redactions::Redactions;
use crate::registry::REGISTRY;
use crate::task::Task;
use crate::tera::{BASE_CONTEXT, get_tera};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource, ToolVersionOptions};
use crate::watch_files::WatchFile;
use crate::{dirs, file};

use super::{ConfigFileType, min_version::MinVersionSpec};

#[derive(Default, Deserialize)]
pub struct MiseToml {
    #[serde(rename = "_")]
    custom: Option<toml::Value>,
    #[serde(default, deserialize_with = "deserialize_min_version")]
    min_version: Option<MinVersionSpec>,
    #[serde(skip)]
    context: TeraContext,
    #[serde(skip)]
    path: PathBuf,
    #[serde(default, alias = "dotenv", deserialize_with = "deserialize_arr")]
    env_file: Vec<String>,
    #[serde(default)]
    env: EnvList,
    #[serde(default, deserialize_with = "deserialize_arr")]
    env_path: Vec<String>,
    #[serde(default)]
    alias: AliasMap,
    #[serde(skip)]
    doc: Mutex<OnceCell<DocumentMut>>,
    #[serde(default)]
    hooks: IndexMap<Hooks, toml::Value>,
    #[serde(default)]
    tools: Mutex<IndexMap<BackendArg, MiseTomlToolList>>,
    #[serde(default)]
    plugins: HashMap<String, String>,
    #[serde(default)]
    redactions: Redactions,
    #[serde(default)]
    task_config: TaskConfig,
    #[serde(default)]
    tasks: Tasks,
    #[serde(default)]
    watch_files: Vec<WatchFile>,
    #[serde(default)]
    vars: EnvList,
    #[serde(default)]
    settings: SettingsPartial,
}

#[derive(Debug, Default, Clone)]
pub struct MiseTomlToolList(Vec<MiseTomlTool>);

#[derive(Debug, Clone)]
pub struct MiseTomlTool {
    pub tt: ToolVersionType,
    pub options: Option<ToolVersionOptions>,
}

#[derive(Debug, Default, Clone)]
pub struct Tasks(pub BTreeMap<String, Task>);

#[derive(Debug, Default, Clone)]
pub struct EnvList(pub(crate) Vec<EnvDirective>);

impl EnvList {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl MiseToml {
    fn enforce_min_version_fallback(body: &str) -> eyre::Result<()> {
        if let Ok(val) = toml::from_str::<toml::Value>(body) {
            if let Some(min_val) = val.get("min_version") {
                let mut hard_req: Option<versions::Versioning> = None;
                let mut soft_req: Option<versions::Versioning> = None;
                match min_val {
                    toml::Value::String(s) => {
                        hard_req = versions::Versioning::new(s);
                    }
                    toml::Value::Table(t) => {
                        if let Some(toml::Value::String(s)) = t.get("hard") {
                            hard_req = versions::Versioning::new(s);
                        }
                        if let Some(toml::Value::String(s)) = t.get("soft") {
                            soft_req = versions::Versioning::new(s);
                        }
                    }
                    _ => {}
                }
                if let Some(spec) =
                    crate::config::config_file::min_version::MinVersionSpec::new(hard_req, soft_req)
                {
                    crate::config::Config::enforce_min_version_spec(&spec)?;
                }
            }
        }
        Ok(())
    }
    fn contains_template_syntax(input: &str) -> bool {
        input.contains("{{") || input.contains("{%") || input.contains("{#")
    }

    pub fn init(path: &Path) -> Self {
        let mut context = BASE_CONTEXT.clone();
        context.insert(
            "config_root",
            config_root::config_root(path).to_str().unwrap(),
        );
        Self {
            path: path.to_path_buf(),
            context,
            ..Default::default()
        }
    }

    pub fn from_file(path: &Path) -> eyre::Result<Self> {
        let body = file::read_to_string(path)?;
        Self::from_str(&body, path)
    }

    pub fn from_str(body: &str, path: &Path) -> eyre::Result<Self> {
        trust_check(path)?;
        trace!("parsing: {}", display_path(path));
        let des = toml::Deserializer::new(body);
        let de_res = serde_ignored::deserialize(des, |p| {
            warn!("unknown field in {}: {p}", display_path(path));
        });
        let mut rf: MiseToml = match de_res {
            Ok(rf) => rf,
            Err(err) => {
                Self::enforce_min_version_fallback(body)?;
                return Err(err.into());
            }
        };
        rf.context = BASE_CONTEXT.clone();
        rf.context
            .insert("config_root", path.parent().unwrap().to_str().unwrap());
        rf.path = path.to_path_buf();
        let project_root = rf.project_root().map(|p| p.to_path_buf());
        for task in rf.tasks.0.values_mut() {
            task.config_source.clone_from(&rf.path);
            task.config_root = project_root.clone();
        }
        // trace!("{}", rf.dump()?);
        Ok(rf)
    }

    fn doc(&self) -> eyre::Result<DocumentMut> {
        self.doc
            .lock()
            .unwrap()
            .get_or_try_init(|| {
                let body = file::read_to_string(&self.path).unwrap_or_default();
                Ok(body.parse()?)
            })
            .cloned()
    }

    fn doc_mut(&self) -> eyre::Result<MutexGuard<'_, OnceCell<DocumentMut>>> {
        self.doc()?;
        Ok(self.doc.lock().unwrap())
    }

    pub fn set_backend_alias(&mut self, fa: &BackendArg, to: &str) -> eyre::Result<()> {
        self.doc_mut()?
            .get_mut()
            .unwrap()
            .entry("alias")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .insert(&fa.short, value(to));
        Ok(())
    }

    pub fn set_alias(&mut self, fa: &BackendArg, from: &str, to: &str) -> eyre::Result<()> {
        self.alias
            .entry(fa.short.to_string())
            .or_default()
            .versions
            .insert(from.into(), to.into());
        self.doc_mut()?
            .get_mut()
            .unwrap()
            .entry("alias")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .entry(&fa.to_string())
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .entry("versions")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .insert(from, value(to));
        Ok(())
    }

    pub fn remove_backend_alias(&mut self, fa: &BackendArg) -> eyre::Result<()> {
        let mut doc = self.doc_mut()?;
        let doc = doc.get_mut().unwrap();
        if let Some(aliases) = doc.get_mut("alias").and_then(|v| v.as_table_mut()) {
            aliases.remove(&fa.short);
            if aliases.is_empty() {
                doc.as_table_mut().remove("alias");
            }
        }
        Ok(())
    }

    pub fn remove_alias(&mut self, fa: &BackendArg, from: &str) -> eyre::Result<()> {
        if let Some(aliases) = self.alias.get_mut(&fa.short) {
            aliases.versions.shift_remove(from);
            if aliases.versions.is_empty() && aliases.backend.is_none() {
                self.alias.shift_remove(&fa.short);
            }
        }
        let mut doc = self.doc_mut()?;
        let doc = doc.get_mut().unwrap();
        if let Some(aliases) = doc.get_mut("alias").and_then(|v| v.as_table_mut()) {
            if let Some(alias) = aliases
                .get_mut(&fa.to_string())
                .and_then(|v| v.as_table_mut())
            {
                if let Some(versions) = alias.get_mut("versions").and_then(|v| v.as_table_mut()) {
                    versions.remove(from);
                    if versions.is_empty() {
                        alias.remove("versions");
                    }
                }
                if alias.is_empty() {
                    aliases.remove(&fa.to_string());
                }
            }
            if aliases.is_empty() {
                doc.as_table_mut().remove("alias");
            }
        }
        Ok(())
    }

    pub fn update_env<V: Into<Value>>(&mut self, key: &str, value: V) -> eyre::Result<()> {
        let mut doc = self.doc_mut()?;
        let mut env_tbl = doc
            .get_mut()
            .unwrap()
            .entry("env")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();
        let key_parts = key.split('.').collect_vec();
        for (i, k) in key_parts.iter().enumerate() {
            if i == key_parts.len() - 1 {
                let k = get_key_with_decor(env_tbl, k);
                env_tbl.insert_formatted(&k, toml_edit::value(value));
                break;
            } else if !env_tbl.contains_key(k) {
                env_tbl.insert_formatted(&Key::from(*k), toml_edit::table());
            }
            env_tbl = env_tbl.get_mut(k).unwrap().as_table_mut().unwrap();
        }
        Ok(())
    }

    pub fn update_env_age(
        &mut self,
        key: &str,
        value: &str,
        format: Option<AgeFormat>,
    ) -> eyre::Result<()> {
        let mut doc = self.doc_mut()?;
        let mut env_tbl = doc
            .get_mut()
            .unwrap()
            .entry("env")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();

        // Create the age inline table
        let mut outer_table = InlineTable::new();

        // Check if we need the complex format or can use simplified form
        match format {
            Some(AgeFormat::Zstd) => {
                // Non-default format, use full form: {age = {value = "...", format = "zstd"}}
                let mut age_table = InlineTable::new();
                age_table.insert("value", value.into());
                age_table.insert("format", "zstd".into());
                outer_table.insert("age", Value::InlineTable(age_table));
            }
            Some(AgeFormat::Raw) | None => {
                // Default format or no format, use simplified form: {age = "..."}
                outer_table.insert("age", value.into());
            }
        }

        let key_parts = key.split('.').collect_vec();
        for (i, k) in key_parts.iter().enumerate() {
            if i == key_parts.len() - 1 {
                let k = get_key_with_decor(env_tbl, k);
                env_tbl
                    .insert_formatted(&k, toml_edit::Item::Value(Value::InlineTable(outer_table)));
                break;
            } else if !env_tbl.contains_key(k) {
                env_tbl.insert_formatted(&Key::from(*k), toml_edit::table());
            }
            env_tbl = env_tbl.get_mut(k).unwrap().as_table_mut().unwrap();
        }
        Ok(())
    }

    pub fn remove_env(&mut self, key: &str) -> eyre::Result<()> {
        let mut doc = self.doc_mut()?;
        let env_tbl = doc
            .get_mut()
            .unwrap()
            .entry("env")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();
        env_tbl.remove(key);
        Ok(())
    }

    fn parse_template(&self, input: &str) -> eyre::Result<String> {
        self.parse_template_with_context(&self.context, input)
    }

    fn parse_template_with_context(
        &self,
        context: &TeraContext,
        input: &str,
    ) -> eyre::Result<String> {
        if !Self::contains_template_syntax(input) {
            return Ok(input.to_string());
        }
        let dir = self.path.parent();
        let output = get_tera(dir).render_str(input, context).wrap_err_with(|| {
            let p = display_path(&self.path);
            eyre!("failed to parse template {input} in {p}")
        })?;
        Ok(output)
    }
}

impl ConfigFile for MiseToml {
    fn config_type(&self) -> ConfigFileType {
        ConfigFileType::MiseToml
    }

    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn min_version(&self) -> Option<&MinVersionSpec> {
        self.min_version.as_ref()
    }

    fn project_root(&self) -> Option<&Path> {
        let filename = self.path.file_name().unwrap_or_default().to_string_lossy();
        match self.path.parent() {
            Some(dir) => match dir {
                dir if dir.starts_with(*dirs::CONFIG) => None,
                dir if dir.starts_with(*dirs::SYSTEM) => None,
                dir if dir == *dirs::HOME => None,
                dir if !filename.starts_with('.')
                    && (dir.ends_with(".mise") || dir.ends_with(".config")) =>
                {
                    dir.parent()
                }
                dir if !filename.starts_with('.') && dir.ends_with(".config/mise") => {
                    dir.parent().unwrap().parent()
                }
                dir if !filename.starts_with('.') && dir.ends_with("mise") => dir.parent(),
                dir => Some(dir),
            },
            None => None,
        }
    }

    fn plugins(&self) -> eyre::Result<HashMap<String, String>> {
        self.plugins
            .clone()
            .into_iter()
            .map(|(k, v)| {
                let v = self.parse_template(&v)?;
                Ok((k, v))
            })
            .collect()
    }

    fn env_entries(&self) -> eyre::Result<Vec<EnvDirective>> {
        let env_entries = self.env.0.iter().cloned();
        let path_entries = self
            .env_path
            .iter()
            .map(|p| EnvDirective::Path(p.clone(), Default::default()))
            .collect_vec();
        let env_files = self
            .env_file
            .iter()
            .map(|p| EnvDirective::File(p.clone(), Default::default()))
            .collect_vec();
        let all = path_entries
            .into_iter()
            .chain(env_files)
            .chain(env_entries)
            .collect::<Vec<_>>();
        Ok(all)
    }

    fn vars_entries(&self) -> eyre::Result<Vec<EnvDirective>> {
        Ok(self.vars.0.clone())
    }

    fn tasks(&self) -> Vec<&Task> {
        self.tasks.0.values().collect()
    }

    fn remove_tool(&self, fa: &BackendArg) -> eyre::Result<()> {
        let mut tools = self.tools.lock().unwrap();
        tools.shift_remove(fa);
        let mut doc = self.doc_mut()?;
        let doc = doc.get_mut().unwrap();
        if let Some(tools) = doc.get_mut("tools") {
            if let Some(tools) = tools.as_table_like_mut() {
                tools.remove(&fa.to_string());
                if tools.is_empty() {
                    doc.as_table_mut().remove("tools");
                }
            }
        }
        Ok(())
    }

    fn replace_versions(&self, ba: &BackendArg, versions: Vec<ToolRequest>) -> eyre::Result<()> {
        trace!("replacing versions {ba:?} {versions:?}");
        let mut tools = self.tools.lock().unwrap();
        let is_tools_sorted = is_tools_sorted(&tools); // was it previously sorted (if so we'll keep it sorted)
        let existing = tools.entry(ba.clone()).or_default();
        let output_empty_opts = |opts: &ToolVersionOptions| {
            if opts.os.is_some() || !opts.install_env.is_empty() {
                return false;
            }
            if let Some(reg_ba) = REGISTRY.get(ba.short.as_str()).and_then(|b| b.ba()) {
                if reg_ba.opts.as_ref().is_some_and(|o| o == opts) {
                    // in this case the options specified are the same as in the registry so output no options and rely on the defaults
                    return true;
                }
            }
            opts.is_empty()
        };
        existing.0 = versions
            .iter()
            .map(|tr| MiseTomlTool::from(tr.clone()))
            .collect();
        trace!("done replacing versions");
        let mut doc = self.doc_mut()?;
        trace!("got doc");
        let tools = doc
            .get_mut()
            .unwrap()
            .entry("tools")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();

        // create a key from the short name preserving any decorations like prefix/suffix if the key already exists
        let key = get_key_with_decor(tools, ba.short.as_str());

        // if a short name is used like "node", make sure we remove any long names like "core:node"
        if ba.short != ba.full() {
            tools.remove(&ba.full());
        }

        if versions.len() == 1 {
            let options = versions[0].options();
            if output_empty_opts(&options) {
                tools.insert_formatted(&key, value(versions[0].version()));
            } else {
                let mut table = InlineTable::new();
                table.insert("version", versions[0].version().into());
                for (k, v) in options.opts {
                    table.insert(k, v.into());
                }
                if let Some(os) = options.os {
                    let mut arr = Array::new();
                    for o in os {
                        arr.push(Value::from(o));
                    }
                    table.insert("os", Value::Array(arr));
                }
                if !options.install_env.is_empty() {
                    let mut env = InlineTable::new();
                    for (k, v) in options.install_env {
                        env.insert(k, v.into());
                    }
                    table.insert("install_env", env.into());
                }
                tools.insert_formatted(&key, table.into());
            }
        } else {
            let mut arr = Array::new();
            for tr in versions {
                let v = tr.version();
                if output_empty_opts(&tr.options()) {
                    arr.push(v.to_string());
                } else {
                    let mut table = InlineTable::new();
                    table.insert("version", v.to_string().into());
                    for (k, v) in tr.options().opts {
                        table.insert(k, v.clone().into());
                    }
                    arr.push(table);
                }
            }
            tools.insert_formatted(&key, Item::Value(Value::Array(arr)));
        }

        if is_tools_sorted {
            tools.sort_values();
        }

        Ok(())
    }

    fn save(&self) -> eyre::Result<()> {
        let contents = self.dump()?;
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }
        file::write(&self.path, contents)?;
        trust(&config_trust_root(&self.path))?;
        Ok(())
    }

    fn dump(&self) -> eyre::Result<String> {
        Ok(self.doc()?.to_string())
    }

    fn source(&self) -> ToolSource {
        ToolSource::MiseToml(self.path.clone())
    }

    fn to_tool_request_set(&self) -> eyre::Result<ToolRequestSet> {
        let source = ToolSource::MiseToml(self.path.clone());
        let mut trs = ToolRequestSet::new();
        let tools = self.tools.lock().unwrap();
        let mut context = self.context.clone();
        if context.get("vars").is_none() {
            if let Some(config) = Config::maybe_get() {
                if let Some(vars_results) = config.vars_results_cached() {
                    let vars = vars_results
                        .vars
                        .iter()
                        .map(|(k, (v, _))| (k.clone(), v.clone()))
                        .collect::<IndexMap<_, _>>();
                    context.insert("vars", &vars);
                } else if !config.vars.is_empty() {
                    context.insert("vars", &config.vars);
                }
            }
        }
        for (ba, tvp) in tools.iter() {
            for tool in &tvp.0 {
                let version = self.parse_template_with_context(&context, &tool.tt.to_string())?;
                let tvr = if let Some(mut options) = tool.options.clone() {
                    for v in options.opts.values_mut() {
                        *v = self.parse_template_with_context(&context, v)?;
                    }
                    let mut ba = ba.clone();
                    let mut ba_opts = ba.opts().clone();
                    ba_opts.merge(&options.opts);
                    ba.set_opts(Some(ba_opts.clone()));
                    ToolRequest::new_opts(ba.into(), &version, options, source.clone())?
                } else {
                    ToolRequest::new(ba.clone().into(), &version, source.clone())?
                };
                trs.add_version(tvr, &source);
            }
        }
        Ok(trs)
    }

    fn aliases(&self) -> eyre::Result<AliasMap> {
        self.alias
            .clone()
            .iter()
            .map(|(k, v)| {
                let versions = v
                    .clone()
                    .versions
                    .into_iter()
                    .map(|(k, v)| {
                        let v = self.parse_template(&v)?;
                        Ok::<(String, String), eyre::Report>((k, v))
                    })
                    .collect::<eyre::Result<IndexMap<_, _>>>()?;
                Ok((
                    k.clone(),
                    Alias {
                        backend: v.backend.clone(),
                        versions,
                    },
                ))
            })
            .collect()
    }

    fn task_config(&self) -> &TaskConfig {
        &self.task_config
    }

    fn redactions(&self) -> &Redactions {
        &self.redactions
    }

    fn watch_files(&self) -> eyre::Result<Vec<WatchFile>> {
        self.watch_files
            .iter()
            .map(|wf| {
                Ok(WatchFile {
                    patterns: wf
                        .patterns
                        .iter()
                        .map(|p| self.parse_template(p))
                        .collect::<eyre::Result<Vec<String>>>()?,
                    run: self.parse_template(&wf.run)?,
                })
            })
            .collect()
    }

    fn hooks(&self) -> eyre::Result<Vec<Hook>> {
        Ok(self
            .hooks
            .iter()
            .map(|(hook, val)| {
                let mut hooks = Hook::from_toml(*hook, val.clone())?;
                for hook in hooks.iter_mut() {
                    hook.script = self.parse_template(&hook.script)?;
                    if let Some(shell) = &hook.shell {
                        hook.shell = Some(self.parse_template(shell)?);
                    }
                }
                eyre::Ok(hooks)
            })
            .collect::<eyre::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect())
    }
}

/// Returns a [`toml_edit::Key`] from the given `key`.
/// Preserves any surrounding whitespace (e.g. comments) if the key already exists in the provided [`toml_edit::Table`].
fn get_key_with_decor(table: &toml_edit::Table, key: &str) -> Key {
    let mut key = Key::from(key);
    if let Some((k, _)) = table.get_key_value(&key) {
        if let Some(prefix) = k.leaf_decor().prefix() {
            key.leaf_decor_mut().set_prefix(prefix.clone());
        }
        if let Some(suffix) = k.leaf_decor().suffix() {
            key.leaf_decor_mut().set_suffix(suffix.clone());
        }
    }
    key
}

impl Debug for MiseToml {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let tools = self.to_tool_request_set().unwrap().to_string();
        let title = format!("MiseToml({}): {tools}", &display_path(&self.path));
        let mut d = f.debug_struct(&title);
        if let Some(min_version) = &self.min_version {
            d.field("min_version", &min_version.to_string());
        }
        if !self.env_file.is_empty() {
            d.field("env_file", &self.env_file);
        }
        if let Ok(env) = self.env_entries() {
            if !env.is_empty() {
                d.field("env", &env);
            }
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
            custom: self.custom.clone(),
            min_version: self.min_version.clone(),
            context: self.context.clone(),
            path: self.path.clone(),
            env_file: self.env_file.clone(),
            env: self.env.clone(),
            env_path: self.env_path.clone(),
            alias: self.alias.clone(),
            doc: Mutex::new(self.doc.lock().unwrap().clone()),
            hooks: self.hooks.clone(),
            tools: Mutex::new(self.tools.lock().unwrap().clone()),
            redactions: self.redactions.clone(),
            plugins: self.plugins.clone(),
            tasks: self.tasks.clone(),
            task_config: self.task_config.clone(),
            settings: self.settings.clone(),
            watch_files: self.watch_files.clone(),
            vars: self.vars.clone(),
        }
    }
}

impl From<ToolRequest> for MiseTomlTool {
    fn from(tr: ToolRequest) -> Self {
        match tr {
            ToolRequest::Version {
                version,
                options,
                backend: _backend,
                source: _source,
            } => Self {
                tt: ToolVersionType::Version(version),
                options: if options.is_empty() {
                    None
                } else {
                    Some(options)
                },
            },
            ToolRequest::Path {
                path,
                options,
                backend: _backend,
                source: _source,
            } => Self {
                tt: ToolVersionType::Path(path),
                options: if options.is_empty() {
                    None
                } else {
                    Some(options)
                },
            },
            ToolRequest::Prefix {
                prefix,
                options,
                backend: _backend,
                source: _source,
            } => Self {
                tt: ToolVersionType::Prefix(prefix),
                options: if options.is_empty() {
                    None
                } else {
                    Some(options)
                },
            },
            ToolRequest::Ref {
                ref_,
                ref_type,
                options,
                backend: _backend,
                source: _source,
            } => Self {
                tt: ToolVersionType::Ref(ref_, ref_type),
                options: if options.is_empty() {
                    None
                } else {
                    Some(options)
                },
            },
            ToolRequest::Sub {
                sub,
                options,
                orig_version,
                backend: _backend,
                source: _source,
            } => Self {
                tt: ToolVersionType::Sub { sub, orig_version },
                options: if options.is_empty() {
                    None
                } else {
                    Some(options)
                },
            },
            ToolRequest::System {
                options,
                backend: _backend,
                source: _source,
            } => Self {
                tt: ToolVersionType::System,
                options: if options.is_empty() {
                    None
                } else {
                    Some(options)
                },
            },
        }
    }
}

fn deserialize_min_version<'de, D>(deserializer: D) -> Result<Option<MinVersionSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct MinVersionVisitor;

    impl<'de> Visitor<'de> for MinVersionVisitor {
        type Value = Option<MinVersionSpec>;

        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("string or table for min_version")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(self)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let version = Versioning::new(v)
                .ok_or_else(|| versions::Error::IllegalVersioning(v.to_string()))
                .map_err(E::custom)?;
            Ok(MinVersionSpec::new(Some(version), None))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            self.visit_str(&v)
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            let mut hard: Option<Versioning> = None;
            let mut soft: Option<Versioning> = None;
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "hard" => {
                        if hard.is_some() {
                            return Err(de::Error::duplicate_field("hard"));
                        }
                        let value: String = map.next_value()?;
                        let version = Versioning::new(&value)
                            .ok_or_else(|| versions::Error::IllegalVersioning(value.clone()))
                            .map_err(de::Error::custom)?;
                        hard = Some(version);
                    }
                    "soft" => {
                        if soft.is_some() {
                            return Err(de::Error::duplicate_field("soft"));
                        }
                        let value: String = map.next_value()?;
                        let version = Versioning::new(&value)
                            .ok_or_else(|| versions::Error::IllegalVersioning(value.clone()))
                            .map_err(de::Error::custom)?;
                        soft = Some(version);
                    }
                    other => {
                        return Err(de::Error::unknown_field(other, &["hard", "soft"]));
                    }
                }
            }
            Ok(MinVersionSpec::new(hard, soft))
        }
    }

    deserializer.deserialize_option(MinVersionVisitor)
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
                            #[serde(untagged)]
                            enum MiseTomlEnvDirective {
                                Single {
                                    #[serde(alias = "path")]
                                    value: String,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                Multiple {
                                    #[serde(alias = "value", alias = "path", alias = "paths")]
                                    values: Vec<String>,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                            }

                            impl FromStr for MiseTomlEnvDirective {
                                type Err = String;
                                fn from_str(s: &str) -> Result<Self, Self::Err> {
                                    Ok(MiseTomlEnvDirective::Single {
                                        value: s.to_string(),
                                        options: Default::default(),
                                    })
                                }
                            }

                            struct EnvDirectivePythonVenv {
                                path: String,
                                create: bool,
                                python: Option<String>,
                                uv_create_args: Option<Vec<String>>,
                                python_create_args: Option<Vec<String>>,
                            }

                            #[derive(Deserialize, Default)]
                            #[serde(deny_unknown_fields)]
                            struct EnvDirectivePython {
                                #[serde(default)]
                                venv: Option<EnvDirectivePythonVenv>,
                            }

                            #[derive(Deserialize)]
                            struct EnvDirectives {
                                #[serde(default, deserialize_with = "deserialize_arr")]
                                path: Vec<MiseTomlEnvDirective>,
                                #[serde(default, deserialize_with = "deserialize_arr")]
                                file: Vec<MiseTomlEnvDirective>,
                                #[serde(default, deserialize_with = "deserialize_arr")]
                                source: Vec<MiseTomlEnvDirective>,
                                #[serde(default)]
                                python: EnvDirectivePython,
                                #[serde(flatten)]
                                other: BTreeMap<String, toml::Value>,
                            }

                            impl<'de> de::Deserialize<'de> for EnvDirectivePythonVenv {
                                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                                where
                                    D: Deserializer<'de>,
                                {
                                    struct EnvDirectivePythonVenvVisitor;

                                    impl<'de> Visitor<'de> for EnvDirectivePythonVenvVisitor {
                                        type Value = EnvDirectivePythonVenv;
                                        fn expecting(
                                            &self,
                                            formatter: &mut Formatter,
                                        ) -> std::fmt::Result
                                        {
                                            formatter.write_str("python venv directive")
                                        }

                                        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                                        where
                                            E: de::Error,
                                        {
                                            Ok(EnvDirectivePythonVenv {
                                                path: v.into(),
                                                create: false,
                                                python: None,
                                                uv_create_args: None,
                                                python_create_args: None,
                                            })
                                        }

                                        fn visit_map<M>(
                                            self,
                                            mut map: M,
                                        ) -> Result<Self::Value, M::Error>
                                        where
                                            M: de::MapAccess<'de>,
                                        {
                                            let mut path = None;
                                            let mut create = false;
                                            let mut python = None;
                                            let mut uv_create_args = None;
                                            let mut python_create_args = None;
                                            while let Some(key) = map.next_key::<String>()? {
                                                match key.as_str() {
                                                    "path" => {
                                                        path = Some(map.next_value()?);
                                                    }
                                                    "create" => {
                                                        create = map.next_value()?;
                                                    }
                                                    "python" => {
                                                        python = Some(map.next_value()?);
                                                    }
                                                    "uv_create_args" => {
                                                        uv_create_args = Some(map.next_value()?);
                                                    }
                                                    "python_create_args" => {
                                                        python_create_args =
                                                            Some(map.next_value()?);
                                                    }
                                                    _ => {
                                                        return Err(de::Error::unknown_field(
                                                            &key,
                                                            &["path", "create"],
                                                        ));
                                                    }
                                                }
                                            }
                                            let path = path
                                                .ok_or_else(|| de::Error::missing_field("path"))?;
                                            Ok(EnvDirectivePythonVenv {
                                                path,
                                                create,
                                                python,
                                                uv_create_args,
                                                python_create_args,
                                            })
                                        }
                                    }

                                    const FIELDS: &[&str] = &["path", "create"];
                                    deserializer.deserialize_struct(
                                        "PythonVenv",
                                        FIELDS,
                                        EnvDirectivePythonVenvVisitor,
                                    )
                                }
                            }

                            fn flatten_directives<F>(
                                directives: Vec<MiseTomlEnvDirective>,
                                constructor: F,
                            ) -> impl Iterator<Item = EnvDirective>
                            where
                                F: Fn(String, EnvDirectiveOptions) -> EnvDirective + 'static,
                            {
                                directives.into_iter().flat_map(move |d| match d {
                                    MiseTomlEnvDirective::Single { value, options } => {
                                        vec![constructor(value, options)]
                                    }
                                    MiseTomlEnvDirective::Multiple { values, options } => values
                                        .into_iter()
                                        .map(|v| constructor(v, options.clone()))
                                        .collect(),
                                })
                            }

                            let directives = map.next_value::<EnvDirectives>()?;
                            // TODO: parse these in the order they're defined somehow
                            env.extend(flatten_directives(directives.path, EnvDirective::Path));
                            env.extend(flatten_directives(directives.file, EnvDirective::File));
                            env.extend(flatten_directives(directives.source, EnvDirective::Source));
                            for (key, value) in directives.other {
                                env.push(EnvDirective::Module(key, value, Default::default()));
                            }
                            if let Some(venv) = directives.python.venv {
                                env.push(EnvDirective::PythonVenv {
                                    path: venv.path,
                                    create: venv.create,
                                    python: venv.python,
                                    uv_create_args: venv.uv_create_args,
                                    python_create_args: venv.python_create_args,
                                    options: EnvDirectiveOptions {
                                        tools: true,
                                        redact: Some(false),
                                        required: RequiredValue::False,
                                    },
                                });
                            }
                        }
                        _ => {
                            #[derive(Deserialize)]
                            #[serde(untagged)]
                            pub enum PrimitiveVal {
                                Str(String),
                                Int(i64),
                                Bool(bool),
                            }
                            #[derive(Deserialize)]
                            #[serde(untagged)]
                            enum Val {
                                AgeComplex {
                                    age: AgeComplexVal,
                                },
                                AgeWithOptions {
                                    age: String,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                Map {
                                    value: PrimitiveVal,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                OptionsOnly {
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                Primitive(PrimitiveVal),
                            }

                            #[derive(Deserialize)]
                            struct AgeComplexVal {
                                value: String,
                                #[serde(default)]
                                format: Option<AgeFormat>,
                                #[serde(flatten)]
                                options: EnvDirectiveOptions,
                            }
                            let val_result = map.next_value::<Val>()?;

                            // Handle Age variants separately since they create different directive types
                            match &val_result {
                                Val::AgeComplex { age } => {
                                    let directive = EnvDirective::Age {
                                        key: key.clone(),
                                        value: age.value.clone(),
                                        format: age.format.clone(),
                                        options: age.options.clone(),
                                    };
                                    env.push(directive);
                                    continue;
                                }
                                Val::AgeWithOptions { age, options } => {
                                    let directive = EnvDirective::Age {
                                        key: key.clone(),
                                        value: age.clone(),
                                        format: None, // Default format for simplified syntax with options
                                        options: options.clone(),
                                    };
                                    env.push(directive);
                                    continue;
                                }
                                _ => {}
                            }

                            let (value, options) = match val_result {
                                Val::Primitive(p) => (Some(p), EnvDirectiveOptions::default()),
                                Val::Map { value, options } => (Some(value), options),
                                Val::OptionsOnly { options } => (None, options),
                                Val::AgeComplex { .. } | Val::AgeWithOptions { .. } => {
                                    unreachable!() // Already handled above
                                }
                            };

                            // Validate that required cannot be used with any value
                            if options.required.is_required() {
                                match &value {
                                    Some(_) => {
                                        return Err(serde::de::Error::custom(format!(
                                            "Environment variable '{}' cannot have both 'value' and 'required'. The 'required' flag means the variable must be defined elsewhere (in the environment or a later config file). Remove either the 'value' field or the 'required' flag.",
                                            key
                                        )));
                                    }
                                    None => {
                                        // Required without a value is valid - it means the variable must be defined elsewhere
                                    }
                                }
                            }
                            let directive = match value {
                                Some(PrimitiveVal::Str(s)) => EnvDirective::Val(key, s, options),
                                Some(PrimitiveVal::Int(i)) => {
                                    EnvDirective::Val(key, i.to_string(), options)
                                }
                                Some(PrimitiveVal::Bool(true)) => {
                                    EnvDirective::Val(key, "true".to_string(), options)
                                }
                                Some(PrimitiveVal::Bool(false)) => EnvDirective::Rm(key, options),
                                None => {
                                    // No value provided - this creates a required variable that must be defined elsewhere
                                    if !options.required.is_required() {
                                        return Err(serde::de::Error::custom(format!(
                                            "Environment variable '{}' has no value. Either provide a value or set required=true to indicate it must be defined elsewhere.",
                                            key
                                        )));
                                    }
                                    // For required variables without a value, we create a Required directive
                                    EnvDirective::Required(key, options)
                                }
                            };
                            env.push(directive);
                        }
                    }
                }
                Ok(EnvList(env))
            }
        }

        deserializer.deserialize_any(EnvManVisitor)
    }
}

impl<'de> de::Deserialize<'de> for MiseTomlToolList {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct MiseTomlToolListVisitor;

        impl<'de> Visitor<'de> for MiseTomlToolListVisitor {
            type Value = MiseTomlToolList;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("tool list")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let tt: ToolVersionType = v
                    .parse()
                    .map_err(|e| de::Error::custom(format!("invalid tool: {e}")))?;
                Ok(MiseTomlToolList(vec![MiseTomlTool { tt, options: None }]))
            }

            fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
            where
                S: de::SeqAccess<'de>,
            {
                let mut tools = vec![];
                while let Some(tool) = seq.next_element::<MiseTomlTool>()? {
                    tools.push(tool);
                }
                Ok(MiseTomlToolList(tools))
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut options: ToolVersionOptions = Default::default();
                let mut tt: Option<ToolVersionType> = None;
                while let Some((k, v)) = map.next_entry::<String, toml::Value>()? {
                    match k.as_str() {
                        "version" => {
                            tt = Some(v.as_str().unwrap().parse().map_err(de::Error::custom)?);
                        }
                        "path" | "prefix" | "ref" => {
                            tt = Some(
                                format!("{k}:{}", v.as_str().unwrap())
                                    .parse()
                                    .map_err(de::Error::custom)?,
                            );
                        }
                        "os" => match v {
                            toml::Value::Array(s) => {
                                options.os = Some(
                                    s.iter().map(|v| v.as_str().unwrap().to_string()).collect(),
                                );
                            }
                            toml::Value::String(s) => {
                                // Convert {{version}} to {version} for backend templating
                                let s = s.replace("{{version}}", "{version}");
                                options.opts.insert(k, s);
                            }
                            _ => {
                                return Err(de::Error::custom("os must be a string or array"));
                            }
                        },
                        "install_env" => match v {
                            toml::Value::Table(env) => {
                                for (k, v) in env {
                                    match v {
                                        toml::Value::Boolean(v) => {
                                            options.install_env.insert(k, v.to_string());
                                        }
                                        toml::Value::Integer(v) => {
                                            options.install_env.insert(k, v.to_string());
                                        }
                                        toml::Value::String(v) => {
                                            options.install_env.insert(k, v);
                                        }
                                        _ => {
                                            return Err(de::Error::custom("invalid value type"));
                                        }
                                    }
                                }
                            }
                            _ => {
                                return Err(de::Error::custom("env must be a table"));
                            }
                        },
                        _ => {
                            // Handle nested structures
                            match v {
                                toml::Value::Table(_) => {
                                    // Store as TOML string, will be flattened later
                                    options.opts.insert(k, v.to_string());
                                }
                                toml::Value::String(s) => {
                                    // Convert {{version}} to {version} for backend templating
                                    let s = s.replace("{{version}}", "{version}");
                                    options.opts.insert(k, s);
                                }
                                toml::Value::Boolean(b) => {
                                    options.opts.insert(k, b.to_string());
                                }
                                toml::Value::Integer(i) => {
                                    options.opts.insert(k, i.to_string());
                                }
                                toml::Value::Float(f) => {
                                    options.opts.insert(k, f.to_string());
                                }
                                _ => {
                                    return Err(de::Error::custom("invalid value type"));
                                }
                            }
                        }
                    }
                }
                if let Some(tt) = tt {
                    Ok(MiseTomlToolList(vec![MiseTomlTool {
                        tt,
                        options: Some(options),
                    }]))
                } else {
                    Err(de::Error::custom("missing version"))
                }
            }
        }

        deserializer.deserialize_any(MiseTomlToolListVisitor)
    }
}

impl<'de> de::Deserialize<'de> for MiseTomlTool {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct MiseTomlToolVisitor;

        impl<'de> Visitor<'de> for MiseTomlToolVisitor {
            type Value = MiseTomlTool;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("tool definition")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let tt: ToolVersionType = v
                    .parse()
                    .map_err(|e| de::Error::custom(format!("invalid tool: {e}")))?;
                Ok(MiseTomlTool { tt, options: None })
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut options: ToolVersionOptions = Default::default();
                let mut tt = ToolVersionType::System;
                while let Some((k, v)) = map.next_entry::<String, toml::Value>()? {
                    match k.as_str() {
                        "version" => {
                            tt = v.as_str().unwrap().parse().map_err(de::Error::custom)?;
                        }
                        "path" | "prefix" | "ref" => {
                            tt = format!("{k}:{}", v.as_str().unwrap())
                                .parse()
                                .map_err(de::Error::custom)?;
                        }
                        "os" => match v {
                            toml::Value::Array(s) => {
                                options.os = Some(
                                    s.iter().map(|v| v.as_str().unwrap().to_string()).collect(),
                                );
                            }
                            toml::Value::String(s) => {
                                // Convert {{version}} to {version} for backend templating
                                let s = s.replace("{{version}}", "{version}");
                                options.opts.insert(k, s);
                            }
                            _ => {
                                return Err(de::Error::custom("os must be a string or array"));
                            }
                        },
                        "install_env" => match v {
                            toml::Value::Table(env) => {
                                for (k, v) in env {
                                    match v {
                                        toml::Value::Boolean(v) => {
                                            options.install_env.insert(k, v.to_string());
                                        }
                                        toml::Value::Integer(v) => {
                                            options.install_env.insert(k, v.to_string());
                                        }
                                        toml::Value::String(v) => {
                                            options.install_env.insert(k, v);
                                        }
                                        _ => {
                                            return Err(de::Error::custom("invalid value type"));
                                        }
                                    }
                                }
                            }
                            _ => {
                                return Err(de::Error::custom("env must be a table"));
                            }
                        },
                        _ => {
                            options.opts.insert(k, v.as_str().unwrap().to_string());
                        }
                    }
                }
                Ok(MiseTomlTool {
                    tt,
                    options: Some(options),
                })
            }
        }

        deserializer.deserialize_any(MiseTomlToolVisitor)
    }
}

impl<'de> de::Deserialize<'de> for Tasks {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct TasksVisitor;

        impl<'de> Visitor<'de> for TasksVisitor {
            type Value = Tasks;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("task, string, or array of strings")
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                struct TaskDef(Task);
                impl<'de> de::Deserialize<'de> for TaskDef {
                    fn deserialize<D>(deserializer: D) -> std::result::Result<TaskDef, D::Error>
                    where
                        D: de::Deserializer<'de>,
                    {
                        struct TaskDefVisitor;
                        impl<'de> Visitor<'de> for TaskDefVisitor {
                            type Value = TaskDef;
                            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                                formatter.write_str("task definition")
                            }

                            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                            where
                                E: de::Error,
                            {
                                Ok(TaskDef(Task {
                                    run: vec![crate::task::RunEntry::Script(v.to_string())],
                                    ..Default::default()
                                }))
                            }

                            fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
                            where
                                S: de::SeqAccess<'de>,
                            {
                                let mut run = vec![];
                                while let Some(s) = seq.next_element::<crate::task::RunEntry>()? {
                                    run.push(s);
                                }
                                Ok(TaskDef(Task {
                                    run,
                                    ..Default::default()
                                }))
                            }

                            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
                            where
                                M: de::MapAccess<'de>,
                            {
                                let t = de::Deserialize::deserialize(
                                    de::value::MapAccessDeserializer::new(map),
                                )?;
                                Ok(TaskDef(t))
                            }
                        }
                        deserializer.deserialize_any(TaskDefVisitor)
                    }
                }
                let mut tasks = BTreeMap::new();
                while let Some(name) = map.next_key::<String>()? {
                    let mut task = map.next_value::<TaskDef>()?.0;
                    task.name.clone_from(&name);
                    tasks.insert(name, task);
                }
                Ok(Tasks(tasks))
            }
        }

        deserializer.deserialize_any(TasksVisitor)
    }
}

impl<'de> de::Deserialize<'de> for BackendArg {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct BackendArgVisitor;

        impl Visitor<'_> for BackendArgVisitor {
            type Value = BackendArg;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("backend argument")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(v.into())
            }
        }

        deserializer.deserialize_any(BackendArgVisitor)
    }
}

impl<'de> de::Deserialize<'de> for Alias {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct AliasVisitor;

        impl<'de> Visitor<'de> for AliasVisitor {
            type Value = Alias;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("alias")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Alias {
                    backend: Some(v.to_string()),
                    ..Default::default()
                })
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut backend = None;
                let mut versions = IndexMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "backend" => {
                            backend = Some(map.next_value()?);
                        }
                        "versions" => {
                            versions = map.next_value()?;
                        }
                        _ => {
                            deprecated!(
                                "TOOL_VERSION_ALIASES",
                                "tool version aliases should be `alias.<TOOL>.versions.<FROM> = <TO>`, not `alias.<TOOL>.<FROM> = <TO>`"
                            );
                            versions.insert(key, map.next_value()?);
                        }
                    }
                }
                Ok(Alias { backend, versions })
            }
        }

        deserializer.deserialize_any(AliasVisitor)
    }
}

fn is_tools_sorted(tools: &IndexMap<BackendArg, MiseTomlToolList>) -> bool {
    let mut last = None;
    for k in tools.keys() {
        if let Some(last) = last {
            if k < last {
                return false;
            }
        }
        last = Some(k);
    }
    true
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::sync::Arc;

    use indoc::formatdoc;
    use insta::{assert_debug_snapshot, assert_snapshot};
    use test_log::test;

    use crate::test::replace_path;
    use crate::toolset::ToolRequest;
    use crate::{config::Config, dirs::CWD};

    use super::*;

    #[tokio::test]
    async fn test_fixture() {
        let _config = Config::get().await.unwrap();
        let cf = MiseToml::from_file(&dirs::HOME.join("fixtures/.mise.toml")).unwrap();

        assert_debug_snapshot!(cf.env_entries().unwrap());
        assert_debug_snapshot!(cf.plugins().unwrap());
        assert_snapshot!(replace_path(&format!(
            "{:#?}",
            cf.to_tool_request_set().unwrap()
        )));
        assert_debug_snapshot!(cf.alias);

        assert_snapshot!(replace_path(&format!("{:#?}", &cf)));
    }

    #[tokio::test]
    async fn test_env() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
        min_version = "2024.1.1"
        [env]
        foo="bar"
        foo2='qux\nquux'
        foo3="qux\nquux"
        "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let dump = cf.dump().unwrap();
        let env = parse_env(file::read_to_string(&p).unwrap());

        assert_debug_snapshot!(env, @r#""foo=bar\nfoo2=qux\\nquux\nfoo3=qux\nquux""#);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        with_settings!({
            assert_snapshot!(dump);
            assert_snapshot!(cf);
            assert_debug_snapshot!(cf);
        });
    }

    #[tokio::test]
    async fn test_env_array_valid() {
        let _config = Config::get().await.unwrap();
        let env = parse_env(formatdoc! {r#"
        [[env]]
        foo="bar"

        [[env]]
        bar="baz"

        [[env]]
        foo2='qux\nquux'
        bar2="qux\nquux"
        "#});

        assert_snapshot!(env, @r"
        foo=bar
        bar=baz
        foo2=qux\nquux
        bar2=qux
        quux
        ");
    }

    #[tokio::test]
    async fn test_path_dirs() {
        let _config = Config::get().await.unwrap();
        let env = parse_env(formatdoc! {r#"
            env_path=["/foo", "./bar"]
            [env]
            foo="bar"
            "#});

        assert_snapshot!(env, @r#"
        _.path = "/foo"
        _.path = "./bar"
        foo=bar
        "#);

        let env = parse_env(formatdoc! {r#"
            env_path="./bar"
            "#});
        assert_snapshot!(env, @r#"_.path = "./bar""#);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.path = "./bar"
            "#});
        assert_debug_snapshot!(env, @r#""_.path = \"./bar\"""#);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.path = ["/foo", "./bar"]
            "#});
        assert_snapshot!(env, @r#"
        _.path = "/foo"
        _.path = "./bar"
        "#);

        let env = parse_env(formatdoc! {r#"
            [[env]]
            _.path = "/foo"
            [[env]]
            _.path = "./bar"
            "#});
        assert_snapshot!(env, @r#"
        _.path = "/foo"
        _.path = "./bar"
        "#);

        let env = parse_env(formatdoc! {r#"
            env_path = "/foo"
            [env]
            _.path = "./bar"
            "#});
        assert_snapshot!(env, @r#"
        _.path = "/foo"
        _.path = "./bar"
        "#);
    }

    #[tokio::test]
    async fn test_env_file() {
        let _config = Config::get().await.unwrap();
        let env = parse_env(formatdoc! {r#"
            env_file = ".env"
            "#});

        assert_debug_snapshot!(env, @r#""_.file = \".env\"""#);

        let env = parse_env(formatdoc! {r#"
            env_file=[".env", ".env2"]
            "#});
        assert_debug_snapshot!(env, @r#""_.file = \".env\"\n_.file = \".env2\"""#);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.file = ".env"
            "#});
        assert_debug_snapshot!(env, @r#""_.file = \".env\"""#);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.file = [".env", ".env2"]
            "#});
        assert_debug_snapshot!(env, @r#""_.file = \".env\"\n_.file = \".env2\"""#);

        let env = parse_env(formatdoc! {r#"
            dotenv = ".env"
            [env]
            _.file = ".env2"
            "#});
        assert_debug_snapshot!(env, @r#""_.file = \".env\"\n_.file = \".env2\"""#);
    }

    #[tokio::test]
    async fn test_set_alias() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [alias.node.versions]
            16 = "16.0.0"
            18 = "18.0.0"
        "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        let node = "node".into();
        let python = "python".into();
        cf.set_alias(&node, "18", "18.0.1").unwrap();
        cf.set_alias(&node, "20", "20.0.0").unwrap();
        cf.set_alias(&python, "3.10", "3.10.0").unwrap();

        assert_debug_snapshot!(cf.alias);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf);
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_remove_alias() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [alias.node.versions]
            16 = "16.0.0"
            18 = "18.0.0"

            [alias.python.versions]
            "3.10" = "3.10.0"
            "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        let node = "node".into();
        let python = "python".into();
        cf.remove_alias(&node, "16").unwrap();
        cf.remove_alias(&python, "3.10").unwrap();

        assert_debug_snapshot!(cf.alias);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump().unwrap());
        assert_snapshot!(cf);
        assert_debug_snapshot!(cf);
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions() {
        let _config = Config::get().await.unwrap();
        let p = PathBuf::from("/tmp/.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            node = ["16.0.0", "18.0.0"]
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let node = "node".into();
        cf.replace_versions(
            &node,
            vec![
                ToolRequest::new(Arc::new("node".into()), "16.0.1", ToolSource::Unknown).unwrap(),
                ToolRequest::new(Arc::new("node".into()), "18.0.1", ToolSource::Unknown).unwrap(),
            ],
        )
        .unwrap();

        assert_debug_snapshot!(cf.to_toolset().unwrap());
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump().unwrap());
        assert_snapshot!(cf);
        assert_debug_snapshot!(cf);
        file::remove_all(&p).unwrap();
    }

    #[tokio::test]
    async fn test_remove_plugin() {
        let _config = Config::get().await.unwrap();
        let p = PathBuf::from("/tmp/.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            node = ["16.0.0", "18.0.0"]
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        cf.remove_tool(&"node".into()).unwrap();

        assert_debug_snapshot!(cf.to_toolset().unwrap());
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump().unwrap());
        assert_snapshot!(cf);
        assert_debug_snapshot!(cf);
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
        assert_snapshot!(parse_env(toml), @r#"
        foo1=1
        unset rm
        _.path = "/foo"
        _.file = ".env"
        foo2=2
        foo3=3
        "#);
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
        assert_snapshot!(parse_env(toml), @r#"
        foo1=1
        unset rm
        _.path = "/foo"
        _.file = ".env"
        _.source = "/baz1"
        foo2=2
        foo3=3
        foo4=4
        unset rm
        _.path = "/bar"
        _.file = ".env2"
        _.source = "/baz2"
        foo5=5
        foo6=6
        "#);
    }

    fn parse(s: String) -> MiseToml {
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(&p, s).unwrap();
        let cfg = MiseToml::from_file(&p).unwrap();
        file::remove_file(&p).unwrap();

        cfg
    }

    fn parse_env(toml: String) -> String {
        parse(toml).env_entries().unwrap().into_iter().join("\n")
    }
}
