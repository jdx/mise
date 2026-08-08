use eyre::{WrapErr, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use path_absolutize::Absolutize;
use serde::Deserialize;
use serde::de::Visitor;
use serde::{Deserializer, de};
use std::fmt::{Debug, Formatter};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, MutexGuard},
};
use tera::Context as TeraContext;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Key, Value, table, value};
use versions::Versioning;

use crate::backend::unalias_backend;
use crate::cli::args::BackendArg;
use crate::config::config_file::{
    ConfigFile, TaskConfig, config_trust_root, is_ignored, trust, trust_check,
};
use crate::config::config_file::{config_root, toml::deserialize_arr};
use crate::config::env_directive::{
    AgeFormat, EnvDirective, EnvDirectiveOptions, EnvValue, RequiredValue,
};
use crate::config::settings::SettingsPartial;
use crate::config::{Alias, AliasMap, Config, Settings};
use crate::deps::DepsConfig;
use crate::env_diff::EnvMap;
use crate::file::{create_dir_all, display_path};
use crate::hooks::{Hook, HookDef, Hooks};
use crate::oci::OciConfig;
use crate::redactions::Redactions;
use crate::registry::REGISTRY;
use crate::system::{BootstrapTomlConfig, DotfilesTomlConfig};
use crate::task::workspace::WorkspaceProjectOverride;
use crate::task::{Task, TaskTemplate, TaskTomlBoolPresence};
use crate::tera::{BASE_CONTEXT, contains_template_syntax, get_tera, render_str};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource, ToolVersionOptions};
use crate::watch_files::WatchFile;
use crate::{env, file};

use super::diagnostic::toml_parse_error;
use super::min_version::MinVersionSpec;

const LEGACY_ENV_KEYS_DEPRECATED_WARN_AT: &str = "2026.4.17";
const LEGACY_ENV_KEYS_DEPRECATED_REMOVE_AT: &str = "2027.4.0";

/// Convert a `toml::Value` to a `toml_edit::Value` for serialization.
fn toml_value_to_edit(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::from(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => Value::from(f),
        toml::Value::Boolean(b) => Value::from(b),
        toml::Value::Datetime(dt) => {
            // Parse the datetime string back into a toml_edit datetime
            dt.to_string()
                .parse::<toml_edit::Datetime>()
                .map(Value::from)
                .unwrap_or_else(|_| Value::from(dt.to_string()))
        }
        toml::Value::Array(arr) => {
            let mut edit_arr = Array::new();
            for item in arr {
                edit_arr.push(toml_value_to_edit(item));
            }
            Value::Array(edit_arr)
        }
        toml::Value::Table(table) => {
            let mut edit_table = InlineTable::new();
            for (k, v) in table {
                edit_table.insert(k, toml_value_to_edit(v));
            }
            Value::InlineTable(edit_table)
        }
    }
}

fn normalize_option_template_value(value: toml::Value) -> toml::Value {
    match value {
        toml::Value::String(s) => toml::Value::String(s.replace("{{version}}", "{version}")),
        value => value,
    }
}

fn should_normalize_option_template(key: &str) -> bool {
    !matches!(key, "os" | "depends" | "install_env") && !key.starts_with("install_env.")
}

fn insert_tool_option<E>(
    options: &mut ToolVersionOptions,
    key: String,
    value: toml::Value,
) -> std::result::Result<(), E>
where
    E: de::Error,
{
    let value = if should_normalize_option_template(&key) {
        normalize_option_template_value(value)
    } else {
        value
    };
    options.insert_option(key, value).map_err(de::Error::custom)
}

#[derive(Deserialize)]
#[serde(try_from = "RawToolMap")]
pub(crate) struct ParsedToolMap {
    pub request: String,
    pub options: IndexMap<String, toml::Value>,
}

#[derive(Deserialize)]
struct RawToolMap {
    version: Option<toml::Value>,
    path: Option<toml::Value>,
    prefix: Option<toml::Value>,
    #[serde(rename = "ref")]
    ref_: Option<toml::Value>,
    #[serde(flatten)]
    options: IndexMap<String, toml::Value>,
}

fn parse_tool_selector(
    key: &'static str,
    value: Option<toml::Value>,
) -> std::result::Result<Option<(&'static str, String)>, String> {
    match value {
        Some(toml::Value::String(value)) => Ok(Some((key, value))),
        Some(_) => Err(format!("tool selector `{key}` must be a string")),
        None => Ok(None),
    }
}

impl TryFrom<RawToolMap> for ParsedToolMap {
    type Error = String;

    fn try_from(raw: RawToolMap) -> std::result::Result<Self, Self::Error> {
        let selectors = [
            parse_tool_selector("version", raw.version)?,
            parse_tool_selector("path", raw.path)?,
            parse_tool_selector("prefix", raw.prefix)?,
            parse_tool_selector("ref", raw.ref_)?,
        ];
        let mut selectors = selectors.into_iter().flatten();
        let Some((key, value)) = selectors.next() else {
            return Err(
                "tool definition must include exactly one of `version`, `path`, `prefix`, or `ref`"
                    .to_string(),
            );
        };
        if let Some((other, _)) = selectors.next() {
            return Err(format!(
                "tool definition cannot specify both `{key}` and `{other}`"
            ));
        }
        let request = if key == "version" {
            value
        } else {
            format!("{key}:{value}")
        };
        Ok(Self {
            request,
            options: raw.options,
        })
    }
}

fn insert_core_options(table: &mut InlineTable, options: ToolVersionOptions) {
    let core = options.core;
    if let Some(os) = core.os
        && !os.is_empty()
    {
        let mut arr = Array::new();
        for o in os {
            arr.push(Value::from(o));
        }
        table.insert("os", Value::Array(arr));
    }
    if let Some(depends) = core.depends
        && !depends.is_empty()
    {
        let mut arr = Array::new();
        for dep in depends {
            arr.push(Value::from(dep));
        }
        table.insert("depends", Value::Array(arr));
    }
    if !core.install_env.is_empty() {
        let mut env = InlineTable::new();
        for (k, v) in core.install_env {
            env.insert(k, v.into());
        }
        table.insert("install_env", env.into());
    }
}

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
    #[serde(default, deserialize_with = "deserialize_arr")]
    env_file: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_arr")]
    dotenv: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_root_env")]
    env: EnvList,
    #[serde(default, deserialize_with = "deserialize_arr")]
    env_path: Vec<String>,
    #[serde(default)]
    alias: AliasMap,
    #[serde(default)]
    tool_alias: AliasMap,
    #[serde(default)]
    shell_alias: IndexMap<String, String>,
    #[serde(skip)]
    doc: Mutex<OnceCell<DocumentMut>>,
    #[serde(default)]
    hooks: IndexMap<Hooks, HookDef>,
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
    task_templates: TaskTemplates,
    #[serde(default)]
    watch_files: Vec<WatchFile>,
    #[serde(default)]
    deps: Option<DepsConfig>,
    #[serde(default)]
    oci: Option<OciConfig>,
    #[serde(default)]
    bootstrap: Option<BootstrapTomlConfig>,
    #[serde(default)]
    dotfiles: Option<DotfilesTomlConfig>,
    #[serde(default, deserialize_with = "deserialize_vars")]
    vars: EnvList,
    #[serde(default)]
    settings: SettingsPartial,
    /// Marks this config as a monorepo root, enabling target path syntax for tasks
    #[serde(default)]
    monorepo_root: Option<bool>,
    /// Legacy name for monorepo_root, retained during its deprecation period
    #[serde(default)]
    experimental_monorepo_root: Option<bool>,
    /// Configuration for monorepo and workspace discovery
    #[serde(default)]
    monorepo: Option<MonorepoConfig>,
}

#[derive(Debug, Default, Clone)]
pub struct MiseTomlToolList(Vec<MiseTomlTool>);

#[derive(Debug, Clone)]
pub struct MiseTomlTool {
    /// The version request exactly as written, still un-rendered.
    ///
    /// Deliberately not parsed into a [`ToolVersionType`] here: a `:` inside a template belongs to
    /// the template, not to a version selector, and templates are not rendered until
    /// [`MiseToml::to_tool_request_set`]. Parsing at deserialize time made
    /// `{{ exec(command='echo VER: 1.2.3') | split(pat=': ') | last }}` fail as
    /// `invalid prefix: {{ exec(command='echo VER`. `ToolRequest::new` parses the rendered string,
    /// so selectors like `prefix:` and `ref:` are still honoured — just later. This matches what
    /// `[tasks.*.tools]` and `.tool-versions` already do.
    ///
    /// See: <https://github.com/jdx/mise/discussions/5531>
    pub request: String,
    pub options: Option<ToolVersionOptions>,
}

fn parse_mise_toml_tool_map<E>(parsed: ParsedToolMap) -> std::result::Result<MiseTomlTool, E>
where
    E: de::Error,
{
    let mut options = ToolVersionOptions::default();
    for (key, value) in parsed.options {
        insert_tool_option(&mut options, key, value)?;
    }
    Ok(MiseTomlTool {
        request: parsed.request,
        options: Some(options),
    })
}

#[derive(Debug, Default, Clone)]
pub struct Tasks(pub BTreeMap<String, Task>);

#[derive(Debug, Default, Clone)]
pub struct TaskTemplates(pub IndexMap<String, TaskTemplate>);

#[derive(Debug, Default, Clone)]
pub struct EnvList(pub(crate) Vec<EnvDirective>);

/// Configuration for the [monorepo] section in mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct MonorepoConfig {
    /// Explicit list of config roots for monorepo task discovery.
    /// Supports single-level glob patterns (*).
    #[serde(default)]
    pub config_roots: Vec<String>,
    /// Use a single lockfile at the monorepo root for descendant config roots.
    /// None follows the rollout default; true opts in, false keeps colocated locks.
    pub lockfile: Option<bool>,
    /// Explicit additions, removals, and overrides for provider-inferred projects.
    // Consumed when workspace providers are connected to task loading.
    #[allow(dead_code)]
    #[serde(default)]
    pub projects: BTreeMap<String, WorkspaceProjectOverride>,
    /// Experimental task defaults applied by task name across workspace projects.
    #[serde(default)]
    pub task_defaults: IndexMap<String, TaskTemplate>,
}

impl EnvList {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl MiseToml {
    fn enforce_min_version_fallback(body: &str) -> eyre::Result<()> {
        if let Ok(val) = toml::from_str::<toml::Value>(body)
            && let Some(min_val) = val.get("min_version")
        {
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
        Ok(())
    }
    pub fn init(path: &Path) -> Self {
        let mut context = BASE_CONTEXT.clone();
        context.insert(
            "config_root",
            config_root::config_root(path).to_str().unwrap(),
        );
        let mut rf = Self {
            path: path.to_path_buf(),
            context,
            ..Default::default()
        };
        rf.update_context_env(env::PRISTINE_ENV.clone());
        rf
    }

    pub fn from_file(path: &Path) -> eyre::Result<Self> {
        let body = file::read_to_string(path)?;
        Self::from_str(&body, path)
    }

    pub fn from_str(body: &str, path: &Path) -> eyre::Result<Self> {
        if !Self::is_trust_exempt(body, path) {
            trust_check(path)?;
        }
        trace!("parsing: {}", display_path(path));
        let des = toml::Deserializer::parse(body).map_err(|e| toml_parse_error(&e, body, path))?;
        let de_res = serde_ignored::deserialize(des, |p| {
            warn!("unknown field in {}: {p}", display_path(path));
        });
        let mut rf: MiseToml = match de_res {
            Ok(rf) => rf,
            Err(err) => {
                Self::enforce_min_version_fallback(body)?;
                return Err(toml_parse_error(&err, body, path));
            }
        };
        if let Some(legacy_monorepo_root) = rf.experimental_monorepo_root.take() {
            deprecated_at!(
                "2026.7.7",
                "2027.12.0",
                "config.experimental_monorepo_root",
                "`experimental_monorepo_root` in {} is deprecated. Use `monorepo_root` instead.",
                display_path(path)
            );
            rf.monorepo_root.get_or_insert(legacy_monorepo_root);
        }
        rf.context = BASE_CONTEXT.clone();
        rf.context.insert(
            "config_root",
            config_root::config_root(path).to_str().unwrap(),
        );
        rf.update_context_env(env::PRISTINE_ENV.clone());
        rf.path = path.to_path_buf();
        let project_root = rf.project_root().map(|p| p.to_path_buf());
        for task in rf.tasks.0.values_mut() {
            task.config_source.clone_from(&rf.path);
            task.config_root = project_root.clone();
        }
        // trace!("{}", rf.dump()?);
        Ok(rf)
    }

    /// Whether the config file at `path` loads without trust (see
    /// [`Self::is_trust_exempt`]). Returns false for unreadable files and for
    /// non-mise.toml files (e.g. `.tool-versions`), which have their own flow.
    pub fn path_is_trust_exempt(path: &Path) -> bool {
        file::read_to_string(path).is_ok_and(|body| Self::is_trust_exempt(&body, path))
    }

    /// Whether this config body can be loaded without trusting the file.
    ///
    /// Safe configs cannot execute code or change mise's behavior beyond
    /// requesting tool versions and defining tasks, so there is nothing to
    /// gate behind a trust prompt. Anything else — env vars, hooks, settings,
    /// aliases, templates, tool options like `postinstall`/`install_env` —
    /// still requires trust.
    fn is_trust_exempt(body: &str, path: &Path) -> bool {
        if Settings::try_get().is_ok_and(|settings| settings.paranoid) {
            return false;
        }
        // configs the user chose to ignore should stay unloaded rather than
        // becoming loadable because their content happens to be safe
        if is_ignored(&config_trust_root(path)) || is_ignored(path) {
            return false;
        }
        is_safe_config_body(body)
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

    fn warn_deprecated_env_keys(&self) {
        if !self.env_file.is_empty() {
            deprecated_at!(
                LEGACY_ENV_KEYS_DEPRECATED_WARN_AT,
                LEGACY_ENV_KEYS_DEPRECATED_REMOVE_AT,
                "config.env_file",
                "`env_file` in {} is deprecated. Use `env._.file` instead.",
                display_path(&self.path)
            );
        }
        if !self.dotenv.is_empty() {
            deprecated_at!(
                LEGACY_ENV_KEYS_DEPRECATED_WARN_AT,
                LEGACY_ENV_KEYS_DEPRECATED_REMOVE_AT,
                "config.dotenv",
                "`dotenv` in {} is deprecated. Use `env._.file` instead.",
                display_path(&self.path)
            );
        }
        if !self.env_path.is_empty() {
            deprecated_at!(
                LEGACY_ENV_KEYS_DEPRECATED_WARN_AT,
                LEGACY_ENV_KEYS_DEPRECATED_REMOVE_AT,
                "config.env_path",
                "`env_path` in {} is deprecated. Use `env._.path` instead.",
                display_path(&self.path)
            );
        }
    }

    fn doc_mut(&self) -> eyre::Result<MutexGuard<'_, OnceCell<DocumentMut>>> {
        self.doc()?;
        Ok(self.doc.lock().unwrap())
    }

    pub fn set_backend_alias(&mut self, fa: &BackendArg, to: &str) -> eyre::Result<()> {
        self.doc_mut()?
            .get_mut()
            .unwrap()
            .entry("tool_alias")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .insert(&fa.short, value(to));
        Ok(())
    }

    pub fn set_alias(&mut self, fa: &BackendArg, from: &str, to: &str) -> eyre::Result<()> {
        self.tool_alias
            .entry(fa.short.to_string())
            .or_default()
            .versions
            .insert(from.into(), to.into());
        let mut doc = self.doc_mut()?;
        let versions = doc
            .get_mut()
            .unwrap()
            .entry("tool_alias")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .entry(&fa.to_string())
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .entry("versions")
            .or_insert_with(table);
        insert_preserving_decor(versions, from, value(to));
        Ok(())
    }

    pub fn remove_backend_alias(&mut self, fa: &BackendArg) -> eyre::Result<()> {
        let mut doc = self.doc_mut()?;
        let doc = doc.get_mut().unwrap();
        // Remove from both tool_alias and deprecated alias sections
        for section in ["tool_alias", "alias"] {
            if let Some(aliases) = doc.get_mut(section).and_then(|v| v.as_table_mut()) {
                aliases.remove(&fa.short);
                if aliases.is_empty() {
                    doc.as_table_mut().remove(section);
                }
            }
        }
        Ok(())
    }

    pub fn remove_alias(&mut self, fa: &BackendArg, from: &str) -> eyre::Result<()> {
        // Remove from both tool_alias and deprecated alias in memory
        for alias_map in [&mut self.tool_alias, &mut self.alias] {
            if let Some(aliases) = alias_map.get_mut(&fa.short) {
                aliases.versions.shift_remove(from);
                if aliases.versions.is_empty() && aliases.backend.is_none() {
                    alias_map.shift_remove(&fa.short);
                }
            }
        }
        let mut doc = self.doc_mut()?;
        let doc = doc.get_mut().unwrap();
        // Remove from both tool_alias and deprecated alias sections in doc
        for section in ["tool_alias", "alias"] {
            if let Some(aliases) = doc.get_mut(section).and_then(|v| v.as_table_mut()) {
                if let Some(alias) = aliases
                    .get_mut(&fa.to_string())
                    .and_then(|v| v.as_table_mut())
                {
                    if let Some(versions) = alias.get_mut("versions").and_then(|v| v.as_table_mut())
                    {
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
                    doc.as_table_mut().remove(section);
                }
            }
        }
        Ok(())
    }

    pub fn set_shell_alias(&mut self, name: &str, command: &str) -> eyre::Result<()> {
        self.shell_alias.insert(name.into(), command.into());
        let mut doc = self.doc_mut()?;
        let shell_alias = doc
            .get_mut()
            .unwrap()
            .entry("shell_alias")
            .or_insert_with(table);
        insert_preserving_decor(shell_alias, name, value(command));
        Ok(())
    }

    pub fn remove_shell_alias(&mut self, name: &str) -> eyre::Result<()> {
        self.shell_alias.shift_remove(name);
        let mut doc = self.doc_mut()?;
        let doc = doc.get_mut().unwrap();
        if let Some(shell_alias) = doc.get_mut("shell_alias").and_then(|v| v.as_table_mut()) {
            shell_alias.remove(name);
            if shell_alias.is_empty() {
                doc.as_table_mut().remove("shell_alias");
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
                let value_decor = get_value_decor(env_tbl, k);
                let k = get_key_with_decor(env_tbl, k);
                let mut item = toml_edit::value(value);
                set_value_decor(&mut item, &value_decor);
                env_tbl.insert_formatted(&k, item);
                break;
            } else if !env_tbl.contains_key(k) {
                env_tbl.insert_formatted(&Key::from(*k), toml_edit::table());
            }
            env_tbl = env_tbl.get_mut(k).unwrap().as_table_mut().unwrap();
        }
        Ok(())
    }

    /// Set the version of `[bootstrap.packages]."<manager>:<package>"`,
    /// creating the tables as needed ("latest" means no pin). An existing
    /// table entry — in any of its TOML spellings (inline table, sub-table,
    /// or dotted keys) — keeps its `os` and any other keys, with only its
    /// `version` updated in place; everything else is written as a plain
    /// string.
    pub fn update_bootstrap_package(&mut self, spec: &str, version: &str) -> eyre::Result<()> {
        let updated_table = {
            let mut doc = self.doc_mut()?;
            let bootstrap = doc
                .get_mut()
                .unwrap()
                .entry("bootstrap")
                .or_insert_with(table)
                .as_table_mut()
                .unwrap();
            // don't render an empty [bootstrap] header above [bootstrap.packages]
            bootstrap.set_implicit(true);
            let packages = bootstrap
                .entry("packages")
                .or_insert_with(table)
                .as_table_mut()
                .unwrap();
            // `as_table_like_mut` covers every table spelling — inline table,
            // sub-table, and dotted keys all reach here, so none of them lose
            // their `os` to a plain-string rewrite
            if let Some(entry) = packages
                .get_mut(spec)
                .and_then(|item| item.as_table_like_mut())
            {
                let value_decor = entry
                    .get("version")
                    .and_then(|item| item.as_value())
                    .map(|value| value.decor().clone());
                let mut item = toml_edit::value(version);
                set_value_decor(&mut item, &value_decor);
                entry.insert("version", item);
                true
            } else {
                let key = get_key_with_decor(packages, spec);
                let value_decor = get_value_decor(packages, spec);
                let mut item = toml_edit::value(version);
                set_value_decor(&mut item, &value_decor);
                packages.insert_formatted(&key, item);
                false
            }
        };
        let packages = &mut self.bootstrap.get_or_insert_with(Default::default).packages;
        match packages.get_mut(spec) {
            Some(crate::system::PackageEntryToml::Table(entry)) if updated_table => {
                entry.insert(
                    "version".to_string(),
                    toml::Value::String(version.to_string()),
                );
            }
            _ => {
                packages.insert(
                    spec.to_string(),
                    crate::system::PackageEntryToml::Version(version.to_string()),
                );
            }
        }
        Ok(())
    }

    /// Set `[bootstrap.brew.taps]."<owner>/<tap>" = "<url>"`, creating the
    /// tables as needed. Only used by the `#[cfg(unix)]` brew CLI commands.
    #[cfg(unix)]
    pub fn update_bootstrap_brew_tap(&mut self, tap: &str, url: &str) -> eyre::Result<()> {
        self.bootstrap
            .get_or_insert_with(Default::default)
            .brew
            .taps
            .insert(tap.to_string(), url.to_string());
        let mut doc = self.doc_mut()?;
        let bootstrap = doc
            .get_mut()
            .unwrap()
            .entry("bootstrap")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();
        bootstrap.set_implicit(true);
        let brew = bootstrap
            .entry("brew")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();
        brew.set_implicit(true);
        let taps = brew
            .entry("taps")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();
        let key = get_key_with_decor(taps, tap);
        let value_decor = get_value_decor(taps, tap);
        let mut item = toml_edit::value(url);
        set_value_decor(&mut item, &value_decor);
        taps.insert_formatted(&key, item);
        Ok(())
    }

    #[cfg(unix)]
    pub fn remove_bootstrap_brew_tap(&mut self, tap: &str) -> eyre::Result<()> {
        if let Some(bootstrap) = &mut self.bootstrap {
            bootstrap.brew.taps.shift_remove(tap);
        }
        let mut doc = self.doc_mut()?;
        let doc = doc.get_mut().unwrap();
        if let Some(bootstrap) = doc.get_mut("bootstrap").and_then(|v| v.as_table_mut())
            && let Some(brew) = bootstrap.get_mut("brew").and_then(|v| v.as_table_mut())
            && let Some(taps) = brew.get_mut("taps").and_then(|v| v.as_table_mut())
        {
            taps.remove(tap);
            if taps.is_empty() {
                brew.remove("taps");
                if brew.is_empty() {
                    bootstrap.remove("brew");
                    if bootstrap.is_empty() {
                        doc.remove("bootstrap");
                    }
                }
            }
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
                let value_decor = get_value_decor(env_tbl, k);
                let k = get_key_with_decor(env_tbl, k);
                let mut item = toml_edit::Item::Value(Value::InlineTable(outer_table));
                set_value_decor(&mut item, &value_decor);
                env_tbl.insert_formatted(&k, item);
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

    // Merge base OS env vars with env sections from this file,
    // so they are available for templating.
    // Note this only merges regular key-value variables; referenced files are not resolved.
    fn update_context_env(&mut self, mut base_env: EnvMap) {
        for e in &self.env.0 {
            match e {
                EnvDirective::Val(key, value, _) => {
                    base_env.insert(key.clone(), value.clone());
                }
                EnvDirective::Default(key, value, _)
                    if base_env.get(key).is_none_or(|v| v.is_empty()) =>
                {
                    base_env.insert(key.clone(), value.clone());
                }
                _ => {}
            }
        }
        self.context.insert("env", &base_env);
    }

    fn parse_template(&self, input: &str) -> eyre::Result<String> {
        self.parse_template_with_context(&self.template_context(), input)
    }

    /// Context for a version request that only became invalid after its template was rendered.
    ///
    /// Deserialization no longer validates the version, so this is the first place such a failure
    /// can surface — and it is far from the file that caused it. Name the file, and show the
    /// template alongside what it produced, since neither alone explains the error.
    fn tool_request_error_context(
        &self,
        short: &str,
        tool: &MiseTomlTool,
        rendered: &str,
    ) -> String {
        let where_ = format!("{short} in {}", display_path(&self.path));
        if tool.request == rendered {
            format!("invalid version for {where_}")
        } else {
            format!(
                "invalid version for {where_}: {} rendered to {rendered:?}",
                tool.request
            )
        }
    }

    fn parse_template_with_context(
        &self,
        context: &TeraContext,
        input: &str,
    ) -> eyre::Result<String> {
        if !contains_template_syntax(input) {
            return Ok(input.to_string());
        }
        let dir = self.path.parent();
        let mut tera = get_tera(dir);
        let output = render_str(&mut tera, input, context).wrap_err_with(|| {
            let p = display_path(&self.path);
            eyre!("failed to parse template {input} in {p}")
        })?;
        Ok(output)
    }

    fn template_context(&self) -> TeraContext {
        let mut context = self.context.clone();
        Self::insert_resolved_vars(&mut context);
        context
    }

    fn insert_resolved_vars(context: &mut TeraContext) {
        if context.get("vars").is_some() {
            return;
        }
        let Some(config) = Config::maybe_get() else {
            return;
        };
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

    /// Render a tool-option template at config-load time, resolving env/vars but
    /// deferring `os()`/`arch()` (re-emitted as `{{ os() }}`/`{{ arch() }}`) so
    /// backends can render them for the host at install time or for an arbitrary
    /// target during cross-platform `mise lock`.
    fn parse_tool_option_template(
        &self,
        context: &TeraContext,
        input: &str,
    ) -> eyre::Result<String> {
        if !contains_template_syntax(input) {
            return Ok(input.to_string());
        }
        let dir = self.path.parent();
        let mut tera = crate::tera::get_tera_preserving_os_arch(dir);
        let output = render_str(&mut tera, input, context).wrap_err_with(|| {
            let p = display_path(&self.path);
            eyre!("failed to parse template {input} in {p}")
        })?;
        Ok(output)
    }

    fn parse_tool_option_value_template(
        &self,
        context: &TeraContext,
        key: Option<&str>,
        value: &mut toml::Value,
        defer_os_arch: bool,
    ) -> eyre::Result<()> {
        match value {
            toml::Value::String(s) => {
                let preserve_os_arch = defer_os_arch && matches!(key, Some("url" | "checksum_url"));
                *s = if preserve_os_arch {
                    self.parse_tool_option_template(context, s)?
                } else {
                    self.parse_template_with_context(context, s)?
                };
            }
            toml::Value::Array(values) => {
                for value in values {
                    self.parse_tool_option_value_template(context, key, value, defer_os_arch)?;
                }
            }
            toml::Value::Table(table) => {
                for (key, value) in table.iter_mut() {
                    self.parse_tool_option_value_template(
                        context,
                        Some(key),
                        value,
                        defer_os_arch,
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl ConfigFile for MiseToml {
    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn min_version(&self) -> Option<&MinVersionSpec> {
        self.min_version.as_ref()
    }

    fn settings(&self) -> Option<&SettingsPartial> {
        Some(&self.settings)
    }

    fn plugins(&self) -> eyre::Result<HashMap<String, String>> {
        self.plugins
            .clone()
            .into_iter()
            .map(|(k, v)| {
                let v = self.parse_template(&v)?;
                Ok((k, resolve_plugin_source_path(&self.path, v)?))
            })
            .collect()
    }

    fn env_entries(&self) -> eyre::Result<Vec<EnvDirective>> {
        self.warn_deprecated_env_keys();
        let env_entries = self.env.0.iter().cloned();
        let path_entries = self
            .env_path
            .iter()
            .map(|p| EnvDirective::Path(p.clone(), Default::default()))
            .collect_vec();
        let env_files = self
            .env_file
            .iter()
            .chain(&self.dotenv)
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

    fn task_templates(&self) -> IndexMap<String, TaskTemplate> {
        self.task_templates.0.clone()
    }

    fn remove_tool(&self, fa: &BackendArg) -> eyre::Result<()> {
        let mut tools = self.tools.lock().unwrap();
        tools.shift_remove(fa);
        let mut doc = self.doc_mut()?;
        let doc = doc.get_mut().unwrap();
        if let Some(tools) = doc.get_mut("tools")
            && let Some(tools) = tools.as_table_like_mut()
        {
            // the tool may be written as an alias ("nodejs"), a qualified name ("core:node") or the
            // fully-qualified backend rather than as fa.short; removing only the short name leaves
            // the entry in the document and save() writes it straight back out
            let keys = tool_keys_for(&*tools, fa);
            for key in &keys {
                tools.remove(key);
            }
            if tools.is_empty() {
                doc.as_table_mut().remove("tools");
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
            if opts.os.as_ref().is_some_and(|o| !o.is_empty())
                || opts.depends.as_ref().is_some_and(|d| !d.is_empty())
                || !opts.install_env.is_empty()
            {
                return false;
            }
            if let Some(reg_ba) = REGISTRY.get(ba.short.as_str()).and_then(|b| b.ba())
                && reg_ba.opts.as_ref().is_some_and(|o| o == opts)
            {
                // in this case the options specified are the same as in the registry so output no options and rely on the defaults
                return true;
            }
            opts.is_empty()
        };
        existing.0 = versions
            .iter()
            .map(|tr| MiseTomlTool::from(tr.clone()))
            .collect();
        if is_tools_sorted {
            // Keep the parsed representation in sync with the document ordering. Sorting only the
            // document leaves this map in insertion order, so a later replacement in the same
            // `mise use` command can mistake an originally sorted table for an unsorted one.
            tools.sort_keys();
        }
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

        // the entry may be written under any spelling of this tool — an alias like "nodejs", a
        // qualified "core:node", or the fully-qualified backend — so collect every key in the
        // document that refers to it. The first one is the entry the file appears to define, so it
        // supplies the decorations; the rest are duplicates of it and are dropped below.
        let keys = tool_keys_for(&*tools, ba);
        let existing = keys.first().cloned().unwrap_or_else(|| ba.short.clone());
        if keys.len() > 1 {
            let dupes = keys.iter().map(|k| format!("`{k}`")).join(", ");
            warn!(
                "{}: {dupes} are the same tool; collapsing them into a single `{}` entry",
                display_path(&self.path),
                ba.short
            );
        }
        // create a key from the short name preserving any decorations like prefix/suffix if the key already exists
        let key = get_key_with_decor_from(tools, ba.short.as_str(), &existing);
        // and the same for the value, so a comment after the version survives the replacement
        let value_decor = get_value_decor(tools, &existing);
        // keep the array as it was written so its layout — multi-line shape, trailing comma and any
        // comments between the elements — can be reused when only the versions change. Read before
        // the removal below, which drops the entry when it is stored under a long name.
        let existing_arr = tools
            .get(&existing)
            .and_then(|i| i.as_value())
            .and_then(|v| v.as_array())
            .cloned();

        // drop the other spellings: they all deserialize to this one entry, so leaving one behind
        // means the file has two keys for one tool and the later one silently wins on read-back.
        // ba.short itself is kept so insert_formatted overwrites it in place instead of appending.
        for k in &keys {
            if k != &ba.short {
                tools.remove(k);
            }
        }

        if versions.len() == 1 {
            let options = versions[0].options();
            let mut item = if output_empty_opts(&options) {
                value(versions[0].version())
            } else {
                let mut table = InlineTable::new();
                table.insert("version", versions[0].version().into());
                for (k, v) in &options.opts {
                    table.insert(k, toml_value_to_edit(v.clone()));
                }
                insert_core_options(&mut table, options);
                Item::Value(Value::InlineTable(table))
            };
            set_value_decor(&mut item, &value_decor);
            tools.insert_formatted(&key, item);
        } else {
            // Reuse the existing array when the version count is unchanged: swapping the values in
            // place keeps the layout, including comments written between the elements. A comment
            // after an element belongs to the decor of the element that follows it, so an array
            // built from scratch always drops it. When the count changes there is no way to line
            // the old decor up with the new elements, so build a fresh array as before.
            let reused = existing_arr.filter(|a| a.len() == versions.len());
            let reusing = reused.is_some();
            let mut arr = reused.unwrap_or_else(Array::new);
            for (i, tr) in versions.into_iter().enumerate() {
                let v = tr.version();
                let val: Value = if output_empty_opts(&tr.options()) {
                    v.to_string().into()
                } else {
                    let mut table = InlineTable::new();
                    table.insert("version", v.to_string().into());
                    let options = tr.options();
                    for (k, v) in &options.opts {
                        table.insert(k, toml_value_to_edit(v.clone()));
                    }
                    insert_core_options(&mut table, options);
                    table.into()
                };
                match reusing.then(|| arr.get_mut(i)).flatten() {
                    Some(slot) => {
                        let mut val = val;
                        *val.decor_mut() = slot.decor().clone();
                        *slot = val;
                    }
                    // `push` applies the default separators, exactly as before
                    None => arr.push(val),
                }
            }
            let mut item = Item::Value(Value::Array(arr));
            set_value_decor(&mut item, &value_decor);
            tools.insert_formatted(&key, item);
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
        if let Some(config) = Config::maybe_get()
            && let Some(env_results) = config.env_results_cached()
        {
            let mut env_vars: EnvMap =
                if let Some(existing_env) = context.get("env").and_then(|v| v.as_map()) {
                    existing_env
                        .iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.to_string(), s.to_string())))
                        .collect()
                } else {
                    env::PRISTINE_ENV.clone()
                };
            for key in &env_results.env_remove {
                env_vars.remove(key);
            }
            env_vars.extend(
                env_results
                    .env
                    .iter()
                    .map(|(k, (v, _))| (k.clone(), v.clone())),
            );
            context.insert("env", &env_vars);
        }
        Self::insert_resolved_vars(&mut context);
        for (ba, tvp) in tools.iter() {
            for tool in &tvp.0 {
                let version = self.parse_template_with_context(&context, &tool.request)?;
                // taken before `ba` is consumed below
                let short = ba.short.clone();
                let tvr = if let Some(mut options) = tool.options.clone() {
                    // Add placeholder for version since it's not available at config load time
                    // This preserves {{ version }} in the output for install-time rendering
                    let mut opts_context = context.clone();
                    opts_context.insert("version", "{{ version }}");
                    // The http and s3 backends re-render their url/checksum_url per
                    // target platform (host at install, any target during `mise
                    // lock`), so only those two options defer os()/arch() instead of
                    // resolving them now. Every other option (here and for other
                    // backends) is consumed verbatim, so it keeps host resolution at
                    // config load — deferring it would leak raw `{{ os() }}`
                    // fragments into consumers that never render again (e.g.
                    // checksum_expr).
                    let defer_os_arch = matches!(
                        ba.backend_type(),
                        crate::backend::backend_type::BackendType::Http
                            | crate::backend::backend_type::BackendType::S3
                    );
                    for (k, v) in options.opts.iter_mut() {
                        self.parse_tool_option_value_template(
                            &opts_context,
                            Some(k),
                            v,
                            defer_os_arch,
                        )?;
                    }
                    let mut ba = ba.clone();
                    // Start with cached options but filter out install-time-only options
                    // when config provides its own options. This allows:
                    // - Changing url/asset_pattern/checksum without reinstall issues
                    // - Replacing stale layout options like bin_path with current config values
                    let mut ba_opts = ba.opts().clone();
                    let backend_type = ba.backend_type();
                    ba_opts.opts.retain(|k, _| {
                        !crate::backend::is_install_time_option_key_for_type(&backend_type, k)
                    });
                    ba_opts.apply_overrides(&options);
                    // Re-apply registry defaults for install-time keys not overridden by user.
                    // The filtering above strips both stale install-state cache AND registry
                    // defaults. We want to keep registry defaults while discarding stale cache.
                    if let Some(rt) = crate::registry::REGISTRY.get(ba.short.as_str()) {
                        let full = ba.full();
                        // Get structured options from registry (table-format backends)
                        let mut registry_opts = rt.backend_options(&full);
                        // Also parse inline options from [key=val,...] in the full string
                        if let Some(start) = full.rfind('[')
                            && full.ends_with(']')
                        {
                            let inline = crate::toolset::parse_tool_options(
                                &full[start + 1..full.len() - 1],
                            );
                            for (k, v) in inline.opts {
                                registry_opts.opts.entry(k).or_insert(v);
                            }
                        }
                        for (k, v) in registry_opts.opts {
                            ba_opts.opts.entry(k).or_insert(v);
                        }
                    }
                    // Replace config-owned fields rather than merging them with cached values.
                    // This intentionally supersedes apply_overrides above so omitted values clear
                    // stale cache and install_env does not retain cached entries.
                    ba_opts.os = options.os.clone();
                    ba_opts.depends = options.depends.clone();
                    ba_opts.install_env = options.install_env.clone();
                    ba.set_opts(Some(ba_opts.clone()));
                    ToolRequest::new_opts(ba.into(), &version, ba_opts, source.clone())
                        .wrap_err_with(|| self.tool_request_error_context(&short, tool, &version))?
                } else {
                    ToolRequest::new(ba.clone().into(), &version, source.clone())
                        .wrap_err_with(|| self.tool_request_error_context(&short, tool, &version))?
                };
                trs.add_version(tvr, &source);
            }
        }
        Ok(trs)
    }

    fn aliases(&self) -> eyre::Result<AliasMap> {
        // Emit deprecation warning if [alias] is used
        if !self.alias.is_empty() {
            deprecated!(
                "alias",
                "[alias] is deprecated, use [tool_alias] instead in {}",
                display_path(&self.path)
            );
        }

        // Merge alias and tool_alias, with tool_alias taking precedence
        let mut combined: AliasMap = self.alias.clone();
        for (k, v) in &self.tool_alias {
            combined.insert(k.clone(), v.clone());
        }

        combined
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

    fn shell_aliases(&self) -> eyre::Result<IndexMap<String, String>> {
        self.shell_alias
            .iter()
            .map(|(k, v)| {
                let v = self.parse_template(v)?;
                Ok((k.clone(), v))
            })
            .collect()
    }

    fn task_config(&self) -> &TaskConfig {
        &self.task_config
    }

    fn task_config_includes(&self) -> eyre::Result<Option<Vec<String>>> {
        self.task_config
            .includes
            .as_ref()
            .map(|includes| {
                includes
                    .iter()
                    .map(|include| self.parse_template(include))
                    .collect()
            })
            .transpose()
    }

    fn monorepo_root(&self) -> Option<bool> {
        self.monorepo_root
    }

    fn monorepo(&self) -> Option<&MonorepoConfig> {
        self.monorepo.as_ref()
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
                    run: wf
                        .run
                        .as_ref()
                        .map(|r| self.parse_template(r))
                        .transpose()?,
                    shell: wf
                        .shell
                        .as_ref()
                        .map(|s| self.parse_template(s))
                        .transpose()?,
                    task: wf
                        .task
                        .as_ref()
                        .map(|t| self.parse_template(t))
                        .transpose()?,
                })
            })
            .collect()
    }

    fn hooks(&self) -> eyre::Result<Vec<Hook>> {
        Ok(self
            .hooks
            .iter()
            .map(|(hook_type, def)| {
                let mut hooks = def.clone().into_hooks(*hook_type);
                for hook in hooks.iter_mut() {
                    hook.render_templates(|s| self.parse_template(s))?;
                }
                eyre::Ok(hooks)
            })
            .collect::<eyre::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    fn deps_config(&self) -> Option<DepsConfig> {
        self.deps.clone()
    }

    fn oci_config(&self) -> Option<OciConfig> {
        self.oci.clone()
    }

    fn bootstrap_config(&self) -> Option<BootstrapTomlConfig> {
        self.bootstrap.clone()
    }

    fn dotfiles_config(&self) -> Option<DotfilesTomlConfig> {
        self.dotfiles.clone()
    }
}

fn resolve_plugin_source_path(config_path: &Path, source: String) -> eyre::Result<String> {
    let source_path = Path::new(&source);
    let is_explicit_relative = matches!(
        source_path.components().next(),
        Some(Component::CurDir | Component::ParentDir)
    );
    let expanded = file::replace_path(source_path);
    let path = if expanded.is_absolute() {
        Some(expanded)
    } else if is_explicit_relative {
        Some(config_root::config_root(config_path).join(expanded))
    } else {
        None
    };

    match path {
        Some(path) => Ok(path.absolutize()?.to_string_lossy().into_owned()),
        None => Ok(source),
    }
}

/// Returns a [`toml_edit::Key`] from the given `key`.
/// Preserves any surrounding whitespace (e.g. comments) if the key already exists in the provided [`toml_edit::Table`].
fn get_key_with_decor(table: &toml_edit::Table, key: &str) -> Key {
    get_key_with_decor_from(table, key, key)
}

/// Same as [`get_key_with_decor`], but takes the decor from `existing` rather than from `key`.
/// The entry being replaced may be stored under a different key than the one written back, e.g. a
/// fully-qualified `"core:node"` that `mise use node` rewrites to `node`.
fn get_key_with_decor_from(table: &toml_edit::Table, key: &str, existing: &str) -> Key {
    let mut key = Key::from(key);
    if let Some((k, _)) = table.get_key_value(existing) {
        if let Some(prefix) = k.leaf_decor().prefix() {
            key.leaf_decor_mut().set_prefix(prefix.clone());
        }
        if let Some(suffix) = k.leaf_decor().suffix() {
            key.leaf_decor_mut().set_suffix(suffix.clone());
        }
    }
    key
}

/// Every key in a `[tools]` table that refers to `ba`, in document order.
///
/// The same tool can be written under several spellings: its short name (`node`), one of the
/// hardcoded aliases (`nodejs`, `golang`, `dotnet-core`), a `core:`-qualified name (`core:node`),
/// or the fully-qualified backend the short name resolves to. [`unalias_backend`] folds the first
/// three onto the short name, and that is what happens when the file is deserialized — so mise's
/// own view of `[tools]` holds a single entry no matter how many of those keys the document has,
/// and when there is more than one the later one silently wins. A writer therefore has to find all
/// of them and not just the one it would write itself, or it leaves a second key behind that
/// outvotes the one it just wrote.
///
/// Registry aliases (`rg` for `ripgrep`) are a different mechanism: they resolve to *different*
/// short names, mise keeps them as separate entries and writes each back as the user spelled it,
/// so they are deliberately not matched here. Keys carrying inline options
/// (`"go:example.com/x[tags=y]"`) are likewise left alone — collapsing those would have to decide
/// what happens to the options.
///
/// The keys are returned owned so the caller can go on to mutate the table.
fn tool_keys_for(tools: &dyn toml_edit::TableLike, ba: &BackendArg) -> Vec<String> {
    let full = ba.full();
    tools
        .iter()
        .filter(|&(k, _)| unalias_backend(k) == ba.short.as_str() || k == full.as_str())
        .map(|(k, _)| k.to_string())
        .collect()
}

/// Captures the decor of the value `key` currently holds, if any.
///
/// A comment written after the value on the same line lives in that decor, so replacing the value
/// without carrying it over drops the comment. The comment *above* the line belongs to the key
/// instead and is handled by [`get_key_with_decor`].
fn get_value_decor(table: &toml_edit::Table, key: &str) -> Option<toml_edit::Decor> {
    let value = table.get(key)?.as_value()?;
    Some(value.decor().clone())
}

/// Inserts `item` under `key` in `target`, carrying over the decor of the entry being replaced:
/// the comment above the line lives on the key, the one after the value on the value.
///
/// Inline tables have no `insert_formatted`, so they fall back to a plain insert — which is what
/// every caller here did unconditionally before, so nothing regresses for them.
fn insert_preserving_decor(target: &mut Item, key: &str, mut item: Item) {
    if let Some(tbl) = target.as_table_mut() {
        let k = get_key_with_decor(tbl, key);
        let value_decor = get_value_decor(tbl, key);
        set_value_decor(&mut item, &value_decor);
        tbl.insert_formatted(&k, item);
    } else if let Some(tbl) = target.as_table_like_mut() {
        tbl.insert(key, item);
    }
}

/// Applies decor captured by [`get_value_decor`] to the value that replaces it.
fn set_value_decor(item: &mut Item, decor: &Option<toml_edit::Decor>) {
    if let (Some(decor), Some(value)) = (decor, item.as_value_mut()) {
        if let Some(prefix) = decor.prefix() {
            value.decor_mut().set_prefix(prefix.clone());
        }
        if let Some(suffix) = decor.suffix() {
            value.decor_mut().set_suffix(suffix.clone());
        }
    }
}

impl Debug for MiseToml {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let tools = self.to_tool_request_set().unwrap().to_string();
        let title = format!("MiseToml({}): {tools}", display_path(&self.path));
        let mut d = f.debug_struct(&title);
        if let Some(min_version) = &self.min_version {
            d.field("min_version", min_version);
        }
        if !self.env_file.is_empty() {
            d.field("env_file", &self.env_file);
        }
        if !self.dotenv.is_empty() {
            d.field("dotenv", &self.dotenv);
        }
        if let Ok(env) = self.env_entries()
            && !env.is_empty()
        {
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
            custom: self.custom.clone(),
            min_version: self.min_version.clone(),
            context: self.context.clone(),
            path: self.path.clone(),
            env_file: self.env_file.clone(),
            dotenv: self.dotenv.clone(),
            env: self.env.clone(),
            env_path: self.env_path.clone(),
            alias: self.alias.clone(),
            tool_alias: self.tool_alias.clone(),
            shell_alias: self.shell_alias.clone(),
            doc: Mutex::new(self.doc.lock().unwrap().clone()),
            hooks: self.hooks.clone(),
            tools: Mutex::new(self.tools.lock().unwrap().clone()),
            redactions: self.redactions.clone(),
            plugins: self.plugins.clone(),
            tasks: self.tasks.clone(),
            task_templates: self.task_templates.clone(),
            task_config: self.task_config.clone(),
            settings: self.settings.clone(),
            watch_files: self.watch_files.clone(),
            deps: self.deps.clone(),
            oci: self.oci.clone(),
            bootstrap: self.bootstrap.clone(),
            dotfiles: self.dotfiles.clone(),
            vars: self.vars.clone(),
            monorepo_root: self.monorepo_root,
            experimental_monorepo_root: self.experimental_monorepo_root,
            monorepo: self.monorepo.clone(),
        }
    }
}

impl From<ToolRequest> for MiseTomlTool {
    fn from(tr: ToolRequest) -> Self {
        // `ToolRequest::version()` re-emits the selector prefix (`prefix:`, `ref:`, `path:`,
        // `sub-N:`, `system`), which is the same string this would have been written from, so the
        // per-variant reconstruction that used to live here is redundant.
        let options = tr.options();
        Self {
            request: tr.version(),
            options: if options.is_empty() {
                None
            } else {
                Some(options)
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
                formatter.write_str("environment variable table")
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut env = vec![];
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "_" | "mise" => {
                            if key == "mise" {
                                deprecated_at!(
                                    "2026.7.0",
                                    "2026.12.0",
                                    "config.env.mise",
                                    "`env.mise` is deprecated. Use `env._` instead."
                                );
                            }
                            #[derive(Deserialize)]
                            #[serde(untagged)]
                            enum MiseTomlEnvDirectiveValue {
                                Single {
                                    #[serde(alias = "value")]
                                    path: String,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                Multiple {
                                    #[serde(alias = "value", alias = "values", alias = "paths")]
                                    path: Vec<String>,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                            }

                            struct MiseTomlEnvDirective(MiseTomlEnvDirectiveValue);

                            impl<'de> Deserialize<'de> for MiseTomlEnvDirective {
                                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                                where
                                    D: Deserializer<'de>,
                                {
                                    let value = toml::Value::deserialize(deserializer)?;
                                    let (uses_value, uses_values) = value
                                        .as_table()
                                        .map(|table| {
                                            (
                                                table.contains_key("value"),
                                                table.contains_key("values"),
                                            )
                                        })
                                        .unwrap_or_default();
                                    let directive = MiseTomlEnvDirectiveValue::deserialize(value)
                                        .map_err(de::Error::custom)?;

                                    if uses_value {
                                        deprecated_at!(
                                            "2026.7.0",
                                            "2026.12.0",
                                            "config.directive.value",
                                            "`value` in built-in `file`, `path`, and `source` directive objects is deprecated. Use `path` instead."
                                        );
                                    }
                                    if uses_values {
                                        deprecated_at!(
                                            "2026.7.0",
                                            "2026.12.0",
                                            "config.directive.values",
                                            "`values` in built-in `file`, `path`, and `source` directive objects is deprecated. Use `path` instead."
                                        );
                                    }

                                    Ok(MiseTomlEnvDirective(directive))
                                }
                            }

                            impl FromStr for MiseTomlEnvDirective {
                                type Err = String;
                                fn from_str(s: &str) -> Result<Self, Self::Err> {
                                    Ok(MiseTomlEnvDirective(MiseTomlEnvDirectiveValue::Single {
                                        path: s.to_string(),
                                        options: Default::default(),
                                    }))
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

                            // Reuses `deserialize_arr` so each of `path`/`file`/`source`
                            // accepts either a single value or a list, while
                            // `ParsedEnvBlock` below iterates the `_` table in the order
                            // the keys were written.
                            #[derive(Deserialize)]
                            struct DirectiveArr(
                                #[serde(deserialize_with = "deserialize_arr")]
                                Vec<MiseTomlEnvDirective>,
                            );

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
                                directives.into_iter().flat_map(move |d| match d.0 {
                                    MiseTomlEnvDirectiveValue::Single { path, options } => {
                                        vec![constructor(path, options)]
                                    }
                                    MiseTomlEnvDirectiveValue::Multiple { path, options } => path
                                        .into_iter()
                                        .map(|v| constructor(v, options.clone()))
                                        .collect(),
                                })
                            }

                            // Parse the `_` table preserving the written order of its
                            // sub-keys (`path`/`file`/`source`/modules) so that a later
                            // directive's template can reference a variable exported by
                            // an earlier one — e.g. `_.path` using a var from `_.source`
                            // (discussion #3783). `python.venv` is applied last
                            // regardless of position: it is a tools-phase directive whose
                            // PATH conventionally comes after tool paths.
                            struct ParsedEnvBlock {
                                directives: Vec<EnvDirective>,
                                venv: Option<EnvDirectivePythonVenv>,
                            }

                            impl<'de> Deserialize<'de> for ParsedEnvBlock {
                                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                                where
                                    D: Deserializer<'de>,
                                {
                                    struct ParsedEnvBlockVisitor;
                                    impl<'de> Visitor<'de> for ParsedEnvBlockVisitor {
                                        type Value = ParsedEnvBlock;
                                        fn expecting(
                                            &self,
                                            formatter: &mut Formatter,
                                        ) -> std::fmt::Result
                                        {
                                            formatter.write_str("the env `_` directive table")
                                        }
                                        fn visit_map<M>(
                                            self,
                                            mut map: M,
                                        ) -> Result<Self::Value, M::Error>
                                        where
                                            M: de::MapAccess<'de>,
                                        {
                                            let mut directives = vec![];
                                            let mut venv = None;
                                            while let Some(key) = map.next_key::<String>()? {
                                                match key.as_str() {
                                                    "path" => {
                                                        directives.extend(flatten_directives(
                                                            map.next_value::<DirectiveArr>()?.0,
                                                            EnvDirective::Path,
                                                        ));
                                                    }
                                                    "file" => {
                                                        directives.extend(flatten_directives(
                                                            map.next_value::<DirectiveArr>()?.0,
                                                            EnvDirective::File,
                                                        ));
                                                    }
                                                    "source" => {
                                                        directives.extend(flatten_directives(
                                                            map.next_value::<DirectiveArr>()?.0,
                                                            EnvDirective::Source,
                                                        ));
                                                    }
                                                    "python" => {
                                                        venv = map
                                                            .next_value::<EnvDirectivePython>()?
                                                            .venv;
                                                    }
                                                    _ => {
                                                        let mut value =
                                                            map.next_value::<toml::Value>()?;
                                                        let mut opts =
                                                            EnvDirectiveOptions::default();
                                                        if let Some(table) = value.as_table_mut()
                                                            && let Some(tools) =
                                                                table.remove("tools")
                                                        {
                                                            opts.tools =
                                                                tools.as_bool().unwrap_or(false);
                                                        }
                                                        directives.push(EnvDirective::Module(
                                                            key, value, opts,
                                                        ));
                                                    }
                                                }
                                            }
                                            Ok(ParsedEnvBlock { directives, venv })
                                        }
                                    }
                                    deserializer.deserialize_map(ParsedEnvBlockVisitor)
                                }
                            }

                            let block = map.next_value::<ParsedEnvBlock>()?;
                            env.extend(block.directives);
                            if let Some(venv) = block.venv {
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
                                        expand: false,
                                    },
                                });
                            }
                        }
                        _ => {
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
                                    value: EnvValue,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                DefaultMap {
                                    default: EnvValue,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                OptionsOnly {
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                Primitive(EnvValue),
                            }

                            #[derive(Deserialize)]
                            struct AgeComplexVal {
                                value: String,
                                #[serde(default)]
                                format: Option<AgeFormat>,
                                #[serde(flatten)]
                                options: EnvDirectiveOptions,
                            }
                            let raw_value = map.next_value::<toml::Value>()?;
                            if let Some(table) = raw_value.as_table() {
                                let has_default = table.contains_key("default");
                                if has_default && table.contains_key("value") {
                                    return Err(serde::de::Error::custom(format!(
                                        "Environment variable '{}' cannot have both 'value' and 'default'. The 'value' field always overwrites, while 'default' only applies when the variable is unset or empty. Remove either the 'value' field or the 'default' field.",
                                        key
                                    )));
                                }
                                if has_default && table.contains_key("required") {
                                    return Err(serde::de::Error::custom(format!(
                                        "Environment variable '{}' cannot have both 'default' and 'required'. The 'required' flag means the variable must be defined elsewhere, while 'default' provides a fallback value. Remove either the 'default' field or the 'required' flag.",
                                        key
                                    )));
                                }
                                if has_default && table.contains_key("age") {
                                    return Err(serde::de::Error::custom(format!(
                                        "Environment variable '{}' cannot have both 'age' and 'default'. Remove either the 'age' field or the 'default' field.",
                                        key
                                    )));
                                }
                            }
                            let val_result = raw_value.try_into::<Val>().map_err(|e| {
                                serde::de::Error::custom(format!(
                                    "failed to parse environment variable '{}': {}",
                                    key, e
                                ))
                            })?;

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

                            let directive = match val_result {
                                Val::Primitive(value) => match value.into_string() {
                                    Some(s) => {
                                        EnvDirective::Val(key, s, EnvDirectiveOptions::default())
                                    }
                                    None => EnvDirective::Rm(key, EnvDirectiveOptions::default()),
                                },
                                Val::Map { value, options } => {
                                    // Validate that required cannot be used with any value
                                    if options.required.is_required() {
                                        return Err(serde::de::Error::custom(format!(
                                            "Environment variable '{}' cannot have both 'value' and 'required'. The 'required' flag means the variable must be defined elsewhere (in the environment or a later config file). Remove either the 'value' field or the 'required' flag.",
                                            key
                                        )));
                                    }
                                    match value.into_string() {
                                        Some(s) => EnvDirective::Val(key, s, options),
                                        None => EnvDirective::Rm(key, options),
                                    }
                                }
                                Val::DefaultMap { default, options } => {
                                    let Some(default) = default.into_default_string() else {
                                        return Err(serde::de::Error::custom(format!(
                                            "Environment variable '{}' default cannot be a boolean. Use a string or integer fallback instead.",
                                            key
                                        )));
                                    };
                                    EnvDirective::Default(key, default, options)
                                }
                                Val::OptionsOnly { options } => {
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
                                Val::AgeComplex { .. } | Val::AgeWithOptions { .. } => {
                                    unreachable!() // Already handled above
                                }
                            };
                            env.push(directive);
                        }
                    }
                }
                Ok(EnvList(env))
            }
        }

        deserializer.deserialize_map(EnvManVisitor)
    }
}

pub(crate) fn deserialize_vars<'de, D>(deserializer: D) -> std::result::Result<EnvList, D::Error>
where
    D: de::Deserializer<'de>,
{
    let value = toml::Value::deserialize(deserializer)?;
    fn contains_mise(value: &toml::Value) -> bool {
        match value {
            toml::Value::Table(table) => table.contains_key("mise"),
            toml::Value::Array(values) => values.iter().any(contains_mise),
            _ => false,
        }
    }
    if contains_mise(&value) {
        return Err(de::Error::custom("`vars.mise` is not supported"));
    }
    EnvList::deserialize(value).map_err(de::Error::custom)
}

fn deserialize_root_env<'de, D>(deserializer: D) -> std::result::Result<EnvList, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct RootEnvVisitor;

    impl<'de> Visitor<'de> for RootEnvVisitor {
        type Value = EnvList;

        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("env table or flat array of env tables")
        }

        fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            EnvList::deserialize(de::value::MapAccessDeserializer::new(map))
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
    }

    deserializer.deserialize_any(RootEnvVisitor)
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
                Ok(MiseTomlToolList(vec![MiseTomlTool {
                    request: v.to_string(),
                    options: None,
                }]))
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

            fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let parsed =
                    ParsedToolMap::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(MiseTomlToolList(vec![
                    parse_mise_toml_tool_map::<M::Error>(parsed)?,
                ]))
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
                Ok(MiseTomlTool {
                    request: v.to_string(),
                    options: None,
                })
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let parsed =
                    ParsedToolMap::deserialize(de::value::MapAccessDeserializer::new(map))?;
                parse_mise_toml_tool_map::<M::Error>(parsed)
            }
        }

        deserializer.deserialize_any(MiseTomlToolVisitor)
    }
}

struct TaskBoolPresenceMapAccess<'a, M> {
    inner: M,
    presence: &'a mut TaskTomlBoolPresence,
}

impl<'de, M> de::MapAccess<'de> for TaskBoolPresenceMapAccess<'_, M>
where
    M: de::MapAccess<'de>,
{
    type Error = M::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        let Some(key) = self.inner.next_key::<String>()? else {
            return Ok(None);
        };
        self.presence.record(&key);
        seed.deserialize(de::value::StringDeserializer::<M::Error>::new(key))
            .map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        self.inner.next_value_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
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
                                let mut presence = TaskTomlBoolPresence::default();
                                let map = TaskBoolPresenceMapAccess {
                                    inner: map,
                                    presence: &mut presence,
                                };
                                let mut task =
                                    Task::deserialize(de::value::MapAccessDeserializer::new(map))?;
                                task.toml_bool_presence = presence;
                                Ok(TaskDef(task))
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

impl<'de> de::Deserialize<'de> for TaskTemplates {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct TaskTemplatesVisitor;

        impl<'de> Visitor<'de> for TaskTemplatesVisitor {
            type Value = TaskTemplates;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("map of task template names to template definitions")
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut templates = IndexMap::new();
                while let Some(name) = map.next_key::<String>()? {
                    let template: TaskTemplate = map.next_value()?;
                    templates.insert(name, template);
                }
                Ok(TaskTemplates(templates))
            }
        }

        deserializer.deserialize_any(TaskTemplatesVisitor)
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

/// A config body is safe to load without trust when nothing in it can execute
/// code at load time or change mise's behavior without an explicit user
/// action:
/// - `min_version` is inert
/// - `[tools]` entries with plain version strings only matter when the user
///   runs something like `mise install`. Entries with options (tables) are
///   excluded because options like `postinstall` and `install_env` run code
///   or alter the install environment.
/// - `[tasks]` definitions are inert until the user explicitly runs one
/// - no Tera template syntax anywhere — templates render while config and
///   tasks load and can run arbitrary commands via exec()
fn is_safe_config_body(body: &str) -> bool {
    // Fast reject: literal Tera delimiters in the raw text.
    if contains_template_syntax(body) {
        return false;
    }
    let Ok(toml::Value::Table(table)) = toml::from_str::<toml::Value>(body) else {
        // let the normal trust + parse flow handle invalid TOML
        return false;
    };
    // The raw-body check above misses escaped delimiters that TOML decodes,
    // e.g. `"{{ exec(...) }}"` becomes `{{ exec(...) }}`
    // after parsing and would still render via Tera. Re-check every decoded
    // string (keys and values, at any depth) so no exec()-capable template
    // can slip through into tool versions or task fields.
    if toml_table_has_template(&table) {
        return false;
    }
    table.iter().all(|(key, value)| match key.as_str() {
        "min_version" | "tasks" => true,
        "tools" => value.as_table().is_some_and(|tools| {
            tools.values().all(|version| match version {
                toml::Value::String(_) => true,
                toml::Value::Array(versions) => {
                    versions.iter().all(|v| matches!(v, toml::Value::String(_)))
                }
                _ => false,
            })
        }),
        _ => false,
    })
}

/// Whether any decoded string (table key or value, at any depth) contains
/// Tera template syntax. Used to catch escaped delimiters (e.g. `{{`)
/// that a raw-text scan misses but that still render after TOML parsing.
pub(crate) fn toml_value_has_template(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(s) => contains_template_syntax(s),
        toml::Value::Array(arr) => arr.iter().any(toml_value_has_template),
        toml::Value::Table(t) => toml_table_has_template(t),
        _ => false,
    }
}

fn toml_table_has_template(table: &toml::Table) -> bool {
    table
        .iter()
        .any(|(k, v)| contains_template_syntax(k) || toml_value_has_template(v))
}

fn is_tools_sorted(tools: &IndexMap<BackendArg, MiseTomlToolList>) -> bool {
    let mut last = None;
    for k in tools.keys() {
        if let Some(last) = last
            && k < last
        {
            return false;
        }
        last = Some(k);
    }
    true
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use indoc::{formatdoc, indoc};
    use insta::{assert_debug_snapshot, assert_snapshot};
    use test_log::test;

    use crate::dirs;
    use crate::file;
    use crate::task::Silent;
    use crate::test::replace_path;
    use crate::toolset::{CoreToolOptions, ToolRequest};
    use crate::{config::Config, dirs::CWD};

    use super::*;

    #[test]
    fn test_resolve_plugin_source_path() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("project/mise.toml");
        let absolute_plugin = temp.path().join("absolute/plugin");
        let home_plugin = dirs::HOME.join("plugins/example");

        for (source, expected) in [
            (
                absolute_plugin.to_string_lossy().into_owned(),
                absolute_plugin,
            ),
            (
                "./plugins/example".to_string(),
                temp.path().join("project/plugins/example"),
            ),
            ("../example".to_string(), temp.path().join("example")),
            ("~/plugins/example".to_string(), home_plugin),
        ] {
            assert_eq!(
                resolve_plugin_source_path(&config_path, source).unwrap(),
                expected.to_string_lossy()
            );
        }

        for source in [
            "https://github.com/example/plugin.git",
            "file:///tmp/plugin",
            "example/plugin",
        ] {
            assert_eq!(
                resolve_plugin_source_path(&config_path, source.to_string()).unwrap(),
                source
            );
        }
    }

    #[test]
    fn test_parse_monorepo_project_overrides() {
        let config = toml::from_str::<MiseToml>(indoc! {r#"
            [monorepo.projects."node:app"]
            root = "apps/app"
            metadata = { kind = "frontend" }
            depends = ["node:lib"]
            depends_add = ["cargo:core"]
            depends_remove = ["node:legacy"]

            [monorepo.projects."node:legacy"]
            remove = true
        "#})
        .unwrap();
        let projects = &config.monorepo.unwrap().projects;
        let app = projects.get("node:app").unwrap();

        assert_eq!(app.root.as_deref(), Some(Path::new("apps/app")));
        assert_eq!(
            app.metadata
                .as_ref()
                .unwrap()
                .get("kind")
                .map(String::as_str),
            Some("frontend")
        );
        assert_eq!(app.depends, Some(BTreeSet::from(["node:lib".to_string()])));
        assert_eq!(app.depends_add, BTreeSet::from(["cargo:core".to_string()]));
        assert_eq!(
            app.depends_remove,
            BTreeSet::from(["node:legacy".to_string()])
        );
        assert!(projects.get("node:legacy").unwrap().remove);
    }

    #[test]
    fn test_task_toml_boolean_overlay_presence() {
        let Tasks(mut tasks) = toml::from_str(
            r#"
            [explicit]
            hide = false
            raw = false
            raw_args = false
            interactive = false
            quiet = false
            silent = false

            [omitted]
            description = "no boolean overrides"
            "#,
        )
        .unwrap();

        let script_task = || Task {
            hide: true,
            raw: true,
            raw_args: true,
            interactive: true,
            quiet: true,
            silent: Silent::Bool(true),
            ..Default::default()
        };

        let mut explicit = script_task();
        explicit.merge_toml_overlay(tasks.remove("explicit").unwrap());
        assert!(!explicit.hide);
        assert!(!explicit.raw);
        assert!(!explicit.raw_args);
        assert!(!explicit.interactive);
        assert!(!explicit.quiet);
        assert_eq!(explicit.silent, Silent::Off);

        let mut omitted = script_task();
        omitted.merge_toml_overlay(tasks.remove("omitted").unwrap());
        assert!(omitted.hide);
        assert!(omitted.raw);
        assert!(omitted.raw_args);
        assert!(omitted.interactive);
        assert!(omitted.quiet);
        assert_eq!(omitted.silent, Silent::Bool(true));
    }

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

        assert_snapshot!(replace_path(&format!("{:#?}", cf)));
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
    async fn test_env_directive_written_order() {
        // Directives inside `[env]._` are emitted in the order they are written,
        // rather than the previous fixed path -> file -> source order, so a later
        // directive's template can reference a variable set by an earlier one.
        // https://github.com/jdx/mise/discussions/3783
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [env]
            _.source = "a.sh"
            _.path = "b"
            _.file = "c.env"
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let kinds: Vec<&str> = cf
            .env
            .0
            .iter()
            .map(|d| match d {
                EnvDirective::Source(..) => "source",
                EnvDirective::Path(..) => "path",
                EnvDirective::File(..) => "file",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["source", "path", "file"]);
    }

    #[tokio::test]
    async fn test_env_var_in_tool() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            r#"
        [env]
        TERRAFORM_VERSION = '1.0.0'
        JQ_PREFIX = '1.6'

        [tools]
        terraform = "{{env.TERRAFORM_VERSION}}"
        jq = { prefix = "{{ env.JQ_PREFIX }}" }
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        assert_snapshot!(replace_path(&format!(
            "{:#?}",
            cf.to_tool_request_set().unwrap().tools
        )));
    }

    /// A `:` inside a template belongs to the template, not to a version selector, and the
    /// template has not been rendered yet when the config is deserialized. Selectors must still
    /// come out the other side, so this pins the rendered `ToolRequest`, not the raw string.
    ///
    /// See: <https://github.com/jdx/mise/discussions/5531>
    #[tokio::test]
    async fn test_colon_in_templated_tool_version() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            r#"
        [env]
        BRANCH = 'main'

        [tools]
        terraform = "{{ exec(command='echo VER: 1.0.0') | split(pat=': ') | last | trim }}"
        jq = { ref = "{{ env.BRANCH }}" }
        node = "{% if true %}20.0.0{% else %}18.0.0{% endif %}"
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let trs = cf.to_tool_request_set().unwrap();
        let version_of = |short: &str| {
            trs.tools
                .iter()
                .find(|(ba, _)| ba.short == short)
                .unwrap_or_else(|| panic!("{short} missing"))
                .1[0]
                .version()
        };
        // the colon belonged to the template, not to a selector
        assert_eq!(version_of("terraform"), "1.0.0");
        // ...and a real selector still survives being rendered late
        assert_eq!(version_of("jq"), "ref:main");
        assert_eq!(version_of("node"), "20.0.0");
    }

    /// The version is no longer validated at deserialize time, so a template that renders to an
    /// unknown selector fails later and further from the config. Pin that the error still names
    /// the file, the template, and what it became.
    #[tokio::test]
    async fn test_templated_tool_version_rendering_to_bad_selector() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            "[tools]\nterraform = \"{{ exec(command='echo bogus:1.0') | trim }}\"\n",
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let err = cf.to_tool_request_set().unwrap_err().to_string();
        assert!(err.contains("invalid version for terraform"), "{err}");
        assert!(err.contains(".test.mise.toml"), "{err}");
        assert!(err.contains("rendered to \"bogus:1.0\""), "{err}");
    }

    #[test]
    fn test_tool_selectors() {
        #[derive(Deserialize)]
        struct ToolConfig {
            #[allow(dead_code)]
            tools: IndexMap<BackendArg, MiseTomlToolList>,
        }

        let valid = indoc! {r#"
        [tools]
        node = { version = "20" }
        go = { prefix = "1.22" }
        python = { ref = "main" }
        shellcheck = { path = "/opt/shellcheck" }
        ruby = [{ prefix = "3.3", os = "linux-x64" }]
        "#};
        assert!(toml::from_str::<ToolConfig>(valid).is_ok());

        for (config, expected) in [
            (
                "[tools]\nnode = { version = \"20\", prefix = \"20\" }\n",
                "tool definition cannot specify both `version` and `prefix`",
            ),
            (
                "[tools]\nnode = [{ path = \"/opt/node\", ref = \"main\" }]\n",
                "tool definition cannot specify both `path` and `ref`",
            ),
            (
                "[tools]\nnode = [{ os = \"linux-x64\" }]\n",
                "tool definition must include exactly one of `version`, `path`, `prefix`, or `ref`",
            ),
            (
                "[tools]\nnode = { prefix = 20 }\n",
                "tool selector `prefix` must be a string",
            ),
        ] {
            let err = match toml::from_str::<ToolConfig>(config) {
                Ok(_) => panic!("expected tool selector validation to fail"),
                Err(err) => err,
            };
            assert!(err.to_string().contains(expected), "{err}");
        }
    }

    #[tokio::test]
    async fn test_bootstrap_packages() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            r#"
        [bootstrap.packages]
        "apt:libssl-dev" = "latest"
        "apt:curl" = "8.5.0-2"
        "brew:postgresql@17" = "latest"
        "future-manager:whatever" = "latest"

        [bootstrap.brew.taps]
        "railwaycat/emacsmacport" = "https://github.com/railwaycat/homebrew-emacsmacport"

        [bootstrap.repos]
        "~/src/dotfiles" = { url = "https://github.com/jdx/dotfiles.git", ref = "main" }

        [bootstrap.hooks.pre-packages]
        run = "echo preparing"

        [bootstrap.hooks.post-tools]
        run = ["echo one", "echo two"]
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let system = cf.bootstrap_config().unwrap();
        fn version_of(entry: &crate::system::PackageEntryToml) -> &str {
            match entry {
                crate::system::PackageEntryToml::Version(v) => v,
                other => panic!("expected a string version entry, got {other:?}"),
            }
        }
        assert_eq!(
            version_of(system.packages.get("apt:libssl-dev").unwrap()),
            "latest"
        );
        assert_eq!(
            version_of(system.packages.get("apt:curl").unwrap()),
            "8.5.0-2"
        );
        assert_eq!(
            version_of(system.packages.get("brew:postgresql@17").unwrap()),
            "latest"
        );
        assert_eq!(
            system.brew.taps.get("railwaycat/emacsmacport").unwrap(),
            "https://github.com/railwaycat/homebrew-emacsmacport"
        );
        let repo = system.repos.get("~/src/dotfiles").unwrap();
        assert_eq!(
            repo.url.as_deref(),
            Some("https://github.com/jdx/dotfiles.git")
        );
        assert_eq!(repo.git_ref.as_deref(), Some("main"));
        assert!(system.hooks.get("pre-packages").unwrap().is_table());
        assert!(system.hooks.get("post-tools").unwrap().is_table());
        assert_eq!(system.user.login_shell, None);
        // unknown managers parse fine (forward compatibility)
        assert_eq!(
            version_of(system.packages.get("future-manager:whatever").unwrap()),
            "latest"
        );

        // no [bootstrap] section -> None
        file::write(&p, "[tools]\n").unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        assert!(cf.bootstrap_config().is_none());
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_bootstrap_packages_table_entries() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test-bootstrap-os.mise.toml");
        file::write(
            &p,
            r#"
        [bootstrap.packages]
        "apt:libssl-dev" = "latest"
        "brew-cask:firefox" = { version = "latest", os = ["macos"] }
        "apt:curl" = { version = "8.5.0-2", os = "linux" }
        "brew:ffmpeg" = { version = "latest", os = ["linux", "macos/arm64"], future_key = "x" }
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let system = cf.bootstrap_config().unwrap();

        // string form still parses as a plain version
        assert!(matches!(
            system.packages.get("apt:libssl-dev").unwrap(),
            crate::system::PackageEntryToml::Version(v) if v == "latest"
        ));

        // table form stays raw TOML: version + os array preserved as written
        let firefox = match system.packages.get("brew-cask:firefox").unwrap() {
            crate::system::PackageEntryToml::Table(t) => t,
            other => panic!("expected a table entry, got {other:?}"),
        };
        assert_eq!(
            firefox.get("version").and_then(|v| v.as_str()),
            Some("latest")
        );
        let os = firefox.get("os").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            os.iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>(),
            vec!["macos"]
        );

        // os accepts a bare string
        let curl = match system.packages.get("apt:curl").unwrap() {
            crate::system::PackageEntryToml::Table(t) => t,
            other => panic!("expected a table entry, got {other:?}"),
        };
        assert_eq!(
            curl.get("version").and_then(|v| v.as_str()),
            Some("8.5.0-2")
        );
        assert_eq!(curl.get("os").and_then(|v| v.as_str()), Some("linux"));

        // unknown keys survive the parse (forward compatibility; validation
        // warns at aggregation time, not here)
        let ffmpeg = match system.packages.get("brew:ffmpeg").unwrap() {
            crate::system::PackageEntryToml::Table(t) => t,
            other => panic!("expected a table entry, got {other:?}"),
        };
        let os = ffmpeg.get("os").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            os.iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>(),
            vec!["linux", "macos/arm64"]
        );
        assert_eq!(ffmpeg.get("future_key").and_then(|v| v.as_str()), Some("x"));

        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_bootstrap_login_shell() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            r#"
        [bootstrap.user]
        login_shell = "/bin/zsh"
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let system = cf.bootstrap_config().unwrap();
        assert_eq!(system.user.login_shell.as_deref(), Some("/bin/zsh"));
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_bootstrap_mise_shell_activate() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            r#"
        [bootstrap.mise_shell_activate]
        zsh = true
        bash = false
        fish = {enabled = true}
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let system = cf.bootstrap_config().unwrap();
        assert_eq!(
            system.mise_shell_activate.get("zsh"),
            Some(&toml::Value::Boolean(true))
        );
        assert_eq!(
            system.mise_shell_activate.get("bash"),
            Some(&toml::Value::Boolean(false))
        );
        assert!(system.mise_shell_activate.get("fish").unwrap().is_table());
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_bootstrap_macos_defaults() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            r#"
        [bootstrap.macos.defaults]
        NSGlobalDomain = { KeyRepeat = 2, ApplePressAndHoldEnabled = false }
        "com.apple.dock" = { autohide = true, tilesize = 48, magnification-scale = 1.5, orientation = "left", future-array = [1, 2] }

        [bootstrap.macos.dock]
        show_recents = false

        [bootstrap.macos.finder]
        show_all_files = true
        preferred_view_style = "list"

        [bootstrap.macos.keyboard]
        initial_key_repeat = 15

        [bootstrap.macos.trackpad]
        tap_to_click = true
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let system = cf.bootstrap_config().unwrap();
        let global = system.macos.defaults.get("NSGlobalDomain").unwrap();
        assert_eq!(global.get("KeyRepeat").unwrap(), &toml::Value::Integer(2));
        assert_eq!(
            global.get("ApplePressAndHoldEnabled").unwrap(),
            &toml::Value::Boolean(false)
        );
        let dock = system.macos.defaults.get("com.apple.dock").unwrap();
        assert_eq!(dock.get("autohide").unwrap(), &toml::Value::Boolean(true));
        assert_eq!(dock.get("tilesize").unwrap(), &toml::Value::Integer(48));
        assert_eq!(
            dock.get("magnification-scale").unwrap(),
            &toml::Value::Float(1.5)
        );
        assert_eq!(
            dock.get("orientation").unwrap(),
            &toml::Value::String("left".into())
        );
        assert!(dock.get("future-array").unwrap().is_array());
        assert_eq!(
            system.macos.dock.get("show_recents").unwrap(),
            &toml::Value::Boolean(false)
        );
        assert_eq!(
            system.macos.finder.get("show_all_files").unwrap(),
            &toml::Value::Boolean(true)
        );
        assert_eq!(
            system.macos.finder.get("preferred_view_style").unwrap(),
            &toml::Value::String("list".into())
        );
        assert_eq!(
            system.macos.keyboard.get("initial_key_repeat").unwrap(),
            &toml::Value::Integer(15)
        );
        assert_eq!(
            system.macos.trackpad.get("tap_to_click").unwrap(),
            &toml::Value::Boolean(true)
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_bootstrap_macos_launchd_agents() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            r#"
        [bootstrap.macos.launchd.agents.my-sync]
        program = "~/.local/bin/my-sync"
        args = ["--watch"]
        run_at_load = true
        start_interval = 300
        start_calendar_interval = { hour = 2, minute = 0 }
        environment = { PATH = "/usr/bin:/bin" }
        working_directory = "~"
        stdout_path = "~/Library/Logs/my-sync.log"
        kickstart = true

        [bootstrap.macos.launchd.agents.my-backup]
        program = "~/.local/bin/my-backup"
        start_calendar_interval = [{ hour = 3 }, { hour = 12, weekday = 1 }]
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let system = cf.bootstrap_config().unwrap();
        let agent = system.macos.launchd.agents.get("my-sync").unwrap();
        assert_eq!(agent.program.as_deref(), Some("~/.local/bin/my-sync"));
        assert_eq!(agent.args, vec!["--watch"]);
        assert!(agent.run_at_load);
        assert_eq!(agent.start_interval, Some(300));
        let crate::system::launchd::LaunchdCalendarIntervals::Single(interval) =
            agent.start_calendar_interval.as_ref().unwrap()
        else {
            panic!("expected single calendar interval");
        };
        assert_eq!(interval.hour, Some(2));
        assert_eq!(interval.minute, Some(0));
        assert_eq!(
            agent.environment.get("PATH").map(String::as_str),
            Some("/usr/bin:/bin")
        );
        assert_eq!(agent.working_directory.as_deref(), Some("~"));
        assert_eq!(
            agent.stdout_path.as_deref(),
            Some("~/Library/Logs/my-sync.log")
        );
        assert!(agent.kickstart);
        let backup = system.macos.launchd.agents.get("my-backup").unwrap();
        let crate::system::launchd::LaunchdCalendarIntervals::Multiple(intervals) =
            backup.start_calendar_interval.as_ref().unwrap()
        else {
            panic!("expected multiple calendar intervals");
        };
        assert_eq!(intervals.len(), 2);
        assert_eq!(intervals[0].hour, Some(3));
        assert_eq!(intervals[1].hour, Some(12));
        assert_eq!(intervals[1].weekday, Some(1));
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_update_bootstrap_package() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        // creates [bootstrap.packages] when absent, preserves other sections
        file::write(&p, "[tools]\njq = \"latest\"\n").unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        cf.update_bootstrap_package("apt:curl", "latest").unwrap();
        cf.update_bootstrap_package("brew:postgresql@17", "latest")
            .unwrap();
        // overrides an existing pin in place
        cf.update_bootstrap_package("apt:curl", "8.5.0-2").unwrap();
        assert_snapshot!(cf.dump().unwrap(), @r#"
        [tools]
        jq = "latest"

        [bootstrap.packages]
        "apt:curl" = "8.5.0-2"
        "brew:postgresql@17" = "latest"
        "#);
        let system = cf.bootstrap_config().unwrap();
        assert_eq!(
            system.packages.get("apt:curl").unwrap().version(),
            Some("8.5.0-2")
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_update_bootstrap_package_preserves_table_entries() {
        let _config = Config::get().await.unwrap();
        let p = CWD
            .as_ref()
            .unwrap()
            .join(".test-bootstrap-use-os.mise.toml");
        file::write(
            &p,
            r#"[bootstrap.packages]
"apt:curl" = "latest"
"brew-cask:firefox" = { version = "latest", os = ["macos"], future_key = "x" }
"#,
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        // table entry: version updated in place, os and unknown keys preserved
        cf.update_bootstrap_package("brew-cask:firefox", "1.2.3")
            .unwrap();
        // string entry: stays a plain string
        cf.update_bootstrap_package("apt:curl", "8.5.0-2").unwrap();
        // new entry: still written as a plain string
        cf.update_bootstrap_package("apt:jq", "latest").unwrap();
        assert_snapshot!(cf.dump().unwrap(), @r#"
        [bootstrap.packages]
        "apt:curl" = "8.5.0-2"
        "brew-cask:firefox" = { version = "1.2.3", os = ["macos"], future_key = "x" }
        "apt:jq" = "latest"
        "#);

        // the in-memory packages map stays consistent with the document
        let system = cf.bootstrap_config().unwrap();
        match system.packages.get("brew-cask:firefox").unwrap() {
            crate::system::PackageEntryToml::Table(t) => {
                assert_eq!(t.get("version").and_then(|v| v.as_str()), Some("1.2.3"));
                let os = t.get("os").and_then(|v| v.as_array()).unwrap();
                assert_eq!(
                    os.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
                    vec!["macos"]
                );
                assert_eq!(t.get("future_key").and_then(|v| v.as_str()), Some("x"));
            }
            other => panic!("expected a table entry, got {other:?}"),
        }
        assert_eq!(
            system.packages.get("apt:curl").unwrap().version(),
            Some("8.5.0-2")
        );
        assert_eq!(
            system.packages.get("apt:jq").unwrap().version(),
            Some("latest")
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_update_bootstrap_package_preserves_sub_table_entries() {
        let _config = Config::get().await.unwrap();
        let p = CWD
            .as_ref()
            .unwrap()
            .join(".test-bootstrap-use-subtable.mise.toml");
        // both are legal TOML spellings of a table entry and both deserialize
        // to PackageEntryToml::Table, so `use` must preserve `os` for them too
        file::write(
            &p,
            r#"[bootstrap.packages]
"brew:ffmpeg".version = "latest"
"brew:ffmpeg".os = ["linux"]

[bootstrap.packages."brew-cask:firefox"]
version = "latest"
os = ["macos"]
future_key = "x"
"#,
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        cf.update_bootstrap_package("brew-cask:firefox", "1.2.3")
            .unwrap();
        cf.update_bootstrap_package("brew:ffmpeg", "7.1").unwrap();

        // the os restriction survives in the written document, not just in
        // the in-memory map
        let dumped = cf.dump().unwrap();
        assert!(
            dumped.contains(r#"os = ["macos"]"#),
            "sub-table os was dropped:\n{dumped}"
        );
        assert!(
            dumped.contains(r#"os = ["linux"]"#),
            "dotted-key os was dropped:\n{dumped}"
        );
        assert!(
            dumped.contains(r#"future_key = "x""#),
            "sub-table unknown key was dropped:\n{dumped}"
        );

        file::write(&p, &dumped).unwrap();
        let system = MiseToml::from_file(&p).unwrap().bootstrap_config().unwrap();
        for (spec, version, os) in [
            ("brew-cask:firefox", "1.2.3", "macos"),
            ("brew:ffmpeg", "7.1", "linux"),
        ] {
            match system.packages.get(spec).unwrap() {
                crate::system::PackageEntryToml::Table(t) => {
                    assert_eq!(t.get("version").and_then(|v| v.as_str()), Some(version));
                    let list = t.get("os").and_then(|v| v.as_array()).unwrap();
                    assert_eq!(
                        list.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
                        vec![os]
                    );
                }
                other => panic!("expected {spec} to stay a table entry, got {other:?}"),
            }
        }
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_core_options_do_not_normalize_version_placeholder() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            r#"
        [tools]
        node = { version = "1.0.0", depends = ["{{version}}"], install_env = { FOO = "{{version}}" }, url = "https://example.com/{{version}}" }
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let trs = cf.to_tool_request_set().unwrap();
        let node_req = trs
            .tools
            .iter()
            .find(|(ba, _)| ba.short == "node")
            .and_then(|(_, reqs)| reqs.first())
            .unwrap();
        let opts = node_req.options();

        assert_eq!(opts.depends, Some(vec!["{{version}}".to_string()]));
        assert_eq!(
            opts.install_env.get("FOO"),
            Some(&EnvValue::String("{{version}}".to_string()))
        );
        assert_eq!(opts.get("url"), Some("https://example.com/{version}"));
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_tool_options_preserve_quoted_literal_dotted_keys() {
        crate::toolset::install_state::init().await.unwrap();
        let cf = MiseToml::from_str(
            r#"
            [tools."aqua:example/vars-tool"]
            version = "1.0.0"
            fixture_version = "2.0.0"
            "vars.fixture_version" = "1.0.0"
            "#,
            std::path::Path::new("mise.toml"),
        )
        .unwrap();
        let trs = cf.to_tool_request_set().unwrap();
        let (_, requests, _) = trs.iter().next().unwrap();
        let opts = requests[0].options();

        assert_eq!(opts.get("fixture_version"), Some("2.0.0"));
        assert_eq!(opts.get("vars.fixture_version"), Some("1.0.0"));
    }

    #[tokio::test]
    async fn test_env_source_var_in_tool() {
        let cwd = CWD.as_ref().unwrap();
        let script = cwd.join("set-go-version.sh");
        let config_file = cwd.join(".test.mise.toml");

        file::write(&script, "export MY_GO_VERSION=\"1.2.3\"\n").unwrap();
        file::write(
            &config_file,
            r#"
        [env]
        _.source = "./set-go-version.sh"

        [tools]
        go = "{{env.MY_GO_VERSION}}"
        "#,
        )
        .unwrap();

        let config = Config::reset().await.unwrap();
        let trs = config.get_tool_request_set().await.unwrap();
        let go_req = trs
            .tools
            .iter()
            .find(|(ba, _)| ba.short == "go")
            .and_then(|(_, reqs)| reqs.first())
            .unwrap();

        assert_eq!(go_req.version(), "1.2.3");

        file::remove_file(&config_file).unwrap();
        file::remove_file(&script).unwrap();
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

    #[test]
    fn test_env_and_vars_array_boundaries() {
        assert!(toml::from_str::<MiseToml>(r#"env = [{ FOO = "one" }, { BAR = "two" }]"#).is_ok());
        assert!(toml::from_str::<MiseToml>("env = []").is_ok());
        assert!(
            toml::from_str::<MiseToml>(
                r#"
                [env]
                _.source = ["./first.sh", "./second.sh"]

                [vars]
                _.file = ["./first.env", "./second.env"]
                "#,
            )
            .is_ok()
        );

        for invalid in [
            r#"env = [[{ FOO = "bar" }]]"#,
            r#"env = [{ FOO = "bar" }, [{ BAR = "baz" }]]"#,
            r#"vars = []"#,
            r#"vars = [{ FOO = "bar" }]"#,
            r#"vars = [[{ FOO = "bar" }]]"#,
        ] {
            assert!(
                toml::from_str::<MiseToml>(invalid).is_err(),
                "expected config to be rejected: {invalid}"
            );
        }
    }

    #[test]
    fn test_task_env_and_vars_arrays_invalid() {
        for field in ["env", "vars"] {
            let config = formatdoc! {r#"
                [tasks.example]
                run = "echo ok"
                {field} = [{{ FOO = "bar" }}]
            "#};
            assert!(
                toml::from_str::<MiseToml>(&config).is_err(),
                "expected task {field} array to be rejected"
            );

            let config = formatdoc! {r#"
                [task_templates.example]
                {field} = [{{ FOO = "bar" }}]
            "#};
            assert!(
                toml::from_str::<MiseToml>(&config).is_err(),
                "expected task template {field} array to be rejected"
            );
        }
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

        let env = parse_env(formatdoc! {r#"
            env_file = ".env"
            dotenv = ".env2"
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
            [tool_alias.node.versions]
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

        assert_debug_snapshot!(cf.tool_alias);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf);
        file::remove_file(&p).unwrap();
    }

    #[test]
    fn test_tasks_confirm_parses() {
        let body = r#"
[tasks.deploy]
confirm = { message = "Are you sure you want to deploy to ({{ env.HOME }})?", default = "no" }
run = 'echo " $usage_environment"'
"#;

        let path = std::path::Path::new("/tmp/mise.toml");
        let rf = MiseToml::from_str(body, path).unwrap();
        let task = rf.tasks.0.get("deploy").expect("deploy task should exist");

        assert!(matches!(
            task.confirm,
            Some(crate::task::TaskConfirm::Options { .. })
        ));
    }

    #[test]
    fn test_task_templates_confirm_parses() {
        let body = r#"
[task_templates.deploy]
confirm = { message = "Are you sure?", default = "no" }
run = 'echo "template"'
"#;

        let path = std::path::Path::new("/tmp/mise.toml");
        let rf = MiseToml::from_str(body, path).unwrap();
        let template = rf
            .task_templates
            .0
            .get("deploy")
            .expect("deploy template should exist");

        assert!(matches!(
            template.confirm,
            Some(crate::task::TaskConfirm::Options { .. })
        ));
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
    async fn test_replace_versions_keeps_sorted_across_multiple_updates() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".multiple-tool-sort.mise.toml");
        file::write(&p, "[tools]\ntiny-ref = \"1\"\n").unwrap();
        let cf = MiseToml::from_file(&p).unwrap();

        for tool in ["dummy", "tiny-local", "tiny"] {
            let ba = BackendArg::from(tool);
            cf.replace_versions(
                &ba,
                vec![ToolRequest::new(Arc::new(ba.clone()), "1", ToolSource::Unknown).unwrap()],
            )
            .unwrap();
        }

        assert_eq!(
            cf.dump().unwrap(),
            "[tools]\ndummy = \"1\"\ntiny = \"1\"\ntiny-local = \"1\"\ntiny-ref = \"1\"\n"
        );
        file::remove_file(&p).unwrap();
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
    fn test_vars_mise_is_not_a_directive_namespace() {
        let err = toml::from_str::<MiseToml>(indoc! {r#"
            [vars.mise]
            file = ".env"
        "#})
        .unwrap_err()
        .to_string();
        assert!(err.contains("`vars.mise` is not supported"), "{err}");

        let err = toml::from_str::<MiseToml>(r#"vars = [[{ mise = { file = ".env" } }]]"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("`vars.mise` is not supported"), "{err}");
    }

    #[test]
    fn test_env_default_entries() {
        let toml = indoc! {r#"
        [env]
        foo1 = { default = "fallback" }
        foo2 = { default = 2, tools = true }
        "#}
        .to_string();
        assert_snapshot!(parse_env(toml), @r#"
        foo1 default=fallback
        foo2 default=2
        "#);
    }

    #[test]
    fn test_env_default_invalid_combinations() {
        let err = parse_error("[env]\nFOO = { value = \"x\", default = \"y\" }\n");
        assert!(
            err.contains("cannot have both 'value' and 'default'"),
            "{err}"
        );
        let err = parse_error("[env]\nFOO = { required = true, default = \"y\" }\n");
        assert!(
            err.contains("cannot have both 'default' and 'required'"),
            "{err}"
        );
        let err = parse_error("[env]\nFOO = { age = \"AGE-SECRET\", default = \"y\" }\n");
        assert!(
            err.contains("cannot have both 'age' and 'default'"),
            "{err}"
        );
        let err = parse_error("[env]\nFOO = { default = false }\n");
        assert!(err.contains("default cannot be a boolean"), "{err}");
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
        _.file = ".env2"
        _.path = "/bar"
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

    fn parse_error(toml: &str) -> String {
        #[derive(Debug, Deserialize)]
        struct TestConfig {
            #[allow(dead_code)]
            env: EnvList,
        }

        toml::from_str::<TestConfig>(toml).unwrap_err().to_string()
    }

    #[test]
    fn test_is_safe_config_body() {
        assert!(is_safe_config_body(""));
        assert!(is_safe_config_body(indoc! {r#"
        min_version = "2024.1.1"
        [tools]
        node = "20"
        python = ["3.11", "3.12"]
        "cargo:eza" = "latest"
        "#}));
        // tasks are inert until the user explicitly runs one
        assert!(is_safe_config_body(indoc! {r#"
        [tasks.build]
        run = "cargo build"
        dir = "src"
        env = { FOO = "bar" }
        [tasks.test]
        depends = ["build"]
        run = ["cargo test"]
        "#}));

        // templates can execute commands
        assert!(!is_safe_config_body(indoc! {r#"
        [tools]
        node = "{{ exec(command='echo 20') }}"
        "#}));
        // tool options like postinstall/install_env run code
        assert!(!is_safe_config_body(indoc! {r#"
        [tools]
        node = { version = "20", postinstall = "corepack enable" }
        "#}));
        assert!(!is_safe_config_body(indoc! {r#"
        [tools]
        node = [{ version = "20" }]
        "#}));
        // tasks with templates render (and can exec) while loading
        assert!(!is_safe_config_body(indoc! {r#"
        [tasks.build]
        run = "cargo build"
        description = "{{ exec(command='echo hi') }}"
        "#}));
        // escaped Tera delimiters ({ == '{', } == '}') decode to
        // `{{ exec(...) }}` after TOML parsing and must not bypass the check
        assert!(!is_safe_config_body(
            "[tools]\nnode = \"\\u007b\\u007b exec(command='echo 20') \\u007d\\u007d\"\n"
        ));
        assert!(!is_safe_config_body(
            "[tasks.build]\nrun = \"cargo build\"\ndescription = \"\\u007b\\u007b exec(command='echo hi') \\u007d\\u007d\"\n"
        ));
        // an escaped delimiter in a key must also be caught
        assert!(!is_safe_config_body(
            "[tasks]\n\"\\u007b\\u007b exec() \\u007d\\u007d\" = { run = \"x\" }\n"
        ));
        // anything beyond min_version/tools/tasks requires trust
        for body in [
            "[env]\nFOO = \"bar\"",
            "[task_config]\nincludes = [\"tasks.toml\"]",
            "[hooks]\nenter = \"echo hi\"",
            "[settings]\nparanoid = false",
            "[alias]\nnode = \"asdf:foo/bar\"",
            "[plugins]\nfoo = \"https://example.com/foo.git\"",
            "env_file = \".env\"",
        ] {
            assert!(!is_safe_config_body(body), "should require trust: {body}");
        }
        // invalid toml falls back to the normal trust + parse flow
        assert!(!is_safe_config_body("[tools"));
    }

    #[tokio::test]
    async fn test_table_syntax_preserves_registry_defaults() {
        // Test for #8039: table syntax like `ansible = { version = "latest" }`
        // should preserve registry defaults (e.g. uvx=false, pipx_args=--include-deps)
        let _config = Config::get().await.unwrap();
        let cf = parse(formatdoc! {r#"
            [tools]
            ansible = {{ version = "latest" }}
        "#});
        let trs = cf.to_tool_request_set().unwrap();
        let tools = trs.tools;
        // Find the ansible tool request
        let ansible_requests = tools
            .iter()
            .find(|(ba, _)| ba.short == "ansible")
            .map(|(_, reqs)| reqs)
            .expect("ansible should be in tool request set");
        let opts = ansible_requests[0].options();
        assert_eq!(
            opts.get_string("uvx").as_deref(),
            Some("false"),
            "registry default uvx=false should be preserved with table syntax"
        );
        assert_eq!(
            opts.get("pipx_args"),
            Some("--include-deps"),
            "registry default pipx_args=--include-deps should be preserved with table syntax"
        );

        // Also verify that user-provided options override registry defaults
        let cf2 = parse(formatdoc! {r#"
            [tools]
            ansible = {{ version = "latest", uvx = "true" }}
        "#});
        let trs2 = cf2.to_tool_request_set().unwrap();
        let ansible2 = trs2
            .tools
            .iter()
            .find(|(ba, _)| ba.short == "ansible")
            .map(|(_, reqs)| reqs)
            .expect("ansible should be in tool request set");
        let opts2 = ansible2[0].options();
        assert_eq!(
            opts2.get_string("uvx").as_deref(),
            Some("true"),
            "user-provided uvx=true should override registry default uvx=false"
        );
        assert_eq!(
            opts2.get("pipx_args"),
            Some("--include-deps"),
            "non-overridden registry default pipx_args should still be preserved"
        );
    }

    #[tokio::test]
    async fn test_table_syntax_user_opts_override_registry_defaults() {
        let _config = Config::get().await.unwrap();
        let cf = parse(formatdoc! {r#"
            [tools]
            podman = {{ version = "latest", rename_exe = "podman-remote" }}
        "#});
        let trs = cf.to_tool_request_set().unwrap();
        let podman = trs
            .tools
            .iter()
            .find(|(ba, _)| ba.short == "podman")
            .map(|(_, reqs)| reqs)
            .expect("podman should be in tool request set");

        assert_eq!(
            podman[0].options().get("rename_exe"),
            Some("podman-remote"),
            "user-provided rename_exe should override the registry default"
        );
    }

    #[tokio::test]
    async fn test_depends_field_parsing() {
        let _config = Config::get().await.unwrap();
        let cf = parse(formatdoc! {r#"
            [tools]
            dummy = {{ version = "latest", depends = ["tiny"] }}
        "#});
        let trs = cf.to_tool_request_set().unwrap();
        let dummy = trs
            .tools
            .iter()
            .find(|(ba, _)| ba.short == "dummy")
            .map(|(_, reqs)| reqs)
            .expect("dummy should be in tool request set");
        let opts = dummy[0].options();
        assert_eq!(
            opts.depends,
            Some(vec!["tiny".to_string()]),
            "depends should be parsed as a named field"
        );
        assert!(
            !opts.opts.contains_key("depends"),
            "depends should not leak into opts"
        );
    }

    #[tokio::test]
    async fn test_depends_field_single_string() {
        let _config = Config::get().await.unwrap();
        let cf = parse(formatdoc! {r#"
            [tools]
            dummy = {{ version = "latest", depends = "tiny" }}
        "#});
        let trs = cf.to_tool_request_set().unwrap();
        let dummy = trs
            .tools
            .iter()
            .find(|(ba, _)| ba.short == "dummy")
            .map(|(_, reqs)| reqs)
            .expect("dummy should be in tool request set");
        let opts = dummy[0].options();
        assert_eq!(
            opts.depends,
            Some(vec!["tiny".to_string()]),
            "single string depends should be wrapped in a vec"
        );
    }

    #[tokio::test]
    async fn test_os_field_single_string() {
        let _config = Config::get().await.unwrap();
        let cf = parse(formatdoc! {r#"
            [tools]
            dummy = {{ version = "latest", os = "linux" }}
        "#});
        let trs = cf.to_tool_request_set().unwrap();
        let dummy = trs
            .tools
            .iter()
            .find(|(ba, _)| ba.short == "dummy")
            .map(|(_, reqs)| reqs)
            .expect("dummy should be in tool request set");
        let opts = dummy[0].options();
        assert_eq!(
            opts.os,
            Some(vec!["linux".to_string()]),
            "single string os should be wrapped in a vec"
        );
        assert!(
            !opts.opts.contains_key("os"),
            "os should not leak into opts"
        );
    }

    #[tokio::test]
    async fn test_replace_versions_preserves_named_core_options() {
        let _config = Config::get().await.unwrap();
        let p = CWD
            .as_ref()
            .unwrap()
            .join(".replace-core-options.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            needs-dummy = "1.0.0"
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let needs_dummy = "needs-dummy".into();
        let mut options = ToolVersionOptions {
            core: CoreToolOptions {
                os: Some(vec!["linux".to_string()]),
                depends: Some(vec!["dummy".to_string()]),
                ..Default::default()
            },
            ..Default::default()
        };
        options
            .install_env
            .insert("FOO".to_string(), EnvValue::from("bar"));

        cf.replace_versions(
            &needs_dummy,
            vec![
                ToolRequest::new_opts(
                    Arc::new("needs-dummy".into()),
                    "1.0.1",
                    options,
                    ToolSource::Unknown,
                )
                .unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(dump.contains("depends"), "depends should be written back");
        assert!(
            dump.contains("dummy"),
            "depends value should be written back"
        );
        assert!(dump.contains("os"), "os should be written back");
        assert!(
            dump.contains("install_env"),
            "install_env should be written back"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_preserves_comments() {
        // https://github.com/jdx/mise/discussions/4797
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".replace-comments.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            # renovate: datasource=github-releases depName=node
            node = "16.0.0" # keep me
            dummy = ["1.0.0"] # keep me too
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let node = "node".into();
        cf.replace_versions(
            &node,
            vec![ToolRequest::new(Arc::new("node".into()), "18.0.0", ToolSource::Unknown).unwrap()],
        )
        .unwrap();
        let dummy = "dummy".into();
        cf.replace_versions(
            &dummy,
            vec![
                ToolRequest::new(Arc::new("dummy".into()), "1.0.1", ToolSource::Unknown).unwrap(),
                ToolRequest::new(Arc::new("dummy".into()), "2.0.0", ToolSource::Unknown).unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains("# renovate: datasource=github-releases depName=node"),
            "comment above the tool should survive: {dump}"
        );
        assert!(
            dump.contains(r#"node = "18.0.0" # keep me"#),
            "comment after the version should survive: {dump}"
        );
        assert!(
            dump.contains(r#"dummy = ["1.0.1", "2.0.0"] # keep me too"#),
            "comment after a multi-version tool should survive: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_preserves_array_element_comments() {
        // https://github.com/jdx/mise/discussions/4797
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".array-comments.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            dummy = [
              "1.0.0", # first
              "2.0.0", # second
            ]
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let dummy = "dummy".into();
        cf.replace_versions(
            &dummy,
            vec![
                ToolRequest::new(Arc::new("dummy".into()), "1.0.1", ToolSource::Unknown).unwrap(),
                ToolRequest::new(Arc::new("dummy".into()), "2.0.1", ToolSource::Unknown).unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains(indoc! {r#"
                dummy = [
                  "1.0.1", # first
                  "2.0.1", # second
                ]"#}),
            "the array should keep its layout with each comment on its own element: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_array_comments_stay_positional() {
        // A comment written after an array element belongs to the decor of the element that
        // follows it, so it is tied to a position rather than to a version. Reordering the
        // versions therefore leaves the comments where the user put them, which is the same rule
        // the scalar case follows: a comment survives its value changing.
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".array-reorder.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            dummy = [
              "1.0.0", # first
              "2.0.0", # second
            ]
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let dummy = "dummy".into();
        cf.replace_versions(
            &dummy,
            vec![
                ToolRequest::new(Arc::new("dummy".into()), "2.0.0", ToolSource::Unknown).unwrap(),
                ToolRequest::new(Arc::new("dummy".into()), "1.0.0", ToolSource::Unknown).unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains(indoc! {r#"
                dummy = [
                  "2.0.0", # first
                  "1.0.0", # second
                ]"#}),
            "comments should stay at their position rather than follow a version: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_array_count_change_still_writes() {
        // a different number of versions cannot reuse the old array's layout, so it falls back to
        // building a fresh one — the versions themselves still have to be written
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".array-count.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            dummy = ["1.0.0", "2.0.0"]
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let dummy = "dummy".into();
        cf.replace_versions(
            &dummy,
            vec![
                ToolRequest::new(Arc::new("dummy".into()), "1.0.1", ToolSource::Unknown).unwrap(),
                ToolRequest::new(Arc::new("dummy".into()), "2.0.1", ToolSource::Unknown).unwrap(),
                ToolRequest::new(Arc::new("dummy".into()), "3.0.0", ToolSource::Unknown).unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains(r#"dummy = ["1.0.1", "2.0.1", "3.0.0"]"#),
            "all versions should be written: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_array_element_comments_survive_alias_rename() {
        // an aliased entry is renamed to the short name, and the element comments have to come
        // along. Nothing in the reuse itself states why this works: the array is looked up under
        // the spelling the file actually uses, which the alias-aware key list supplies. Reading it
        // from the short name instead would drop these comments again while every other test here
        // still passed, so pin the combination.
        let _config = Config::get().await.unwrap();
        let p = CWD
            .as_ref()
            .unwrap()
            .join(".aliased-array-comments.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            nodejs = [
              "20.11.0", # first
              "22.0.0", # second
            ]
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let node: BackendArg = "node".into();
        cf.replace_versions(
            &node,
            vec![
                ToolRequest::new(Arc::new("node".into()), "20.11.1", ToolSource::Unknown).unwrap(),
                ToolRequest::new(Arc::new("node".into()), "22.1.0", ToolSource::Unknown).unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains(indoc! {r#"
                node = [
                  "20.11.1", # first
                  "22.1.0", # second
                ]"#}),
            "the renamed key should keep the comments between its elements: {dump}"
        );
        assert!(
            !dump.contains("nodejs"),
            "the alias must not be left behind as a second key: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_preserves_comments_on_qualified_key() {
        // a fully-qualified entry is rewritten to its short name, and the comments have to move
        // with it: https://github.com/jdx/mise/discussions/4797
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".qualified.mise.toml");
        let node: BackendArg = "node".into();
        let contents = formatdoc! {r#"
            [tools]
            # renovate: datasource=github-releases depName=node
            "{}" = "16.0.0" # keep me
            "#, node.full()};
        file::write(&p, contents).unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        cf.replace_versions(
            &node,
            vec![ToolRequest::new(Arc::new("node".into()), "18.0.0", ToolSource::Unknown).unwrap()],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains("# renovate: datasource=github-releases depName=node"),
            "comment above should survive the rename: {dump}"
        );
        assert!(
            dump.contains(r#"node = "18.0.0" # keep me"#),
            "comment after the version should survive the rename: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_renames_aliased_key() {
        // `nodejs` is an alias for `node`, so the file defines one tool and not two: the entry has
        // to be renamed in place rather than a second key added beside it, or the file ends up
        // with two keys for one tool and the later one silently wins when it is read back
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".aliased-key.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            # renovate: datasource=github-releases depName=node
            nodejs = "20.11.0" # keep me
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let node: BackendArg = "node".into();
        cf.replace_versions(
            &node,
            vec![
                ToolRequest::new(Arc::new("node".into()), "24.16.0", ToolSource::Unknown).unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains(r#"node = "24.16.0" # keep me"#),
            "the aliased key should be renamed and keep its comment: {dump}"
        );
        assert!(
            dump.contains("# renovate: datasource=github-releases depName=node"),
            "the comment above should survive the rename: {dump}"
        );
        assert!(
            !dump.contains("nodejs"),
            "the alias must not be left behind as a second key: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_renames_aliased_key_with_array() {
        // same for a multi-version entry: the array moves to the short name rather than being
        // written out a second time
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".aliased-array.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            nodejs = ["20.11.0", "22.0.0"] # keep me
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let node: BackendArg = "node".into();
        cf.replace_versions(
            &node,
            vec![
                ToolRequest::new(Arc::new("node".into()), "20.11.1", ToolSource::Unknown).unwrap(),
                ToolRequest::new(Arc::new("node".into()), "22.1.0", ToolSource::Unknown).unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains(r#"node = ["20.11.1", "22.1.0"] # keep me"#),
            "the array should move to the short name: {dump}"
        );
        assert!(
            !dump.contains("nodejs"),
            "the alias must not be left behind as a second key: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_collapses_duplicate_spellings() {
        // a file damaged by the old behavior has both keys; they deserialize to one entry and the
        // later one silently wins, so the write has to leave exactly one key behind. The first key
        // in the file is the entry the file appears to define, so it supplies the decorations.
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".dupe-spellings.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            nodejs = "20.11.0" # keep me
            node = "24.16.0"
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let node: BackendArg = "node".into();
        cf.replace_versions(
            &node,
            vec![
                ToolRequest::new(Arc::new("node".into()), "24.17.0", ToolSource::Unknown).unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains(r#"node = "24.17.0" # keep me"#),
            "the first key in the file supplies the decor: {dump}"
        );
        assert!(
            !dump.contains("nodejs"),
            "the duplicate spelling should be removed: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_keeps_registry_alias_key() {
        // `rg` is a registry alias for `ripgrep`, not one of the hardcoded backend aliases: the two
        // resolve to different short names, mise treats them as separate entries, and each is
        // written back under the name it was asked for
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".registry-alias.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            ripgrep = "14.1.0"
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let rg: BackendArg = "rg".into();
        cf.replace_versions(
            &rg,
            vec![ToolRequest::new(Arc::new("rg".into()), "14.1.1", ToolSource::Unknown).unwrap()],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains(r#"ripgrep = "14.1.0""#),
            "a registry alias is a different entry and must be left as written: {dump}"
        );
        assert!(
            dump.contains(r#"rg = "14.1.1""#),
            "the tool should be written under the name it was asked for: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_remove_tool_removes_aliased_key() {
        // `mise unuse nodejs` used to report success and prune the install while leaving the
        // `nodejs` key in the file, because it only ever looked for the short name
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".remove-aliased.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            dummy = "1.0.0"
            nodejs = "20.11.0"
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        cf.remove_tool(&"nodejs".into()).unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            !dump.contains("nodejs"),
            "the aliased key should be removed: {dump}"
        );
        assert!(
            dump.contains(r#"dummy = "1.0.0""#),
            "other tools should be left alone: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_remove_tool_removes_qualified_key() {
        // matching is done on the key as written, so this holds whatever the registry currently
        // reports as node's backend
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".remove-qualified.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            "core:node" = "20.11.0"
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        cf.remove_tool(&"node".into()).unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            !dump.contains("node"),
            "the qualified key should be removed: {dump}"
        );
        assert!(
            !dump.contains("[tools]"),
            "the now-empty tools table should be dropped: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_update_env_preserves_comments() {
        // https://github.com/jdx/mise/discussions/4797
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".env-comments.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [env]
            # keep this comment
            FOO = "bar" # keep me
            "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        cf.update_env("FOO", "baz").unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains("# keep this comment"),
            "comment above the variable should survive: {dump}"
        );
        assert!(
            dump.contains(r#"FOO = "baz" # keep me"#),
            "comment after the value should survive: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_set_alias_preserves_comments() {
        // https://github.com/jdx/mise/discussions/4797
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".alias-comments.mise.toml");
        let node: BackendArg = "node".into();
        let contents = formatdoc! {r#"
            [tool_alias.{node}.versions]
            # keep this comment
            lts = "20" # keep me
            "#};
        file::write(&p, contents).unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        cf.set_alias(&node, "lts", "22").unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains("# keep this comment"),
            "comment above the alias should survive: {dump}"
        );
        assert!(
            dump.contains(r#"lts = "22" # keep me"#),
            "comment after the alias should survive: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_set_shell_alias_preserves_comments() {
        // https://github.com/jdx/mise/discussions/4797
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".shell-alias.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [shell_alias]
            # keep this comment
            ll = "ls -l" # keep me
            "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        cf.set_shell_alias("ll", "ls -la").unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains("# keep this comment"),
            "comment above the shell alias should survive: {dump}"
        );
        assert!(
            dump.contains(r#"ll = "ls -la" # keep me"#),
            "comment after the shell alias should survive: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_update_bootstrap_package_preserves_comments() {
        // https://github.com/jdx/mise/discussions/4797
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".bootstrap-comments.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [bootstrap.packages]
            # keep this comment
            "apt:curl" = "8.5.0" # keep me
            "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        cf.update_bootstrap_package("apt:curl", "8.6.0").unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains("# keep this comment"),
            "comment above the package should survive: {dump}"
        );
        assert!(
            dump.contains(r#""apt:curl" = "8.6.0" # keep me"#),
            "comment after the package version should survive: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_update_bootstrap_brew_tap_preserves_comments() {
        // https://github.com/jdx/mise/discussions/4797
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".brew-tap-comments.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [bootstrap.brew.taps]
            # keep this comment
            "acme/tools" = "https://example.com/old.git" # keep me
            "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        cf.update_bootstrap_brew_tap("acme/tools", "https://example.com/new.git")
            .unwrap();

        let dump = cf.dump().unwrap();
        assert!(
            dump.contains("# keep this comment"),
            "comment above the tap should survive: {dump}"
        );
        assert!(
            dump.contains(r#""acme/tools" = "https://example.com/new.git" # keep me"#),
            "comment after the tap url should survive: {dump}"
        );
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_replace_versions_omits_empty_os() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".replace-empty-os.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            dummy = "1.0.0"
            "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let dummy = "dummy".into();
        let options = ToolVersionOptions {
            core: CoreToolOptions {
                os: Some(vec![]),
                ..Default::default()
            },
            ..Default::default()
        };

        cf.replace_versions(
            &dummy,
            vec![
                ToolRequest::new_opts(
                    Arc::new("dummy".into()),
                    "1.0.1",
                    options,
                    ToolSource::Unknown,
                )
                .unwrap(),
            ],
        )
        .unwrap();

        let dump = cf.dump().unwrap();
        assert!(dump.contains(r#"dummy = "1.0.1""#));
        assert!(!dump.contains("os"), "empty os should not be written back");
        file::remove_file(&p).unwrap();
    }

    #[tokio::test]
    async fn test_bootstrap_linux_systemd_units() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            r#"
        [bootstrap.linux.systemd.units.my-sync]
        description = "sync files"
        after = ["network-online.target"]
        wants = ["network-online.target"]
        exec_start = "~/.local/bin/my-sync --watch"
        type = "oneshot"
        remain_after_exit = true
        exec_stop = "~/.local/bin/my-sync --stop"
        timeout_start_sec = "120"
        timeout_stop_sec = "30"
        no_new_privileges = true
        private_tmp = true
        environment = { PATH = "/usr/bin:/bin" }
        working_directory = "~"
        restart = "on-failure"
        restart_sec = "5s"
        standard_output = "append:%h/.local/state/my-sync.log"
        wanted_by = ["default.target"]

        [bootstrap.linux.systemd.units.my-sync-timer]
        on_boot_sec = "2min"
        on_unit_active_sec = "10min"
        on_unit_inactive_sec = "5min"
        on_calendar = "hourly"
        randomized_delay_sec = "30s"
        accuracy_sec = "1s"
        persistent = true
        unit = "dev.mise.my-sync.service"
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let system = cf.bootstrap_config().unwrap();
        let unit = system.linux.systemd.units.get("my-sync").unwrap();
        assert_eq!(unit.description.as_deref(), Some("sync files"));
        assert_eq!(unit.after, vec!["network-online.target"]);
        assert_eq!(unit.wants, vec!["network-online.target"]);
        assert_eq!(
            unit.exec_start.as_deref(),
            Some("~/.local/bin/my-sync --watch")
        );
        assert_eq!(unit.service_type.as_deref(), Some("oneshot"));
        assert_eq!(unit.remain_after_exit, Some(true));
        assert_eq!(
            unit.exec_stop.as_deref(),
            Some("~/.local/bin/my-sync --stop")
        );
        assert_eq!(unit.timeout_start_sec.as_deref(), Some("120"));
        assert_eq!(unit.timeout_stop_sec.as_deref(), Some("30"));
        assert_eq!(unit.no_new_privileges, Some(true));
        assert_eq!(unit.private_tmp, Some(true));
        assert_eq!(
            unit.environment.get("PATH").map(String::as_str),
            Some("/usr/bin:/bin")
        );
        assert_eq!(unit.working_directory.as_deref(), Some("~"));
        assert_eq!(unit.restart.as_deref(), Some("on-failure"));
        assert_eq!(unit.restart_sec.as_deref(), Some("5s"));
        assert_eq!(
            unit.standard_output.as_deref(),
            Some("append:%h/.local/state/my-sync.log")
        );
        assert_eq!(
            unit.wanted_by.as_deref(),
            Some(["default.target".to_string()].as_slice())
        );
        let timer = system.linux.systemd.units.get("my-sync-timer").unwrap();
        assert_eq!(timer.on_boot_sec.as_deref(), Some("2min"));
        assert_eq!(timer.on_unit_active_sec.as_deref(), Some("10min"));
        assert_eq!(timer.on_unit_inactive_sec.as_deref(), Some("5min"));
        assert_eq!(timer.on_calendar.as_deref(), Some("hourly"));
        assert_eq!(timer.randomized_delay_sec.as_deref(), Some("30s"));
        assert_eq!(timer.accuracy_sec.as_deref(), Some("1s"));
        assert_eq!(timer.persistent, Some(true));
        assert_eq!(timer.unit.as_deref(), Some("dev.mise.my-sync.service"));
        file::remove_file(&p).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn test_bootstrap_linux_firewall() {
        let _config = Config::get().await.unwrap();
        let p = CWD.as_ref().unwrap().join(".test-firewall.mise.toml");
        file::write(
            &p,
            r#"
        [bootstrap.linux.firewall]
        backend = "nftables"
        state = "enabled"
        default_incoming = "deny"
        default_outgoing = "allow"
        exclusive = false

        [[bootstrap.linux.firewall.rules]]
        name = "https"
        port = 443
        protocol = "tcp"
        action = "allow"

        [[bootstrap.linux.firewall.rules]]
        name = "admin"
        port = "2200-2205"
        protocol = "tcp"
        source = "203.0.113.0/24"
        interface = "eth0"
        action = "allow"
        "#,
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let firewall = cf.bootstrap_config().unwrap().linux.firewall.unwrap();
        assert_eq!(
            firewall.backend,
            Some(crate::system::firewall::FirewallBackend::Nftables)
        );
        assert_eq!(firewall.rules.len(), 2);
        assert_eq!(firewall.rules[0].name, "https");
        assert!(matches!(
            firewall.rules[0].port,
            Some(crate::system::firewall::FirewallPortToml::Single(443))
        ));
        assert!(matches!(
            firewall.rules[1].port,
            Some(crate::system::firewall::FirewallPortToml::Range(ref range))
                if range == "2200-2205"
        ));
        assert_eq!(firewall.rules[1].source.as_deref(), Some("203.0.113.0/24"));
        assert_eq!(firewall.rules[1].interface.as_deref(), Some("eth0"));
        file::remove_file(&p).unwrap();
    }
}
