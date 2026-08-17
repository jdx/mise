use crate::cli::Cli;
use crate::duration;
use crate::file::FindUp;
use crate::platform::Platform;
use crate::{dirs, env, file};
#[allow(unused_imports)]
use confique::env::parse::{list_by_colon, list_by_comma};
use confique::{Config, Layer};
use eyre::{Result, bail, eyre};
use indexmap::{IndexMap, indexmap};
use itertools::Itertools;
use path_absolutize::Absolutize;
use serde::Serialize;
use serde::ser::Error;
use serde::{Deserialize, Deserializer, Serializer};
use std::env::consts::{ARCH, OS};
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock as Lazy;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use std::{
    collections::{BTreeSet, HashSet},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

use super::{TOML_CONFIG_FILENAMES, load_config_paths};
use url::Url;

// settings are generated from settings.toml in the project root
// make sure you run `mise run render` after updating settings.toml
include!(concat!(env!("OUT_DIR"), "/settings.rs"));

pub enum SettingsType {
    Bool,
    String,
    Integer,
    Duration,
    Path,
    Url,
    ListString,
    ListPath,
    SetString,
    IndexMap,
    BoolOrString,
}

#[derive(Clone, Copy)]
pub enum CompilePurpose {
    Install,
    Inspect,
}

pub struct SettingsMeta {
    // pub key: String,
    pub type_: SettingsType,
    pub description: &'static str,
    pub env: Option<&'static str>,
    pub deprecated: Option<&'static str>,
    pub deprecated_warn_at: Option<&'static str>,
    pub deprecated_remove_at: Option<&'static str>,
    pub global_only: bool,
    /// Consumed before config files are read, so a value in one can never apply.
    pub env_only: bool,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SettingsStatusMissingTools {
    /// never show the warning
    Never,
    /// hide this warning if the user hasn't installed at least 1 version of the tool before
    #[default]
    IfOtherVersionsInstalled,
    /// always show the warning if tools are missing
    Always,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum NpmPackageManager {
    #[default]
    Auto,
    Npm,
    Aube,
    AubeCli,
    Bun,
    Pnpm,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SystemDepsMode {
    /// prompt to install missing plugin system dependencies (falls back to `warn` non-interactively)
    #[default]
    Prompt,
    /// install missing plugin system dependencies without prompting
    Auto,
    /// print missing plugin system dependencies and continue
    Warn,
    /// skip the plugin system dependency check
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PythonUvVenvAuto {
    #[default]
    Off,
    Source,
    CreateSource,
    LegacyTrue,
}

impl PythonUvVenvAuto {
    pub fn should_source(self) -> bool {
        matches!(self, Self::Source | Self::CreateSource | Self::LegacyTrue)
    }

    pub fn should_create(self) -> bool {
        matches!(self, Self::CreateSource | Self::LegacyTrue)
    }

    pub fn is_legacy_true(self) -> bool {
        matches!(self, Self::LegacyTrue)
    }
}

impl<'de> Deserialize<'de> for PythonUvVenvAuto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct PythonUvVenvAutoVisitor;

        impl<'de> Visitor<'de> for PythonUvVenvAutoVisitor {
            type Value = PythonUvVenvAuto;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a boolean, \"source\", or \"create|source\"")
            }

            fn visit_bool<E>(self, value: bool) -> Result<PythonUvVenvAuto, E>
            where
                E: de::Error,
            {
                if value {
                    deprecated_at!(
                        "2026.7.0",
                        "2027.7.0",
                        "python.uv_venv_auto.true",
                        "python.uv_venv_auto=true is deprecated. Use python.uv_venv_auto=\"create|source\" or \"source\" instead."
                    );
                }
                Ok(if value {
                    PythonUvVenvAuto::LegacyTrue
                } else {
                    PythonUvVenvAuto::Off
                })
            }

            fn visit_str<E>(self, value: &str) -> Result<PythonUvVenvAuto, E>
            where
                E: de::Error,
            {
                let normalized = value.trim().to_ascii_lowercase();
                match normalized.as_str() {
                    "source" => Ok(PythonUvVenvAuto::Source),
                    "create|source" => Ok(PythonUvVenvAuto::CreateSource),
                    "true" | "yes" | "1" => self.visit_bool(true),
                    "false" | "no" | "0" => self.visit_bool(false),
                    _ => Err(E::invalid_value(de::Unexpected::Str(value), &self)),
                }
            }

            fn visit_string<E>(self, value: String) -> Result<PythonUvVenvAuto, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }
        }

        deserializer.deserialize_any(PythonUvVenvAutoVisitor)
    }
}

impl serde::Serialize for PythonUvVenvAuto {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            PythonUvVenvAuto::Off => serializer.serialize_bool(false),
            PythonUvVenvAuto::LegacyTrue => serializer.serialize_bool(true),
            PythonUvVenvAuto::Source => serializer.serialize_str("source"),
            PythonUvVenvAuto::CreateSource => serializer.serialize_str("create|source"),
        }
    }
}

pub type SettingsPartial = <Settings as Config>::Layer;

static BASE_SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);
/// Caches the resolved `safe` value from the most recent settings load so
/// `safe_mode()` answers correctly during the config parse pass that runs before
/// settings are (re)loaded — e.g. after `Config::reset()`. This captures `safe`
/// set via global config, which the `MISE_SAFE` env-var fallback cannot see.
/// 0 = false, 1 = true, 2 = never loaded (fall back to the env var).
static LAST_SAFE: AtomicU8 = AtomicU8::new(2);
static CLI_SETTINGS: Mutex<Option<SettingsPartial>> = Mutex::new(None);
static PENDING_DEPRECATED_SETTINGS: Lazy<Mutex<BTreeSet<&'static str>>> =
    Lazy::new(Default::default);
static DEPRECATED_WARNINGS_READY: AtomicBool = AtomicBool::new(false);
// TODO(2027.8.0): Remove the per-tool warning accessors once the NixOS
// `all_compile` deprecation process is complete.

fn default_all_compile(linux_distro: Option<&str>) -> bool {
    matches!(linux_distro, Some("alpine" | "nixos"))
}

fn compile_inherits_nixos_all_compile_default(
    linux_distro: Option<&str>,
    explicit_all_compile: Option<bool>,
    compile: Option<bool>,
) -> bool {
    linux_distro == Some("nixos") && explicit_all_compile.is_none() && compile.is_none()
}

fn effective_compile_setting(all_compile: bool, compile: Option<bool>) -> Option<bool> {
    compile.or_else(|| all_compile.then_some(true))
}

fn warn_nixos_all_compile_default_deprecated(
    tool: &str,
    id: &'static str,
    all_compile: Option<bool>,
    compile: Option<bool>,
) {
    if !cfg!(test)
        && compile_inherits_nixos_all_compile_default(
            env::LINUX_DISTRO.as_deref(),
            all_compile,
            compile,
        )
    {
        deprecated_at!(
            "2026.8.0",
            "2027.8.0",
            id,
            "The automatic all_compile=true default on NixOS caused {tool} to compile from source. Enable nix-ld to use precompiled binaries, or configure all_compile=true explicitly to keep compiling tools from source."
        );
    }
}

static DEFAULT_SETTINGS: Lazy<SettingsPartial> = Lazy::new(|| {
    let mut s = SettingsPartial::empty();
    s.python.default_packages_file = Some(env::HOME.join(".default-python-packages"));
    s
});

pub fn is_loaded() -> bool {
    BASE_SETTINGS.read().unwrap().is_some()
}

#[derive(Serialize, Deserialize)]
pub struct SettingsFile {
    #[serde(default)]
    pub settings: SettingsPartial,
}

fn parse_boolish_toml_value(value: &toml::Value) -> Option<bool> {
    match value {
        toml::Value::Boolean(value) => Some(*value),
        toml::Value::Integer(0) => Some(false),
        toml::Value::Integer(1) => Some(true),
        toml::Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" | "true" | "1" | "on" => Some(true),
            "n" | "no" | "false" | "0" | "off" => Some(false),
            _ => None,
        },
        toml::Value::Table(table) => table.get("value").and_then(parse_boolish_toml_value),
        _ => None,
    }
}

fn tera_v1_from_env_config(raw: &toml::Value) -> Option<bool> {
    raw.get("env")
        .and_then(toml::Value::as_table)
        .and_then(|env| env.get("MISE_TERA_V1"))
        .and_then(parse_boolish_toml_value)
}

fn deprecated_settings_in_toml_table(settings: &toml::Table) -> Vec<&'static str> {
    SETTINGS_META
        .iter()
        .filter_map(|(key, meta)| {
            meta.deprecated?;
            let value = nested_toml_value(settings, key)?;
            should_warn_deprecated_value(value).then_some(*key)
        })
        .collect()
}

fn deprecated_settings_in_env_directives(raw: &toml::Value) -> Vec<&'static str> {
    tera_v1_from_env_config(raw)
        .is_some()
        .then_some("tera_v1")
        .into_iter()
        .collect()
}

fn deprecated_settings_in_toml_config(raw: &toml::Value) -> Vec<&'static str> {
    let mut deprecated = Vec::new();
    if let Some(settings) = raw.get("settings").and_then(toml::Value::as_table) {
        deprecated.extend(deprecated_settings_in_toml_table(settings));
    }
    deprecated.extend(deprecated_settings_in_env_directives(raw));
    deprecated
}

fn warn_deprecated_env_settings() {
    for (key, meta) in SETTINGS_META.iter() {
        let Some(env_key) = meta.env else {
            continue;
        };
        if meta.deprecated.is_some()
            && env::var_os(env_key).is_some_and(|value| !value.as_os_str().is_empty())
        {
            warn_deprecated(key);
        }
    }
}

fn queue_deprecated(key: &'static str) {
    PENDING_DEPRECATED_SETTINGS.lock().unwrap().insert(key);
}

fn queue_deprecated_settings(keys: impl IntoIterator<Item = &'static str>) {
    for key in keys {
        warn_deprecated(key);
    }
}

fn nested_toml_value<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let mut parts = key.split('.').collect_vec();
    let last = parts.pop()?;
    let mut current = table;
    for part in parts {
        current = current.get(part)?.as_table()?;
    }
    current.get(last)
}

fn should_warn_deprecated_value(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(value) => !value.is_empty(),
        toml::Value::Array(value) => !value.is_empty(),
        toml::Value::Table(value) => {
            if value.get("unset").and_then(toml::Value::as_bool) == Some(true) {
                return false;
            }
            match value.get("value") {
                Some(value) => should_warn_deprecated_value(value),
                None => !value.is_empty(),
            }
        }
        _ => true,
    }
}

fn warn_deprecated(key: &'static str) {
    if !DEPRECATED_WARNINGS_READY.load(Ordering::SeqCst) {
        queue_deprecated(key);
        return;
    }
    warn_deprecated_now(key);
}

fn warn_deprecated_now(key: &'static str) {
    if let Some(meta) = SETTINGS_META.get(key)
        && let (Some(msg), Some(warn_at), Some(remove_at)) = (
            meta.deprecated,
            meta.deprecated_warn_at,
            meta.deprecated_remove_at,
        )
    {
        use versions::Versioning;
        let warn_version = Versioning::new(warn_at).unwrap();
        let remove_version = Versioning::new(remove_at).unwrap();
        debug_assert!(
            *crate::cli::version::V < remove_version,
            "Deprecated setting [{key}] should have been removed in {remove_at}. Please remove this deprecated setting.",
        );
        if *crate::cli::version::V >= warn_version {
            let id = Box::leak(format!("setting.{key}").into_boxed_str());
            if crate::output::DEPRECATED.lock().unwrap().insert(id) {
                warn!(
                    "deprecated [setting.{key}]: {msg} This will be removed in mise {remove_at}."
                );
            }
        }
    }
}

fn normalize_hidden_config_aliases(mut partial: SettingsPartial) -> SettingsPartial {
    if let Some(v) = partial.install_before.take() {
        warn_deprecated("install_before");
        if partial.minimum_release_age.is_none() {
            partial.minimum_release_age = Some(v);
        }
    }
    if let Some(v) = partial.task.cache_remote_mode.take() {
        warn_deprecated("task.cache_remote_mode");
        if partial.task.cache.remote_mode.is_none() {
            partial.task.cache.remote_mode = Some(v);
        }
    }
    if let Some(v) = partial.task.cache_remote_namespace.take() {
        warn_deprecated("task.cache_remote_namespace");
        if partial.task.cache.remote_namespace.is_none() {
            partial.task.cache.remote_namespace = Some(v);
        }
    }
    if let Some(v) = partial.task.cache_remote_oidc_audience.take() {
        warn_deprecated("task.cache_remote_oidc_audience");
        if partial.task.cache.remote_oidc_audience.is_none() {
            partial.task.cache.remote_oidc_audience = Some(v);
        }
    }
    if let Some(v) = partial.task.cache_remote_token.take() {
        warn_deprecated("task.cache_remote_token");
        if partial.task.cache.remote_token.is_none() {
            partial.task.cache.remote_token = Some(v);
        }
    }
    if let Some(v) = partial.task.cache_remote_token_file.take() {
        warn_deprecated("task.cache_remote_token_file");
        if partial.task.cache.remote_token_file.is_none() {
            partial.task.cache.remote_token_file = Some(v);
        }
    }
    if let Some(v) = partial.task.cache_remote_url.take() {
        warn_deprecated("task.cache_remote_url");
        if partial.task.cache.remote_url.is_none() {
            partial.task.cache.remote_url = Some(v);
        }
    }
    partial
}

fn strip_local_only_settings(settings: &mut toml::Table, path: &Path, is_global: bool) {
    if is_global {
        return;
    }

    for key in SETTINGS_META
        .iter()
        .filter_map(|(key, meta)| meta.global_only.then_some(*key))
    {
        if let Some(value) = remove_nested_toml_value(settings, key)
            && should_warn_ignored_global_only_value(&value)
        {
            warn!(
                "{key} in non-global config {} is ignored for security reasons",
                path.display()
            );
        }
    }
}

/// Drop settings the config loader consumes *before* any config file is read — the config
/// filenames and paths. A value written here can never take effect, and leaving it in the
/// partial makes `mise settings get` report it as if it were live, which is what
/// <https://github.com/jdx/mise/discussions/5791> ran into. Unlike the global-only strip this
/// applies to every config, global included: the ordering problem is the same either way.
fn strip_env_only_settings(settings: &mut toml::Table, path: &Path) {
    for (key, meta) in SETTINGS_META.iter().filter(|(_, meta)| meta.env_only) {
        if remove_nested_toml_value(settings, key).is_some() {
            warn!(
                "{key} in {} is ignored: mise reads it before config files load. Set {} instead.",
                file::display_path(path),
                meta.env.unwrap_or("the matching MISE_* variable")
            );
        }
    }
}

fn remove_nested_toml_value(table: &mut toml::Table, key: &str) -> Option<toml::Value> {
    let mut parts = key.split('.').collect_vec();
    let last = parts.pop()?;
    let mut current = table;
    for part in parts {
        current = current.get_mut(part)?.as_table_mut()?;
    }
    current.remove(last)
}

fn should_warn_ignored_global_only_value(value: &toml::Value) -> bool {
    !matches!(value, toml::Value::String(s) if s.is_empty())
}

/// `aqua.registries` entries must be URLs — `github_repo_slug` requires a
/// parseable `https` URL, and anything else fails later with `relative URL
/// without a base`. A value that is not a URL is therefore dead today, so
/// reading it as a path relative to the config file that declared it is a pure
/// widening: it lets a registry committed alongside the project be referenced
/// portably, without changing any config that works now. See discussion #4306.
///
/// Returns `None` when the value should be left as written.
fn resolve_registry_source(value: &str, config_root: &Path) -> Option<String> {
    let looks_like_path = match Url::parse(value) {
        Err(_) => true,
        // A Windows path such as `C:\registry.yaml` parses as a URL whose scheme
        // is the drive letter. No real URL scheme is a single character.
        Ok(url) => url.scheme().len() == 1,
    };
    if !looks_like_path {
        return None;
    }
    // Joining an absolute path returns it unchanged, so absolute entries are
    // normalized rather than rebased onto the config root.
    let joined = config_root.join(value);
    let absolute = joined
        .absolutize()
        .map(|p| p.into_owned())
        .unwrap_or(joined);
    // Leave the value alone if it cannot be expressed as a file URL rather than
    // substituting something the user did not write.
    Url::from_file_path(&absolute).ok().map(String::from)
}

/// Rewrite relative `aqua.registries` entries so they resolve against the config
/// root of the file that declared them. Both `aqua.registries = [..]` under
/// `[settings]` and `[settings.aqua]` + `registries = [..]` parse to the same
/// nested table, so navigating the table covers either spelling.
fn resolve_aqua_registry_paths(settings: &mut toml::Table, path: &Path) {
    let Some(registries) = settings
        .get_mut("aqua")
        .and_then(toml::Value::as_table_mut)
        .and_then(|aqua| aqua.get_mut("registries"))
        .and_then(toml::Value::as_array_mut)
    else {
        return;
    };
    let config_root = crate::config::config_file::config_root::config_root(path);
    for entry in registries.iter_mut() {
        if let Some(value) = entry.as_str()
            && let Some(resolved) = resolve_registry_source(value, &config_root)
        {
            *entry = toml::Value::String(resolved);
        }
    }
}

/// Resolve age identity paths while the settings file that declared them is
/// still known. Once settings layers are merged, a relative `PathBuf` no
/// longer carries enough information to distinguish two config roots.
fn resolve_age_paths(settings: &mut toml::Table, path: &Path) -> Result<()> {
    let Some(age) = settings.get_mut("age").and_then(toml::Value::as_table_mut) else {
        return Ok(());
    };
    let config_root = crate::config::config_file::config_root::config_root(path);
    let mut tera = crate::tera::get_miserc_tera();
    let mut context = tera::Context::new();
    context.insert("env", &*env::PRISTINE_ENV);
    context.insert("config_root", &config_root);
    if let Ok(cwd) = std::env::current_dir() {
        context.insert("cwd", &cwd);
    }
    context.insert("xdg_cache_home", &*env::XDG_CACHE_HOME);
    context.insert("xdg_config_home", &*env::XDG_CONFIG_HOME);
    context.insert("xdg_data_home", &*env::XDG_DATA_HOME);
    context.insert("xdg_state_home", &*env::XDG_STATE_HOME);

    let mut resolve = |setting: &str, target: &mut toml::Value| -> Result<()> {
        let Some(value) = target.as_str() else {
            return Ok(());
        };
        let rendered = if crate::tera::contains_template_syntax(value) {
            crate::tera::render_str(&mut tera, value, &context).map_err(|err| {
                eyre!(
                    "failed to render settings.age.{setting} in {}: {err}",
                    file::display_path(path)
                )
            })?
        } else {
            value.to_string()
        };
        let rendered_path = Path::new(&rendered);
        let resolved = if rendered_path.is_absolute()
            || rendered_path == Path::new("~")
            || rendered_path.starts_with("~/")
        {
            rendered_path.to_path_buf()
        } else {
            let joined = config_root.join(rendered_path);
            joined
                .absolutize()
                .map(|p| p.into_owned())
                .unwrap_or(joined)
        };
        *target = toml::Value::String(resolved.to_string_lossy().into_owned());
        Ok(())
    };

    if let Some(key_file) = age.get_mut("key_file") {
        resolve("key_file", key_file)?;
    }
    for setting in ["identity_files", "ssh_identity_files"] {
        if let Some(values) = age.get_mut(setting).and_then(toml::Value::as_array_mut) {
            for value in values {
                resolve(setting, value)?;
            }
        }
    }
    Ok(())
}

impl Settings {
    const UNIX_DEFAULT_FILE_SHELL_ARGS: &'static str = "sh";
    const UNIX_DEFAULT_INLINE_SHELL_ARGS: &'static str = "sh -c -o errexit";
    const WINDOWS_DEFAULT_FILE_SHELL_ARGS: &'static str = "cmd /c";
    const WINDOWS_DEFAULT_INLINE_SHELL_ARGS: &'static str = "cmd /c";

    pub fn parse_default_package_line(package: &str) -> Option<String> {
        let package = package.split('#').next().unwrap_or_default().trim();
        (!package.is_empty()).then(|| package.to_string())
    }

    pub fn warn_default_package_file_deprecated(id: &'static str, package_type: &str) {
        if SETTINGS_META
            .get(id)
            .is_some_and(|m| m.deprecated.is_some())
        {
            warn_deprecated(id);
            return;
        }
        deprecated_at!(
            "2026.11.0",
            "2027.11.0",
            id,
            "Default {package_type} files are deprecated. Use tool-level postinstall hooks for packages that should be installed into every runtime version, or use package manager backends such as npm:, pipx:, gem:, or go: for CLI tools."
        );
    }

    pub fn get() -> Arc<Self> {
        Self::try_get().unwrap()
    }

    pub fn all_compile(&self) -> bool {
        self.all_compile.unwrap_or_else(|| {
            !cfg!(test)
                && default_all_compile(env::LINUX_DISTRO.as_ref().map(|distro| distro.as_str()))
        })
    }

    fn compile_setting(
        &self,
        purpose: CompilePurpose,
        tool: &str,
        id: &'static str,
        compile: Option<bool>,
    ) -> Option<bool> {
        if matches!(purpose, CompilePurpose::Install) {
            warn_nixos_all_compile_default_deprecated(tool, id, self.all_compile, compile);
        }
        effective_compile_setting(self.all_compile(), compile)
    }

    pub fn node_compile(&self, purpose: CompilePurpose) -> Option<bool> {
        self.compile_setting(
            purpose,
            "node",
            "nixos.node_all_compile_default",
            self.node.compile,
        )
    }

    pub fn python_compile(&self, purpose: CompilePurpose) -> Option<bool> {
        self.compile_setting(
            purpose,
            "python",
            "nixos.python_all_compile_default",
            self.python.compile,
        )
    }

    pub fn erlang_compile(&self, purpose: CompilePurpose) -> Option<bool> {
        self.compile_setting(
            purpose,
            "erlang",
            "nixos.erlang_all_compile_default",
            self.erlang.compile,
        )
    }

    #[cfg(not(windows))]
    pub fn ruby_compile(&self, purpose: CompilePurpose) -> Option<bool> {
        self.compile_setting(
            purpose,
            "ruby",
            "nixos.ruby_all_compile_default",
            self.ruby.compile,
        )
    }

    pub fn try_get() -> Result<Arc<Self>> {
        if let Some(settings) = BASE_SETTINGS.read().unwrap().as_ref() {
            return Ok(settings.clone());
        }
        time!("try_get");

        // Initial pass to obtain cd option
        let mut sb = Self::builder()
            .preloaded(normalize_hidden_config_aliases(
                CLI_SETTINGS.lock().unwrap().clone().unwrap_or_default(),
            ))
            .env();
        time!("try_get builder1+env");

        let mut settings = sb.load()?;
        time!("try_get load1");
        if let Some(mut cd) = settings.cd {
            static ORIG_PATH: Lazy<std::io::Result<PathBuf>> = Lazy::new(env::current_dir);
            if cd.is_relative() {
                cd = ORIG_PATH.as_ref()?.join(cd);
            }
            env::set_current_dir(cd)?;
        }

        // Reload settings after current directory option processed
        sb = Self::builder()
            .preloaded(normalize_hidden_config_aliases(
                CLI_SETTINGS.lock().unwrap().clone().unwrap_or_default(),
            ))
            .env();
        time!("try_get builder2+env");
        for file in Self::all_settings_files() {
            sb = sb.preloaded(file);
        }
        time!("try_get all_settings_files");
        sb = sb.preloaded(DEFAULT_SETTINGS.clone());
        time!("try_get default_settings");

        settings = sb.load()?;
        time!("try_get load2");
        if !settings.legacy_version_file {
            settings.idiomatic_version_file = Some(false);
        }
        if settings.raw {
            settings.jobs = 1;
        } else {
            settings.jobs = crate::jobs::normalize(settings.jobs);
        }
        // Handle NO_COLOR environment variable
        if *env::NO_COLOR {
            settings.color = false;
        }
        if settings.debug {
            settings.log_level = "debug".to_string();
        }
        if settings.trace {
            settings.log_level = "trace".to_string();
        }
        if settings.quiet {
            settings.log_level = "error".to_string();
        }
        if settings.log_level == "trace" || settings.log_level == "debug" {
            settings.verbose = true;
            settings.debug = true;
            if settings.log_level == "trace" {
                settings.trace = true;
            }
        }
        // handle the special case of `mise -v` which should show version, not set verbose.
        // Use the args mise was invoked with (already captured safely in Cli::run and kept
        // in sync with internal re-dispatch like `mise asdf ...`) rather than re-reading the
        // process argv. See also Settings::no_config().
        let is_version_flag = {
            let args = env::ARGS.read().unwrap();
            args.len() == 2 && args[1] == "-v"
        };
        if settings.verbose && !is_version_flag {
            settings.quiet = false;
            if settings.log_level != "trace" {
                settings.log_level = "debug".to_string();
            }
        }
        if !settings.color {
            console::set_colors_enabled(false);
            console::set_colors_enabled_stderr(false);
        } else if *env::CLICOLOR_FORCE == Some(true) {
            console::set_colors_enabled(true);
            console::set_colors_enabled_stderr(true);
        } else if *env::CLICOLOR == Some(false) {
            console::set_colors_enabled(false);
            console::set_colors_enabled_stderr(false);
        } else if ci_info::is_ci() && !cfg!(test) {
            console::set_colors_enabled_stderr(true);
        }
        if settings.ci {
            settings.yes = true;
        }
        if settings.gpg_verify.is_some() {
            settings.node.gpg_verify = settings.node.gpg_verify.or(settings.gpg_verify);
            settings.swift.gpg_verify = settings.swift.gpg_verify.or(settings.gpg_verify);
        }
        settings.set_hidden_configs();
        if cfg!(test) {
            settings.experimental = true;
        }
        trace!("Settings: {:#?}", redacted_settings_for_debug(&settings));
        let settings = Arc::new(settings);
        LAST_SAFE.store(u8::from(settings.safe), Ordering::Relaxed);
        *BASE_SETTINGS.write().unwrap() = Some(settings.clone());
        time!("try_get done");
        Ok(settings)
    }

    pub fn flush_deprecated_warnings() {
        if CLI_SETTINGS.lock().unwrap().is_none() {
            return;
        }
        Self::flush_deprecated_warnings_now();
    }

    pub fn flush_deprecated_warnings_for_fast_exit() {
        Self::flush_deprecated_warnings_now();
    }

    fn flush_deprecated_warnings_now() {
        DEPRECATED_WARNINGS_READY.store(true, Ordering::SeqCst);
        warn_deprecated_env_settings();
        let pending = {
            let mut pending = PENDING_DEPRECATED_SETTINGS.lock().unwrap();
            std::mem::take(&mut *pending)
        };
        for key in pending {
            warn_deprecated_now(key);
        }
    }

    /// Sets deprecated settings to new names
    fn set_hidden_configs(&mut self) {
        if let Some(v) = self.install_before.take() {
            warn_deprecated("install_before");
            if self.minimum_release_age.is_none() {
                self.minimum_release_age = Some(v);
            }
        }
        // Migrate task_* settings to task.* (must run before auto_install override below)
        if let Some(v) = self.task_disable_paths.take()
            && !v.is_empty()
        {
            warn_deprecated("task_disable_paths");
            self.task.disable_paths.extend(v);
        }
        if let Some(v) = self.task_output.take() {
            warn_deprecated("task_output");
            self.task.output = Some(v);
        }
        if let Some(v) = self.task_remote_no_cache {
            warn_deprecated("task_remote_no_cache");
            self.task.remote_no_cache = Some(v);
        }
        if let Some(v) = self.task_run_auto_install {
            warn_deprecated("task_run_auto_install");
            self.task.run_auto_install = v;
        }
        if let Some(v) = self.task_show_full_cmd {
            warn_deprecated("task_show_full_cmd");
            self.task.show_full_cmd = v;
        }
        if let Some(v) = self.task_skip.take()
            && !v.is_empty()
        {
            warn_deprecated("task_skip");
            self.task.skip.extend(v);
        }
        if let Some(v) = self.task_skip_depends {
            warn_deprecated("task_skip_depends");
            self.task.skip_depends = v;
        }
        if let Some(v) = self.task_timeout.take() {
            warn_deprecated("task_timeout");
            self.task.timeout = Some(v);
        }
        if let Some(v) = self.task_timings {
            warn_deprecated("task_timings");
            self.task.timings = Some(v);
        }
        if !self.auto_install {
            self.exec_auto_install = false;
            self.not_found_auto_install = false;
            self.task.run_auto_install = false;
        }
        if let Some(go_default_packages_file) = &self.go_default_packages_file {
            self.go.default_packages_file = go_default_packages_file.clone();
        }
        if let Some(go_download_mirror) = &self.go_download_mirror {
            self.go.download_mirror = go_download_mirror.clone();
        }
        if let Some(go_repo) = &self.go_repo {
            self.go.repo = go_repo.clone();
        }
        if let Some(go_set_gobin) = self.go_set_gobin {
            self.go.set_gobin = Some(go_set_gobin);
        }
        if let Some(go_set_gopath) = self.go_set_gopath {
            self.go.set_gopath = go_set_gopath;
        }
        if let Some(go_set_goroot) = self.go_set_goroot {
            self.go.set_goroot = go_set_goroot;
        }
        if let Some(go_skip_checksum) = self.go_skip_checksum {
            self.go.skip_checksum = go_skip_checksum;
        }
        if self.npm.bun {
            self.npm.package_manager = NpmPackageManager::Bun;
        }
        if self.shorthands_file.is_some() {
            warn_deprecated("shorthands_file");
        }
        if let Some(registry_url) = self.aqua.registry_url.take() {
            warn_deprecated("aqua.registry_url");
            if self.aqua.registries.is_none() {
                self.aqua.registries = Some(vec![registry_url]);
            }
        }
    }

    pub fn add_cli_matches(cli: &Cli) {
        let mut s = SettingsPartial::empty();

        // Don't process mise-specific flags when running as a shim
        if *crate::env::IS_RUNNING_AS_SHIM {
            Self::reset(Some(s));
            return;
        }

        if cli.raw {
            s.raw = Some(true);
        }
        if cli.locked {
            s.locked = Some(true);
        }
        if let Some(cd) = &cli.cd {
            s.cd = Some(cd.clone());
        }
        if cli.profile.is_some() {
            s.env = cli.profile.clone();
        }
        if cli.env.is_some() {
            s.env = cli.env.clone();
        }
        if cli.yes {
            s.yes = Some(true);
        }
        if cli.quiet || cli.silent {
            s.quiet = Some(true);
        }
        if cli.silent {
            s.silent = Some(true);
        }
        if cli.trace {
            s.log_level = Some("trace".to_string());
        }
        if cli.debug {
            s.log_level = Some("debug".to_string());
        }
        if let Some(log_level) = &cli.log_level {
            s.log_level = Some(log_level.to_string());
        }
        if cli.verbose > 0 {
            s.verbose = Some(true);
        }
        if cli.verbose > 1 {
            s.log_level = Some("trace".to_string());
        }
        Self::reset(Some(s));
    }

    pub fn parse_settings_file(path: &Path) -> Result<SettingsPartial> {
        let raw = file::read_to_string(path)?;
        let mut raw: toml::Value = toml::from_str(&raw)?;
        let tera_v1_from_env = tera_v1_from_env_config(&raw);
        if let Some(settings) = raw.get_mut("settings").and_then(toml::Value::as_table_mut) {
            strip_local_only_settings(settings, path, crate::config::is_global_config(path));
            strip_env_only_settings(settings, path);
            // After the strips, so a setting that will not survive them is
            // never rewritten.
            resolve_aqua_registry_paths(settings, path);
            resolve_age_paths(settings, path)?;
        }
        let deprecated = deprecated_settings_in_toml_config(&raw);
        let settings_file: SettingsFile = raw.try_into()?;
        queue_deprecated_settings(deprecated);
        let mut settings = normalize_hidden_config_aliases(settings_file.settings);
        if settings.tera_v1.is_none() {
            settings.tera_v1 = tera_v1_from_env;
        }
        Ok(settings)
    }

    fn all_settings_files() -> Vec<SettingsPartial> {
        // In safe mode, ignore `[settings]` from project (non-global) config so
        // an untrusted repo cannot change mise's behavior during resolution
        // (e.g. disable verification, redirect a backend/registry). Global and
        // system config is operator-owned and still applies. A specific setting
        // could be allowlisted here later if it is safe and necessary.
        let safe_mode = Settings::safe_mode();
        load_config_paths(&TOML_CONFIG_FILENAMES, false)
            .into_iter()
            .filter(|p| !safe_mode || crate::config::is_global_config(p))
            .map(|p| Self::parse_settings_file(&p))
            .filter_map(|cfg| match cfg {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    eprintln!("Error loading settings file: {e}");
                    None
                }
            })
            .collect()
    }

    pub fn hidden_configs() -> &'static HashSet<&'static str> {
        static HIDDEN_CONFIGS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
            [
                "ci",
                "cd",
                "debug",
                "env_file",
                "install_before",
                "trace",
                "log_level",
            ]
            .into()
        });
        &HIDDEN_CONFIGS
    }

    pub fn reset(cli_settings: Option<SettingsPartial>) {
        *CLI_SETTINGS.lock().unwrap() = cli_settings;
        *BASE_SETTINGS.write().unwrap() = None;
        // Clear caches that depend on settings and environment
        crate::config::config_file::config_root::reset();
    }

    /// Invalidate settings loaded from config files without discarding CLI overrides.
    pub fn reload() {
        *BASE_SETTINGS.write().unwrap() = None;
        crate::config::config_file::config_root::reset();
    }

    /// Merge an override into the CLI-level settings partial.
    ///
    /// `reset` replaces CLI_SETTINGS wholesale, which would clobber overrides
    /// installed earlier in startup (`--offline`, `--quiet`, etc.). This
    /// helper merges in-place so a subcommand flag (e.g. `mise ls-remote
    /// --prerelease`) can layer on top of those without losing them. Clears
    /// BASE_SETTINGS so the next `Settings::get()` rebuilds with the override
    /// applied.
    pub fn override_with(updater: impl FnOnce(&mut SettingsPartial)) {
        let mut lock = CLI_SETTINGS.lock().unwrap();
        let partial = lock.get_or_insert_with(SettingsPartial::empty);
        updater(partial);
        drop(lock);
        *BASE_SETTINGS.write().unwrap() = None;
    }

    pub fn lockfile_enabled(&self) -> bool {
        self.lockfile.unwrap_or(true)
    }

    pub fn lockfile_creation_enabled(&self) -> bool {
        self.lockfile == Some(true)
    }

    /// Returns configured lockfile platforms parsed into Platform structs, or None for defaults.
    /// Errors on invalid platform strings (same validation as `mise lock --platform`).
    pub fn lockfile_platforms(&self) -> Result<Option<Vec<Platform>>> {
        match &self.lockfile_platforms {
            Some(platforms) if !platforms.is_empty() => {
                Ok(Some(Platform::parse_multiple(platforms)?))
            }
            _ => Ok(None),
        }
    }

    pub fn force_provenance_verify(&self) -> bool {
        self.locked_verify_provenance || self.paranoid
    }

    pub fn ensure_experimental(&self, what: &str) -> Result<()> {
        if !self.experimental {
            bail!("{what} is experimental. Enable it with `mise settings experimental=true`");
        }
        Ok(())
    }

    pub fn trusted_config_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.trusted_config_paths
            .iter()
            .filter(|p| !p.to_string_lossy().is_empty())
            .map(file::replace_path)
            .filter_map(|p| file::canonicalize_cached(&p))
    }

    pub fn global_tools_file(&self) -> PathBuf {
        env::var_path("MISE_GLOBAL_CONFIG_FILE")
            .or_else(|| env::var_path("MISE_CONFIG_FILE"))
            .unwrap_or_else(|| {
                if self.asdf_compat {
                    env::HOME.join(&*env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME)
                } else {
                    dirs::CONFIG.join("config.toml")
                }
            })
    }

    pub fn env_files(&self) -> Vec<PathBuf> {
        let mut files = vec![];
        if let Some(cwd) = &*dirs::CWD
            && let Some(env_file) = &self.env_file
        {
            let env_file = env_file.to_string_lossy().to_string();
            for p in FindUp::new(cwd, &[env_file]) {
                files.push(p);
            }
        }
        files.into_iter().rev().collect()
    }

    pub fn as_dict(&self) -> eyre::Result<toml::Table> {
        let s = toml::to_string(self)?;
        let mut table: toml::Table = toml::from_str(&s)?;
        table.insert(
            "all_compile".to_string(),
            toml::Value::Boolean(self.all_compile()),
        );
        redact_settings_table(&mut table);
        Ok(table)
    }

    pub fn cache_prune_age_duration(&self) -> Option<Duration> {
        let age = duration::parse_duration(&self.cache_prune_age).unwrap();
        if age.as_secs() == 0 { None } else { Some(age) }
    }

    pub fn fetch_remote_versions_timeout(&self) -> Duration {
        let timeout = self.configured_fetch_remote_versions_timeout();
        if self.bound_remote_version_lookups() {
            timeout.min(Duration::from_secs(3))
        } else {
            timeout
        }
    }

    pub fn configured_fetch_remote_versions_timeout(&self) -> Duration {
        duration::parse_duration(&self.fetch_remote_versions_timeout).unwrap()
    }

    /// Whether remote-version lookups should use the aggressive fast-path budget
    /// (a single ~3s attempt with no retries). This is on under `prefer_offline`
    /// so shims and shell activation never stall — but NOT for commands whose
    /// whole job is to enumerate remote versions/tags (`mise lock`, `ls-remote`,
    /// `outdated`, `upgrade`), which must honor the full configured
    /// `fetch_remote_versions_timeout` and retry budget even when
    /// `prefer_offline` is set.
    ///
    /// See <https://github.com/jdx/mise/discussions/11185>.
    pub fn bound_remote_version_lookups(&self) -> bool {
        self.prefer_offline() && !env::REMOTE_FETCH_COMMAND.load(Ordering::Relaxed)
    }

    /// duration that remote version cache is kept for
    /// for "fast" commands (represented by PREFER_OFFLINE), these are always
    /// cached. For "slow" commands like `mise ls-remote` or `mise install`:
    /// - if MISE_FETCH_REMOTE_VERSIONS_CACHE is set, use that
    /// - if MISE_FETCH_REMOTE_VERSIONS_CACHE is not set, use HOURLY
    pub fn fetch_remote_versions_cache(&self) -> Option<Duration> {
        if self.prefer_offline() {
            None
        } else {
            Some(duration::parse_duration(&self.fetch_remote_versions_cache).unwrap())
        }
    }

    pub fn http_timeout(&self) -> Duration {
        duration::parse_duration(&self.http_timeout).unwrap()
    }

    pub fn http_download_timeout(&self) -> Duration {
        duration::parse_duration(&self.http_download_timeout).unwrap()
    }

    /// Fast-path commands should make at most one network attempt before falling
    /// back to cached/local behavior. In particular, shims must not multiply a
    /// stalled resolver timeout by the configured retry count.
    pub fn http_retries(&self) -> i64 {
        if self.bound_remote_version_lookups() {
            0
        } else {
            self.http_retries
        }
    }

    /// Returns true if offline mode is enabled via setting or CLI flag/env var.
    pub fn offline(&self) -> bool {
        self.offline || *env::OFFLINE
    }

    /// Returns true if prefer-offline mode is enabled via setting, env var, or
    /// because the current command is a "fast" command (hook-env, activate, etc.).
    /// Also returns true if offline mode is enabled (offline implies prefer-offline).
    pub fn prefer_offline(&self) -> bool {
        self.offline() || self.prefer_offline || env::PREFER_OFFLINE.load(Ordering::Relaxed)
    }

    pub fn env_cache_ttl(&self) -> Duration {
        duration::parse_duration(&self.env_cache_ttl).unwrap()
    }

    pub fn aqua_registry_cache_ttl(&self) -> Duration {
        self.aqua
            .registry_cache_ttl
            .as_deref()
            .map(duration::parse_duration)
            .transpose()
            .unwrap()
            .unwrap_or(crate::aqua::aqua_registry_wrapper::DEFAULT_AQUA_REGISTRY_CACHE_TTL)
    }

    pub fn registry_cache_ttl(&self) -> Duration {
        self.registry_cache_ttl
            .as_deref()
            .map(duration::parse_duration)
            .transpose()
            .unwrap()
            .unwrap_or(duration::HOURLY)
    }

    pub fn task_timeout_duration(&self) -> Option<Duration> {
        self.task
            .timeout
            .as_ref()
            .and_then(|s| duration::parse_duration(s).ok())
    }

    pub fn log_level(&self) -> log::LevelFilter {
        self.log_level.parse().unwrap_or(log::LevelFilter::Info)
    }

    pub fn disable_tools(&self) -> BTreeSet<String> {
        normalize_tool_names(&self.disable_tools)
    }

    pub fn enable_tools(&self) -> Option<BTreeSet<String>> {
        self.enable_tools.as_ref().map(normalize_tool_names)
    }

    pub fn partial_as_dict(partial: &SettingsPartial) -> eyre::Result<toml::Table> {
        let s = toml::to_string(partial)?;
        let mut table = toml::from_str(&s)?;
        remove_empty_nested_settings(&mut table, "");
        redact_settings_table(&mut table);
        Ok(table)
    }

    pub fn default_inline_shell(&self) -> Result<Vec<String>> {
        let (sa, fallback) = if cfg!(windows) {
            (
                &self.windows_default_inline_shell_args,
                Self::WINDOWS_DEFAULT_INLINE_SHELL_ARGS,
            )
        } else {
            (
                &self.unix_default_inline_shell_args,
                Self::UNIX_DEFAULT_INLINE_SHELL_ARGS,
            )
        };
        let mut shell = split_default_shell_or_fallback(sa, fallback)?;
        self.maybe_no_profile(&mut shell);
        Ok(shell)
    }

    pub fn default_file_shell(&self) -> Result<Vec<String>> {
        let (sa, fallback) = if cfg!(windows) {
            (
                &self.windows_default_file_shell_args,
                Self::WINDOWS_DEFAULT_FILE_SHELL_ARGS,
            )
        } else {
            (
                &self.unix_default_file_shell_args,
                Self::UNIX_DEFAULT_FILE_SHELL_ARGS,
            )
        };
        let mut shell = split_default_shell_or_fallback(sa, fallback)?;
        self.maybe_no_profile(&mut shell);
        Ok(shell)
    }

    /// Inject `-NoProfile` into a PowerShell shell command when
    /// `windows_powershell_no_profile` is enabled. No-op for other shells.
    pub fn maybe_no_profile(&self, shell: &mut Vec<String>) {
        if self.windows_powershell_no_profile {
            crate::path::inject_powershell_no_profile(shell);
        }
    }

    pub fn os(&self) -> &str {
        match self.os.as_deref().unwrap_or(OS) {
            "darwin" | "macos" => "macos",
            "linux" => "linux",
            "windows" => "windows",
            other => other,
        }
    }

    pub fn arch(&self) -> &str {
        match self.arch.as_deref().unwrap_or(ARCH) {
            "x86_64" | "amd64" => "x64",
            "aarch64" | "arm64" => "arm64",
            other => other,
        }
    }

    pub fn libc(&self) -> Option<&str> {
        match self.libc.as_deref()?.to_ascii_lowercase().as_str() {
            "glibc" | "gnu" => Some("gnu"),
            "musl" => Some("musl"),
            _ => None,
        }
    }

    pub fn no_config() -> bool {
        *env::MISE_NO_CONFIG
            || !*crate::env::IS_RUNNING_AS_SHIM
                && env::ARGS
                    .read()
                    .unwrap()
                    .iter()
                    .take_while(|a| *a != "--")
                    .any(|a| a == "--no-config")
    }

    pub fn no_env() -> bool {
        *env::MISE_NO_ENV
            || !*crate::env::IS_RUNNING_AS_SHIM
                && env::ARGS
                    .read()
                    .unwrap()
                    .iter()
                    .take_while(|a| *a != "--")
                    .any(|a| a == "--no-env")
    }

    pub fn no_hooks() -> bool {
        *env::MISE_NO_HOOKS
            || !*crate::env::IS_RUNNING_AS_SHIM
                && env::ARGS
                    .read()
                    .unwrap()
                    .iter()
                    .take_while(|a| *a != "--")
                    .any(|a| a == "--no-hooks")
    }

    /// Whether safe mode (`MISE_SAFE=1` or the `safe` setting) is active.
    ///
    /// Safe to call during the config parse pass: it reads the loaded setting
    /// when settings are available, otherwise falls back to the `MISE_SAFE`
    /// environment variable. This avoids triggering a recursive settings load
    /// from `trust_check` (which runs while config files are being parsed,
    /// before settings are loaded, e.g. after `Config::reset`). `safe` is
    /// global-only, so it can only come from the environment or global config;
    /// the env fallback covers the common `MISE_SAFE=1` case in that window.
    pub fn safe_mode() -> bool {
        if is_loaded() {
            return Settings::get().safe;
        }
        // Settings not loaded (e.g. the config parse pass after Config::reset).
        // Use the value cached from the last full load, which captures `safe`
        // set via global config; before any load, fall back to the env var.
        match LAST_SAFE.load(Ordering::Relaxed) {
            0 => false,
            1 => true,
            _ => crate::env::var_is_true("MISE_SAFE"),
        }
    }

    /// Errors when safe mode (`MISE_SAFE=1`) is enabled. Call this before any
    /// operation that would execute code controlled by project configuration.
    /// Safe mode is a security boundary: blocked operations must fail loudly,
    /// never silently fall back to something that executes.
    pub fn ensure_not_safe(operation: &str) -> Result<()> {
        if Settings::safe_mode() {
            bail!(
                "{operation} is disabled in safe mode (MISE_SAFE=1)\nSee https://mise.jdx.dev/configuration/settings.html#safe"
            );
        }
        Ok(())
    }
}

fn redacted_settings_for_debug(settings: &Settings) -> Settings {
    let mut debug_settings = settings.clone();
    if debug_settings.task.cache.remote_token.is_some() {
        debug_settings.task.cache.remote_token = Some("[redacted]".to_string());
    }
    debug_settings
}

fn remove_empty_nested_settings(table: &mut toml::Table, prefix: &str) {
    table.retain(|key, value| {
        let path = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        let Some(child) = value.as_table_mut() else {
            return true;
        };
        remove_empty_nested_settings(child, &path);
        !child.is_empty() || SETTINGS_META.contains_key(path.as_str())
    });
}

fn redact_settings_table(table: &mut toml::Table) {
    let Some(cache) = table
        .get_mut("task")
        .and_then(toml::Value::as_table_mut)
        .and_then(|task| task.get_mut("cache"))
        .and_then(toml::Value::as_table_mut)
    else {
        return;
    };
    if cache.contains_key("remote_token") {
        cache.insert(
            "remote_token".to_string(),
            toml::Value::String("[redacted]".to_string()),
        );
    }
}

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match toml::to_string_pretty(self) {
            Ok(s) => write!(f, "{s}"),
            Err(e) => Err(std::fmt::Error::custom(e)),
        }
    }
}

pub const DEFAULT_NODE_MIRROR_URL: &str = "https://nodejs.org/dist/";

impl SettingsNode {
    pub fn mirror_url(&self) -> Url {
        let s = self
            .mirror_url
            .clone()
            .or(env::var("NODE_BUILD_MIRROR_URL").ok())
            .unwrap_or_else(|| DEFAULT_NODE_MIRROR_URL.to_string());
        Url::parse(&s).unwrap()
    }

    pub fn ninja(&self) -> bool {
        self.ninja.unwrap_or_else(|| which::which("ninja").is_ok())
    }

    pub fn concurrency(&self) -> Option<usize> {
        self.concurrency
            .map(|c| std::cmp::max(c, 1) as usize)
            .or_else(|| {
                if self.ninja() {
                    None
                } else {
                    Some(num_cpus::get_physical())
                }
            })
    }

    pub fn default_packages_file(&self) -> PathBuf {
        self.default_packages_file
            .clone()
            .or_else(|| {
                env::var("NODE_DEFAULT_PACKAGES_FILE")
                    .ok()
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| {
                let p = env::HOME.join(".default-nodejs-packages");
                if p.exists() {
                    return p;
                }
                let p = env::HOME.join(".default-node-packages");
                if p.exists() {
                    return p;
                }
                env::HOME.join(".default-npm-packages")
            })
    }

    pub fn cflags(&self) -> Option<String> {
        self.cflags.clone().or_else(|| env::var("NODE_CFLAGS").ok())
    }

    pub fn configure_opts(&self) -> Option<String> {
        self.configure_opts
            .clone()
            .or_else(|| env::var("NODE_CONFIGURE_OPTS").ok())
    }

    pub fn make_opts(&self) -> Option<String> {
        self.make_opts
            .clone()
            .or_else(|| env::var("NODE_MAKE_OPTS").ok())
    }

    pub fn make_install_opts(&self) -> Option<String> {
        self.make_install_opts
            .clone()
            .or_else(|| env::var("NODE_MAKE_INSTALL_OPTS").ok())
    }

    pub fn configure_cmd(&self, install_path: &Path) -> String {
        let mut configure_cmd = format!("./configure --prefix={}", install_path.display());
        if self.ninja() {
            configure_cmd.push_str(" --ninja");
        }
        if let Some(opts) = self.configure_opts() {
            configure_cmd.push_str(&format!(" {opts}"));
        }
        configure_cmd
    }

    pub fn make_cmd(&self) -> String {
        let mut make_cmd = self.make.clone().unwrap_or_else(|| "make".into());
        if let Some(concurrency) = self.concurrency() {
            make_cmd.push_str(&format!(" -j{concurrency}"));
        }
        if let Some(opts) = self.make_opts() {
            make_cmd.push_str(&format!(" {opts}"));
        }
        make_cmd
    }

    pub fn make_install_cmd(&self) -> String {
        let make = self.make.clone().unwrap_or_else(|| "make".into());
        let mut make_install_cmd = format!("{} install", make);
        if let Some(opts) = self.make_install_opts() {
            make_install_cmd.push_str(&format!(" {opts}"));
        }
        make_install_cmd
    }
}

impl SettingsStatus {
    pub fn missing_tools(&self) -> SettingsStatusMissingTools {
        SettingsStatusMissingTools::from_str(&self.missing_tools).unwrap()
    }
}

/// Deserialize a string to a boolean, accepting "false", "no", "0"
/// and their case-insensitive variants as `false`. Any other value (incl. "") is considered `true`.
fn bool_string<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "false" | "no" | "0" => Ok(false),
        _ => Ok(true),
    }
}

fn set_by_comma<T, C>(input: &str) -> Result<C, <T as FromStr>::Err>
where
    T: FromStr + Eq + Ord,
    C: FromIterator<T>,
{
    input
        .split(',')
        // Filter out empty strings
        .filter_map(|s| {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                Some(T::from_str(trimmed))
            } else {
                None
            }
        })
        // collect into BTreeSet to remove duplicates
        .collect::<Result<BTreeSet<_>, _>>()
        .map(|set| set.into_iter().collect())
}

fn normalize_tool_names(tools: &BTreeSet<String>) -> BTreeSet<String> {
    tools
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn split_default_shell_or_fallback(sa: &str, fallback: &str) -> Result<Vec<String>> {
    let shell = crate::path::split_shell_command(sa)?;
    if shell.is_empty() {
        crate::path::split_shell_command(fallback)
    } else {
        Ok(shell)
    }
}

/// Parse URL replacements from JSON string format
/// Expected format: {"source_domain": "replacement_domain", ...}
pub fn parse_url_replacements(input: &str) -> Result<IndexMap<String, String>, serde_json::Error> {
    serde_json::from_str(input)
}

/// Parse a path list from an environment variable using the OS-native path
/// separator (`:` on Unix, `;` on Windows). This correctly handles Windows
/// absolute paths whose drive letters contain `:` (e.g. `C:\foo`).
fn list_by_os_path_separator<C>(input: &str) -> Result<C, std::convert::Infallible>
where
    C: FromIterator<PathBuf>,
{
    Ok(std::env::split_paths(input)
        .filter(|p| !p.as_os_str().is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_all_compile_is_limited_to_alpine_and_deprecated_nixos_behavior() {
        assert!(default_all_compile(Some("alpine")));
        assert!(default_all_compile(Some("nixos")));
        assert!(!default_all_compile(Some("ubuntu")));
        assert!(!default_all_compile(None));
    }

    #[test]
    fn all_compile_preserves_unset_and_explicit_values() {
        let mut settings = Settings::default();
        assert_eq!(settings.all_compile, None);
        assert!(!settings.all_compile());
        assert_eq!(
            settings.as_dict().unwrap().get("all_compile"),
            Some(&toml::Value::Boolean(false))
        );

        settings.all_compile = Some(true);
        assert!(settings.all_compile());
        assert_eq!(
            settings.as_dict().unwrap().get("all_compile"),
            Some(&toml::Value::Boolean(true))
        );

        settings.all_compile = Some(false);
        assert!(!settings.all_compile());
        assert_eq!(
            settings.as_dict().unwrap().get("all_compile"),
            Some(&toml::Value::Boolean(false))
        );
    }

    #[test]
    fn compile_warning_only_applies_to_implicit_nixos_values() {
        assert!(compile_inherits_nixos_all_compile_default(
            Some("nixos"),
            None,
            None
        ));
        assert!(!compile_inherits_nixos_all_compile_default(
            Some("nixos"),
            Some(true),
            None
        ));
        assert!(!compile_inherits_nixos_all_compile_default(
            Some("nixos"),
            Some(false),
            None
        ));
        assert!(!compile_inherits_nixos_all_compile_default(
            Some("nixos"),
            None,
            Some(true)
        ));
        assert!(!compile_inherits_nixos_all_compile_default(
            Some("nixos"),
            None,
            Some(false)
        ));
        assert!(!compile_inherits_nixos_all_compile_default(
            Some("alpine"),
            None,
            None
        ));
        assert!(!compile_inherits_nixos_all_compile_default(
            None, None, None
        ));
    }

    #[test]
    fn effective_compile_prefers_tool_setting_then_global_source_setting() {
        assert_eq!(effective_compile_setting(true, None), Some(true));
        assert_eq!(effective_compile_setting(false, None), None);
        assert_eq!(effective_compile_setting(true, Some(true)), Some(true));
        assert_eq!(effective_compile_setting(true, Some(false)), Some(false));
    }

    #[test]
    fn debug_settings_redact_remote_cache_token() {
        let mut settings = Settings::default();
        settings.task.cache.remote_token = Some("super-secret-token".to_string());

        let debug = format!("{:?}", redacted_settings_for_debug(&settings));

        assert!(!debug.contains("super-secret-token"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn settings_dictionary_redacts_remote_cache_token() {
        let mut settings = Settings::default();
        settings.task.cache.remote_token = Some("super-secret-token".to_string());

        let table = settings.as_dict().unwrap();
        let encoded = toml::to_string(&table).unwrap();

        assert!(!encoded.contains("super-secret-token"));
        assert!(encoded.contains("[redacted]"));
    }

    #[test]
    fn settings_partial_dictionary_omits_empty_nested_groups() {
        let settings = toml::from_str::<toml::Value>("[task.cache]")
            .unwrap()
            .as_table()
            .unwrap()
            .clone();
        let partial = settings_partial_from_table(settings);

        assert!(
            !Settings::partial_as_dict(&partial)
                .unwrap()
                .contains_key("task")
        );
    }

    #[test]
    fn settings_partial_dictionary_preserves_empty_map_values() {
        let settings = toml::from_str::<toml::Value>("url_replacements = {}")
            .unwrap()
            .as_table()
            .unwrap()
            .clone();
        let partial = settings_partial_from_table(settings);

        assert_eq!(
            Settings::partial_as_dict(&partial)
                .unwrap()
                .get("url_replacements"),
            Some(&toml::Value::Table(toml::Table::new()))
        );
    }

    fn credential_command_settings_table() -> toml::Table {
        toml::from_str::<toml::Value>(
            r#"
            [github]
            credential_command = "echo github-token"

            [gitlab]
            credential_command = "echo gitlab-token"

            [forgejo]
            credential_command = "echo forgejo-token"
            "#,
        )
        .unwrap()
        .as_table()
        .unwrap()
        .clone()
    }

    fn default_shell_args_settings_table() -> toml::Table {
        toml::from_str::<toml::Value>(
            r#"
            unix_default_file_shell_args = "malicious-unix-file-shell"
            unix_default_inline_shell_args = "malicious-unix-inline-shell"
            windows_default_file_shell_args = "malicious-windows-file-shell"
            windows_default_inline_shell_args = "malicious-windows-inline-shell"
            "#,
        )
        .unwrap()
        .as_table()
        .unwrap()
        .clone()
    }

    fn settings_partial_from_table(settings: toml::Table) -> SettingsPartial {
        let mut root = toml::Table::new();
        root.insert("settings".to_string(), toml::Value::Table(settings));
        let settings_file: SettingsFile = toml::Value::Table(root).try_into().unwrap();
        settings_file.settings
    }

    fn sv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_split_default_shell_or_fallback_uses_fallback_for_empty_shell() {
        assert_eq!(
            split_default_shell_or_fallback("   ", "cmd /c").unwrap(),
            sv(&["cmd", "/c"])
        );
    }

    #[test]
    fn test_split_default_shell_or_fallback_preserves_custom_shell() {
        assert_eq!(
            split_default_shell_or_fallback("pwsh -Command", "cmd /c").unwrap(),
            sv(&["pwsh", "-Command"])
        );
    }

    #[test]
    fn test_split_default_shell_or_fallback_reports_parse_errors() {
        assert!(split_default_shell_or_fallback("\"unterminated", "cmd /c").is_err());
    }

    /// The shape #5791 reported: the setting is accepted into the file, so without this it
    /// survives into the partial and `mise settings get` echoes it back while the config
    /// loader — which already ran — never saw it.
    #[test]
    fn test_parse_settings_file_strips_env_only_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mise.toml");
        std::fs::write(
            &path,
            r#"
            [settings]
            default_config_filename = ".mise.toml"
            default_tool_versions_filename = ".tool-versions-custom"
            "#,
        )
        .unwrap();

        let partial = Settings::parse_settings_file(&path).unwrap();

        assert_eq!(partial.default_config_filename, None);
        assert_eq!(partial.default_tool_versions_filename, None);
    }

    /// Unlike `global_only`, being in the *global* config does not rescue these — the loader
    /// has read the file by the time the value would be applied either way.
    #[test]
    fn test_parse_settings_file_strips_env_only_settings_from_global_too() {
        let mut settings = toml::Table::new();
        settings.insert(
            "global_config_file".to_string(),
            toml::Value::String("/tmp/elsewhere.toml".to_string()),
        );
        strip_env_only_settings(&mut settings, Path::new("/tmp/global-config.toml"));

        assert!(settings.get("global_config_file").is_none());
    }

    #[test]
    fn test_parse_settings_file_strips_local_credential_commands() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mise.toml");
        std::fs::write(
            &path,
            r#"
            [settings.github]
            credential_command = "echo github-token"

            [settings.gitlab]
            credential_command = "echo gitlab-token"

            [settings.forgejo]
            credential_command = "echo forgejo-token"
            "#,
        )
        .unwrap();

        let partial = Settings::parse_settings_file(&path).unwrap();

        assert_eq!(partial.github.credential_command, None);
        assert_eq!(partial.gitlab.credential_command, None);
        assert_eq!(partial.forgejo.credential_command, None);
    }

    #[test]
    fn test_global_config_preserves_credential_commands() {
        let path = Path::new("/tmp/global-config.toml");
        let mut settings = credential_command_settings_table();
        strip_local_only_settings(&mut settings, path, true);
        let partial = settings_partial_from_table(settings);

        assert_eq!(
            partial.github.credential_command.as_deref(),
            Some("echo github-token")
        );
        assert_eq!(
            partial.gitlab.credential_command.as_deref(),
            Some("echo gitlab-token")
        );
        assert_eq!(
            partial.forgejo.credential_command.as_deref(),
            Some("echo forgejo-token")
        );
    }

    #[test]
    fn test_local_config_strips_default_shell_args() {
        let path = Path::new("/tmp/.mise.toml");
        let mut settings = default_shell_args_settings_table();
        strip_local_only_settings(&mut settings, path, false);
        let partial = settings_partial_from_table(settings);

        assert_eq!(partial.unix_default_file_shell_args, None);
        assert_eq!(partial.unix_default_inline_shell_args, None);
        assert_eq!(partial.windows_default_file_shell_args, None);
        assert_eq!(partial.windows_default_inline_shell_args, None);
    }

    #[test]
    fn test_global_config_preserves_default_shell_args() {
        let path = Path::new("/tmp/global-config.toml");
        let mut settings = default_shell_args_settings_table();
        strip_local_only_settings(&mut settings, path, true);
        let partial = settings_partial_from_table(settings);

        assert_eq!(
            partial.unix_default_file_shell_args.as_deref(),
            Some("malicious-unix-file-shell")
        );
        assert_eq!(
            partial.unix_default_inline_shell_args.as_deref(),
            Some("malicious-unix-inline-shell")
        );
        assert_eq!(
            partial.windows_default_file_shell_args.as_deref(),
            Some("malicious-windows-file-shell")
        );
        assert_eq!(
            partial.windows_default_inline_shell_args.as_deref(),
            Some("malicious-windows-inline-shell")
        );
    }

    #[test]
    fn test_parse_settings_file_strips_non_global_trust_controls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mise.toml");
        std::fs::write(
            &path,
            r#"
            [settings]
            ci = "true"
            paranoid = true
            trusted_config_paths = ["/"]
            yes = true
            "#,
        )
        .unwrap();

        let partial = Settings::parse_settings_file(&path).unwrap();

        assert_eq!(partial.ci, None);
        assert_eq!(partial.paranoid, None);
        assert_eq!(partial.trusted_config_paths, None);
        assert_eq!(partial.yes, None);
    }

    #[test]
    fn test_parse_settings_file_reads_tera_v1_from_env_directive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mise.toml");
        std::fs::write(
            &path,
            r#"
            env.MISE_TERA_V1 = true
            "#,
        )
        .unwrap();

        let partial = Settings::parse_settings_file(&path).unwrap();

        assert_eq!(partial.tera_v1, Some(true));
    }

    #[test]
    fn test_parse_settings_file_reads_tera_v1_from_env_value_directive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mise.toml");
        std::fs::write(
            &path,
            r#"
            [env]
            MISE_TERA_V1 = { value = "1" }
            "#,
        )
        .unwrap();

        let partial = Settings::parse_settings_file(&path).unwrap();

        assert_eq!(partial.tera_v1, Some(true));
    }

    #[test]
    fn test_parse_settings_file_prefers_explicit_tera_v1_setting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mise.toml");
        std::fs::write(
            &path,
            r#"
            [settings]
            tera_v1 = false

            [env]
            MISE_TERA_V1 = true
            "#,
        )
        .unwrap();

        let partial = Settings::parse_settings_file(&path).unwrap();

        assert_eq!(partial.tera_v1, Some(false));
    }

    /// Run the rewrite over a `[settings]` block and return the resulting
    /// `aqua.registries` entries, as `parse_settings_file` would see them.
    fn rewritten_registries(dir: &Path, body: &str) -> Vec<String> {
        let mut settings = toml::from_str::<toml::Table>(body).unwrap();
        resolve_aqua_registry_paths(&mut settings, &dir.join(".mise.toml"));
        settings["aqua"]["registries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    /// discussion #4306: a registry committed next to the project could not be
    /// referenced, because a relative entry failed as `relative URL without a
    /// base`.
    #[test]
    fn relative_aqua_registry_resolves_against_config_root() {
        let dir = tempfile::tempdir().unwrap();
        let registries = rewritten_registries(dir.path(), r#"aqua.registries = ["registry.yaml"]"#);

        let expected = Url::from_file_path(dir.path().join("registry.yaml"))
            .unwrap()
            .to_string();
        assert_eq!(registries, vec![expected]);
    }

    /// `aqua.registries = [..]` and `[aqua]` + `registries = [..]` parse to the
    /// same nested table, so both spellings must be picked up.
    #[test]
    fn table_form_registries_also_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let registries = rewritten_registries(
            dir.path(),
            r#"
            [aqua]
            registries = ["registry.yaml"]
            "#,
        );

        let expected = Url::from_file_path(dir.path().join("registry.yaml"))
            .unwrap()
            .to_string();
        assert_eq!(registries, vec![expected]);
    }

    #[test]
    fn absolute_registry_urls_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let registries = rewritten_registries(
            dir.path(),
            r#"
            aqua.registries = [
              "https://github.com/aquaproj/aqua-registry",
              "file:///somewhere/registry.yaml",
            ]
            "#,
        );

        assert_eq!(
            registries,
            vec![
                "https://github.com/aquaproj/aqua-registry".to_string(),
                "file:///somewhere/registry.yaml".to_string(),
            ]
        );
    }

    #[test]
    fn parent_relative_registry_path_is_normalized() {
        let dir = tempfile::tempdir().unwrap();
        let registries = rewritten_registries(
            dir.path(),
            r#"aqua.registries = ["../shared/registry.yaml"]"#,
        );

        assert!(
            !registries[0].contains(".."),
            "expected `..` to be normalized away, got {}",
            registries[0]
        );
    }

    #[test]
    fn registries_without_aqua_table_are_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = toml::from_str::<toml::Table>("offline = true").unwrap();
        resolve_aqua_registry_paths(&mut settings, &dir.path().join(".mise.toml"));
        assert_eq!(settings["offline"].as_bool(), Some(true));
    }

    #[test]
    fn age_paths_resolve_against_the_declaring_config_root() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("mise.toml");
        let mut settings = toml::from_str::<toml::Table>(
            r#"
            [age]
            key_file = "keys/age.txt"
            identity_files = ["identities/first.txt", "{{ config_root }}/second.txt"]
            ssh_identity_files = ["ssh/id_ed25519"]
            "#,
        )
        .unwrap();

        resolve_age_paths(&mut settings, &config_path).unwrap();

        assert_eq!(
            settings["age"]["key_file"].as_str(),
            Some(dir.path().join("keys/age.txt").to_str().unwrap())
        );
        assert_eq!(
            settings["age"]["identity_files"][0].as_str(),
            Some(dir.path().join("identities/first.txt").to_str().unwrap())
        );
        assert_eq!(
            Path::new(settings["age"]["identity_files"][1].as_str().unwrap()),
            dir.path().join("second.txt")
        );
        assert_eq!(
            settings["age"]["ssh_identity_files"][0].as_str(),
            Some(dir.path().join("ssh/id_ed25519").to_str().unwrap())
        );
    }

    #[test]
    fn age_paths_preserve_absolute_and_home_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let absolute = dir.path().join("absolute.txt");
        let mut age = toml::Table::new();
        age.insert(
            "key_file".into(),
            toml::Value::String(absolute.to_string_lossy().into_owned()),
        );
        age.insert(
            "identity_files".into(),
            toml::Value::Array(vec![toml::Value::String("~/identity.txt".into())]),
        );
        let mut settings = toml::Table::new();
        settings.insert("age".into(), toml::Value::Table(age));

        resolve_age_paths(&mut settings, &dir.path().join("mise.toml")).unwrap();

        assert_eq!(
            settings["age"]["key_file"].as_str(),
            Some(absolute.to_str().unwrap())
        );
        assert_eq!(
            settings["age"]["identity_files"][0].as_str(),
            Some("~/identity.txt")
        );
    }

    #[test]
    fn invalid_age_path_template_names_the_setting_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("mise.toml");
        let mut settings = toml::from_str::<toml::Table>(
            r#"
            [age]
            key_file = "{{ missing_variable }}"
            "#,
        )
        .unwrap();

        let err = resolve_age_paths(&mut settings, &config_path)
            .unwrap_err()
            .to_string();
        assert!(err.contains("settings.age.key_file"), "{err}");
        assert!(err.contains("mise.toml"), "{err}");
    }

    /// A drive-letter path parses as a URL whose scheme is one character, so it
    /// must not be mistaken for an absolute URL and left unusable.
    #[cfg(windows)]
    #[test]
    fn windows_drive_registry_path_is_treated_as_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let registries =
            rewritten_registries(dir.path(), r#"aqua.registries = ['C:\reg\registry.yaml']"#);

        assert_eq!(registries.len(), 1);
        assert!(
            registries[0].starts_with("file:///C:/reg/"),
            "expected a file URL, got {}",
            registries[0]
        );
    }

    #[test]
    fn test_deprecated_settings_in_toml_table_detects_nested_settings() {
        let settings = toml::from_str::<toml::Value>(
            r#"
            tera_v1 = true

            [aqua]
            registry_url = "https://example.com/aqua-registry"
            "#,
        )
        .unwrap()
        .as_table()
        .unwrap()
        .clone();

        let deprecated = deprecated_settings_in_toml_table(&settings);

        assert!(deprecated.contains(&"tera_v1"));
        assert!(deprecated.contains(&"aqua.registry_url"));
    }

    #[test]
    fn test_deprecated_settings_in_env_directives_detects_setting_env() {
        let raw = toml::from_str::<toml::Value>(
            r#"
            [env]
            MISE_TERA_V1 = true
            "#,
        )
        .unwrap();

        assert_eq!(deprecated_settings_in_env_directives(&raw), vec!["tera_v1"]);
    }

    #[test]
    fn test_deprecated_settings_in_env_directives_ignores_unset_env() {
        let raw = toml::from_str::<toml::Value>(
            r#"
            [env]
            MISE_TERA_V1 = { unset = true }
            "#,
        )
        .unwrap();

        assert!(deprecated_settings_in_env_directives(&raw).is_empty());
    }

    #[test]
    fn test_deprecated_settings_in_env_directives_ignores_child_only_env() {
        let raw = toml::from_str::<toml::Value>(
            r#"
            [env]
            MISE_SHORTHANDS_FILE = "~/.mise-shorthands"
            "#,
        )
        .unwrap();

        assert!(deprecated_settings_in_env_directives(&raw).is_empty());
    }

    #[test]
    fn test_global_config_preserves_trust_controls() {
        let path = Path::new("/tmp/global-config.toml");
        let mut settings = toml::from_str::<toml::Value>(
            r#"
            ci = "true"
            paranoid = true
            trusted_config_paths = ["/"]
            yes = true
            "#,
        )
        .unwrap()
        .as_table()
        .unwrap()
        .clone();
        strip_local_only_settings(&mut settings, path, true);
        let partial = settings_partial_from_table(settings);

        assert_eq!(partial.ci, Some(true));
        assert_eq!(partial.paranoid, Some(true));
        assert_eq!(
            partial.trusted_config_paths,
            Some([PathBuf::from("/")].into_iter().collect())
        );
        assert_eq!(partial.yes, Some(true));
    }

    #[test]
    fn test_set_by_comma_empty_string() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BTreeSet::new());
    }

    #[test]
    fn test_set_by_comma_whitespace_only() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("  ");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BTreeSet::new());
    }

    #[test]
    fn test_set_by_comma_single_value() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo");
        assert!(result.is_ok());
        let expected: BTreeSet<String> = ["foo".to_string()].into_iter().collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_multiple_values() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo,bar,baz");
        assert!(result.is_ok());
        let expected: BTreeSet<String> = ["foo".to_string(), "bar".to_string(), "baz".to_string()]
            .into_iter()
            .collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_with_whitespace() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo, bar, baz");
        assert!(result.is_ok());
        let expected: BTreeSet<String> = ["foo".to_string(), "bar".to_string(), "baz".to_string()]
            .into_iter()
            .collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_trailing_comma() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo,bar,");
        assert!(result.is_ok());
        let expected: BTreeSet<String> =
            ["foo".to_string(), "bar".to_string()].into_iter().collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_duplicate_values() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo,bar,foo");
        assert!(result.is_ok());
        let expected: BTreeSet<String> =
            ["foo".to_string(), "bar".to_string()].into_iter().collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_empty_elements() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo,,bar");
        assert!(result.is_ok());
        let expected: BTreeSet<String> =
            ["foo".to_string(), "bar".to_string()].into_iter().collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_normalize_tool_names() {
        let tools = BTreeSet::from([
            " node ".to_string(),
            "  ".to_string(),
            "ruby".to_string(),
            "".to_string(),
        ]);
        let expected = BTreeSet::from(["node".to_string(), "ruby".to_string()]);
        assert_eq!(normalize_tool_names(&tools), expected);
    }

    #[test]
    fn test_offline_default_is_false() {
        Settings::reset(None);
        let settings = Settings::get();
        // When neither setting nor env var is set, offline should be false
        // (env::OFFLINE is process-global so we can't easily toggle it,
        // but the setting field defaults to false)
        assert!(!settings.offline);
    }

    #[test]
    fn test_prefer_offline_default_is_false() {
        Settings::reset(None);
        let settings = Settings::get();
        assert!(!settings.prefer_offline);
    }

    #[test]
    fn test_cargo_binstall_quickinstall_setting() {
        let settings = Settings::builder().load().unwrap();
        assert!(!settings.cargo.binstall_quickinstall);

        let meta = SETTINGS_META
            .get("cargo.binstall_quickinstall")
            .expect("cargo.binstall_quickinstall setting should exist");
        assert_eq!(meta.env, Some("MISE_CARGO_BINSTALL_QUICKINSTALL"));
    }

    #[test]
    fn test_three_level_task_cache_setting() {
        let settings = Settings::builder().load().unwrap();
        assert_eq!(
            settings.task.cache.remote_mode,
            crate::cache::CacheRemoteMode::ReadWrite
        );

        let meta = SETTINGS_META
            .get("task.cache.remote_mode")
            .expect("task.cache.remote_mode setting should exist");
        assert_eq!(meta.env, Some("MISE_TASK_CACHE_REMOTE_MODE"));
    }

    #[test]
    fn test_offline_setting_enables_offline() {
        let mut partial = SettingsPartial::empty();
        partial.offline = Some(true);
        Settings::reset(Some(partial));
        let settings = Settings::get();
        assert!(settings.offline());
        Settings::reset(None);
    }

    #[test]
    fn test_reload_preserves_cli_settings() {
        let mut partial = SettingsPartial::empty();
        partial.offline = Some(true);
        Settings::reset(Some(partial));
        assert!(Settings::get().offline());

        Settings::reload();
        assert!(Settings::get().offline());
        Settings::reset(None);
    }

    #[test]
    fn test_offline_implies_prefer_offline() {
        let mut partial = SettingsPartial::empty();
        partial.offline = Some(true);
        Settings::reset(Some(partial));
        let settings = Settings::get();
        assert!(settings.prefer_offline());
        Settings::reset(None);
    }

    #[test]
    fn test_prefer_offline_setting() {
        let mut partial = SettingsPartial::empty();
        partial.prefer_offline = Some(true);
        Settings::reset(Some(partial));
        let settings = Settings::get();
        assert!(settings.prefer_offline());
        // prefer_offline does NOT imply offline
        assert!(!settings.offline);
        Settings::reset(None);
    }

    #[test]
    fn test_install_before_hidden_alias_sets_minimum_release_age() {
        let mut partial = SettingsPartial::empty();
        partial.install_before = Some("7d".to_string());
        Settings::reset(Some(partial));
        let settings = Settings::get();
        assert_eq!(settings.minimum_release_age.as_deref(), Some("7d"));
        Settings::reset(None);
    }

    #[test]
    fn test_minimum_release_age_hidden_alias_wins_over_install_before() {
        let mut partial = SettingsPartial::empty();
        partial.install_before = Some("7d".to_string());
        partial.minimum_release_age = Some("3d".to_string());
        Settings::reset(Some(partial));
        let settings = Settings::get();
        assert_eq!(settings.minimum_release_age.as_deref(), Some("3d"));
        Settings::reset(None);
    }

    #[test]
    fn test_task_cache_hidden_aliases_map_to_nested_settings() {
        let settings_file: SettingsFile = toml::from_str(
            r#"
            [settings.task]
            cache_remote_mode = "read-only"
            cache_remote_token = "secret"
            cache_remote_url = "https://old.example.com"

            [settings.task.cache]
            remote_url = "https://new.example.com"
            "#,
        )
        .unwrap();

        let partial = normalize_hidden_config_aliases(settings_file.settings);
        assert_eq!(partial.task.cache_remote_mode, None);
        assert_eq!(partial.task.cache_remote_token, None);
        assert_eq!(partial.task.cache_remote_url, None);
        assert_eq!(
            partial.task.cache.remote_mode,
            Some(crate::cache::CacheRemoteMode::ReadOnly)
        );
        assert_eq!(
            partial.task.cache.remote_url.as_deref(),
            Some("https://new.example.com")
        );
        assert_eq!(partial.task.cache.remote_token.as_deref(), Some("secret"));
    }

    #[test]
    fn test_aqua_registry_url_accepts_single_string() {
        let settings_file: SettingsFile = toml::from_str(
            r#"
            [settings]
            aqua.registry_url = "https://example.com/registry"
            "#,
        )
        .unwrap();

        assert_eq!(
            settings_file.settings.aqua.registry_url,
            Some("https://example.com/registry".to_string())
        );
    }

    #[test]
    fn test_aqua_registries_accepts_list() {
        let settings_file: SettingsFile = toml::from_str(
            r#"
            [settings]
            aqua.registries = [
              "https://example.com/first",
              "https://example.com/second",
            ]
            "#,
        )
        .unwrap();

        assert_eq!(
            settings_file.settings.aqua.registries,
            Some(vec![
                "https://example.com/first".to_string(),
                "https://example.com/second".to_string(),
            ])
        );
    }

    #[test]
    fn test_aqua_registry_url_sets_registries() {
        let mut settings = Settings::default();
        settings.aqua.registry_url = Some("https://example.com/legacy".to_string());

        settings.set_hidden_configs();

        assert_eq!(settings.aqua.registry_url, None);
        assert_eq!(
            settings.aqua.registries,
            Some(vec!["https://example.com/legacy".to_string()])
        );
    }

    #[test]
    fn test_aqua_registries_overrides_registry_url() {
        let mut settings = Settings::default();
        settings.aqua.registry_url = Some("https://example.com/legacy".to_string());
        settings.aqua.registries = Some(vec!["https://example.com/new".to_string()]);

        settings.set_hidden_configs();

        assert_eq!(settings.aqua.registry_url, None);
        assert_eq!(
            settings.aqua.registries,
            Some(vec!["https://example.com/new".to_string()])
        );
    }

    #[test]
    fn test_settings_toml_is_sorted() {
        let content =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/settings.toml"))
                .expect("failed to read settings.toml");
        let table: toml::Table = content.parse().expect("failed to parse settings.toml");

        fn collect_keys(table: &toml::Table, prefix: &str) -> Vec<String> {
            let mut keys = Vec::new();
            for (key, value) in table {
                let full_key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if let toml::Value::Table(sub) = value {
                    // A nested table that has no "type" or "description" is a grouping table
                    // (e.g., [aqua], [node]), not a setting itself.
                    if !sub.contains_key("type") && !sub.contains_key("description") {
                        keys.extend(collect_keys(sub, &full_key));
                        continue;
                    }
                }
                keys.push(full_key);
            }
            keys
        }

        let keys = collect_keys(&table, "");
        let mut sorted = keys.clone();
        sorted.sort();

        for (i, (got, expected)) in keys.iter().zip(sorted.iter()).enumerate() {
            assert_eq!(
                got, expected,
                "settings.toml is not alphabetically sorted at index {i}: found \"{got}\", expected \"{expected}\". Run the sort script or reorder manually."
            );
        }
    }

    /// Every scalar setting type in settings.toml. Anything else is a collection.
    ///
    /// Spelled as "not a scalar" rather than by listing the collections on purpose. A new
    /// collection type — `SetPath`, say — has the same failure mode and is caught here
    /// automatically, whereas enumerating `List*`/`SetString`/`IndexMap<…>` would let it through
    /// silently, which is the exact failure this guard exists to prevent. A new *scalar* type
    /// instead fails this test loudly and is fixed by adding one entry here, which is the cheaper
    /// mistake to make.
    const SCALAR_SETTING_TYPES: &[&str] = &[
        "Bool",
        "BoolOrString",
        "Duration",
        "Integer",
        "Path",
        "String",
        "Url",
    ];

    /// Collect settings that are readable from the environment, hold a collection, and declare no
    /// `parse_env`.
    fn collect_settings_missing_parse_env(
        table: &toml::Table,
        prefix: &str,
        missing: &mut Vec<String>,
    ) {
        for (key, value) in table {
            let toml::Value::Table(setting) = value else {
                continue;
            };
            let full_key = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            // A nested table that has no "type" or "description" is a grouping table
            // (e.g., [aqua], [node]), not a setting itself.
            if !setting.contains_key("type") && !setting.contains_key("description") {
                collect_settings_missing_parse_env(setting, &full_key, missing);
                continue;
            }
            let is_collection = setting
                .get("type")
                .and_then(|type_| type_.as_str())
                .is_some_and(|type_| !SCALAR_SETTING_TYPES.contains(&type_));
            if is_collection && setting.contains_key("env") && !setting.contains_key("parse_env") {
                missing.push(full_key);
            }
        }
    }

    #[test]
    fn test_settings_toml_collection_settings_declare_parse_env() {
        // A collection setting that can be set from the environment needs `parse_env`. Without it
        // confique hands the raw string to a `Vec`, set or map deserializer and mise aborts before
        // doing anything: "failed to deserialize value `SettingsAge::identity_files` from
        // environment variable `MISE_AGE_IDENTITY_FILES`: invalid type: string "...", expected a
        // sequence".
        let content =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/settings.toml"))
                .expect("failed to read settings.toml");
        let table: toml::Table = content.parse().expect("failed to parse settings.toml");

        let mut missing = vec![];
        collect_settings_missing_parse_env(&table, "", &mut missing);

        assert!(
            missing.is_empty(),
            "these collection settings are readable from the environment but declare no \
             parse_env, so mise panics as soon as one is set: {missing:?}"
        );
    }

    /// The guard above only earns its place if it catches every collection type, not just `List*`.
    /// settings.toml also carries `SetString` and `IndexMap<String, String>`, which fail the same
    /// way, so a violation of each is checked against a fixture here rather than waiting for one
    /// to be committed.
    #[test]
    fn test_parse_env_guard_covers_sets_and_maps_too() {
        let table: toml::Table = r#"
            [bad_list]
            description = "x"
            type = "ListString"
            env = "MISE_BAD_LIST"

            [bad_set]
            description = "x"
            type = "SetString"
            env = "MISE_BAD_SET"

            [bad_map]
            description = "x"
            type = "IndexMap<String, String>"
            env = "MISE_BAD_MAP"

            [group.bad_nested]
            description = "x"
            type = "ListPath"
            env = "MISE_BAD_NESTED"

            [ok_has_parse_env]
            description = "x"
            type = "SetString"
            env = "MISE_OK_PARSE"
            parse_env = "set_by_comma"

            [ok_no_env]
            description = "x"
            type = "SetString"

            [ok_scalar]
            description = "x"
            type = "String"
            env = "MISE_OK_SCALAR"
        "#
        .parse()
        .expect("fixture parses");

        let mut missing = vec![];
        collect_settings_missing_parse_env(&table, "", &mut missing);
        missing.sort();

        assert_eq!(
            missing,
            vec!["bad_list", "bad_map", "bad_set", "group.bad_nested"]
        );
    }

    #[test]
    fn test_settings_node_build_cmds() {
        let node = SettingsNode::default();
        let path = Path::new("/tmp/install");

        // Defaults
        assert!(
            node.configure_cmd(path)
                .starts_with("./configure --prefix=/tmp/install")
        );
        assert!(node.make_cmd().starts_with("make"));
        assert_eq!(node.make_install_cmd(), "make install");
    }

    #[test]
    fn test_settings_node_build_cmds_with_opts() {
        let node = SettingsNode {
            configure_opts: Some("--verbose".to_string()),
            make_opts: Some("-s".to_string()),
            make_install_opts: Some("--no-strip".to_string()),
            make: Some("gmake".to_string()),
            concurrency: Some(4),
            ..Default::default()
        };

        let path = Path::new("/tmp/install");
        assert!(node.configure_cmd(path).contains("--verbose"));
        assert!(node.make_cmd().starts_with("gmake -j4 -s"));
        assert_eq!(node.make_install_cmd(), "gmake install --no-strip");
    }

    #[test]
    fn test_list_by_os_path_separator_empty() {
        let result: Result<Vec<PathBuf>, _> = list_by_os_path_separator("");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_list_by_os_path_separator_single() {
        #[cfg(not(windows))]
        let (input, expected) = ("/foo/bar", PathBuf::from("/foo/bar"));
        #[cfg(windows)]
        let (input, expected) = (r"C:\foo\bar", PathBuf::from(r"C:\foo\bar"));
        let result: Vec<PathBuf> = list_by_os_path_separator(input).unwrap();
        assert_eq!(result, vec![expected]);
    }

    #[test]
    #[cfg(unix)]
    fn test_list_by_os_path_separator_multiple_unix() {
        let result: Vec<PathBuf> = list_by_os_path_separator("/foo:/bar").unwrap();
        assert_eq!(result, vec![PathBuf::from("/foo"), PathBuf::from("/bar")]);
    }

    #[test]
    #[cfg(windows)]
    fn test_list_by_os_path_separator_multiple_windows() {
        let result: Vec<PathBuf> = list_by_os_path_separator(r"C:\foo;D:\bar").unwrap();
        assert_eq!(
            result,
            vec![PathBuf::from(r"C:\foo"), PathBuf::from(r"D:\bar")]
        );
    }

    #[test]
    fn test_list_by_os_path_separator_as_btreeset() {
        // Verify the function works with BTreeSet as the collection type,
        // matching the field types used in Settings (e.g. trusted_config_paths).
        #[cfg(not(windows))]
        let (input, a, b) = ("/foo:/bar", PathBuf::from("/foo"), PathBuf::from("/bar"));
        #[cfg(windows)]
        let (input, a, b) = (
            r"C:\foo;D:\bar",
            PathBuf::from(r"C:\foo"),
            PathBuf::from(r"D:\bar"),
        );
        let result: BTreeSet<PathBuf> = list_by_os_path_separator(input).unwrap();
        assert_eq!(result, [a, b].into_iter().collect());
    }
}
