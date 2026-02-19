use crate::cli::Cli;
use crate::config::ALL_TOML_CONFIG_FILES;
use crate::duration;
use crate::file::FindUp;
use crate::{dirs, env, file};
#[allow(unused_imports)]
use confique::env::parse::{list_by_colon, list_by_comma};
use confique::{Config, Partial};
use eyre::{Result, bail};
use indexmap::{IndexMap, indexmap};
use itertools::Itertools;
use serde::ser::Error;
use serde::{Deserialize, Deserializer, Serializer};
use serde_derive::Serialize;
use std::env::consts::{ARCH, OS};
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock as Lazy;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use std::{
    collections::{BTreeSet, HashSet},
    sync::atomic::Ordering,
};
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

pub struct SettingsMeta {
    // pub key: String,
    pub type_: SettingsType,
    pub description: &'static str,
    pub deprecated: Option<&'static str>,
    pub deprecated_warn_at: Option<&'static str>,
    pub deprecated_remove_at: Option<&'static str>,
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
    Npm,
    Bun,
    Pnpm,
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

pub type SettingsPartial = <Settings as Config>::Partial;

static BASE_SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);
static CLI_SETTINGS: Mutex<Option<SettingsPartial>> = Mutex::new(None);
static DEFAULT_SETTINGS: Lazy<SettingsPartial> = Lazy::new(|| {
    let mut s = SettingsPartial::empty();
    s.python.default_packages_file = Some(env::HOME.join(".default-python-packages"));
    if let Some("alpine" | "nixos") = env::LINUX_DISTRO.as_ref().map(|s| s.as_str())
        && !cfg!(test)
    {
        s.all_compile = Some(true);
    }
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

fn warn_deprecated(key: &str) {
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

impl Settings {
    pub fn get() -> Arc<Self> {
        Self::try_get().unwrap()
    }
    pub fn try_get() -> Result<Arc<Self>> {
        if let Some(settings) = BASE_SETTINGS.read().unwrap().as_ref() {
            return Ok(settings.clone());
        }
        time!("try_get");

        // Initial pass to obtain cd option
        let mut sb = Self::builder()
            .preloaded(CLI_SETTINGS.lock().unwrap().clone().unwrap_or_default())
            .env();

        let mut settings = sb.load()?;
        if let Some(mut cd) = settings.cd {
            static ORIG_PATH: Lazy<std::io::Result<PathBuf>> = Lazy::new(env::current_dir);
            if cd.is_relative() {
                cd = ORIG_PATH.as_ref()?.join(cd);
            }
            env::set_current_dir(cd)?;
        }

        // Reload settings after current directory option processed
        sb = Self::builder()
            .preloaded(CLI_SETTINGS.lock().unwrap().clone().unwrap_or_default())
            .env();
        for file in Self::all_settings_files() {
            sb = sb.preloaded(file);
        }
        sb = sb.preloaded(DEFAULT_SETTINGS.clone());

        settings = sb.load()?;
        if !settings.legacy_version_file {
            settings.idiomatic_version_file = Some(false);
        }
        if settings.raw {
            settings.jobs = 1;
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
        let args = env::args().collect_vec();
        // handle the special case of `mise -v` which should show version, not set verbose
        if settings.verbose && !(args.len() == 2 && args[1] == "-v") {
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
        if settings.all_compile {
            if settings.node.compile.is_none() {
                settings.node.compile = Some(true);
            }
            if settings.python.compile.is_none() {
                settings.python.compile = Some(true);
            }
            if settings.erlang.compile.is_none() {
                settings.erlang.compile = Some(true);
            }
            if settings.ruby.compile.is_none() {
                settings.ruby.compile = Some(true);
            }
        }
        if settings.gpg_verify.is_some() {
            settings.node.gpg_verify = settings.node.gpg_verify.or(settings.gpg_verify);
            settings.swift.gpg_verify = settings.swift.gpg_verify.or(settings.gpg_verify);
        }
        settings.set_hidden_configs();
        if cfg!(test) {
            settings.experimental = true;
        }
        let settings = Arc::new(settings);
        *BASE_SETTINGS.write().unwrap() = Some(settings.clone());
        time!("try_get done");
        trace!("Settings: {:#?}", settings);
        Ok(settings)
    }

    /// Sets deprecated settings to new names
    fn set_hidden_configs(&mut self) {
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
        if let Some(false) = self.asdf {
            self.disable_backends.push("asdf".to_string());
        }
        if let Some(false) = self.vfox {
            self.disable_backends.push("vfox".to_string());
        }
        if let Some(disable_default_shorthands) = self.disable_default_shorthands {
            self.disable_default_registry = disable_default_shorthands;
        }
        if let Some(cargo_binstall) = self.cargo_binstall {
            self.cargo.binstall = cargo_binstall;
        }
        if let Some(pipx_uvx) = self.pipx_uvx {
            self.pipx.uvx = Some(pipx_uvx);
        }
        if let Some(python_compile) = self.python_compile {
            self.python.compile = Some(python_compile);
        }
        if let Some(python_default_packages_file) = &self.python_default_packages_file {
            self.python.default_packages_file = Some(python_default_packages_file.clone());
        }
        if let Some(python_patch_url) = &self.python_patch_url {
            self.python.patch_url = Some(python_patch_url.clone());
        }
        if let Some(python_patches_directory) = &self.python_patches_directory {
            self.python.patches_directory = Some(python_patches_directory.clone());
        }
        if let Some(python_precompiled_arch) = &self.python_precompiled_arch {
            self.python.precompiled_arch = Some(python_precompiled_arch.clone());
        }
        if let Some(python_precompiled_os) = &self.python_precompiled_os {
            self.python.precompiled_os = Some(python_precompiled_os.clone());
        }
        if let Some(python_pyenv_repo) = &self.python_pyenv_repo {
            self.python.pyenv_repo = python_pyenv_repo.clone();
        }
        if let Some(python_venv_stdlib) = self.python_venv_stdlib {
            self.python.venv_stdlib = python_venv_stdlib;
        }
        if let Some(python_venv_auto_create) = self.python_venv_auto_create {
            self.python.venv_auto_create = python_venv_auto_create;
        }
        if self.npm.bun {
            self.npm.package_manager = NpmPackageManager::Bun;
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
        if cli.quiet {
            s.quiet = Some(true);
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
        let settings_file: SettingsFile = toml::from_str(&raw)?;

        Ok(settings_file.settings)
    }

    fn all_settings_files() -> Vec<SettingsPartial> {
        ALL_TOML_CONFIG_FILES
            .iter()
            .map(|p| Self::parse_settings_file(p))
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
                "trace",
                "log_level",
                "python_venv_auto_create",
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
            .filter_map(|p| p.canonicalize().ok())
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
        let table = toml::from_str(&s)?;
        Ok(table)
    }

    pub fn cache_prune_age_duration(&self) -> Option<Duration> {
        let age = duration::parse_duration(&self.cache_prune_age).unwrap();
        if age.as_secs() == 0 { None } else { Some(age) }
    }

    pub fn fetch_remote_versions_timeout(&self) -> Duration {
        duration::parse_duration(&self.fetch_remote_versions_timeout).unwrap()
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
        self.disable_tools
            .iter()
            .map(|t| t.trim().to_string())
            .collect()
    }

    pub fn enable_tools(&self) -> BTreeSet<String> {
        self.enable_tools
            .iter()
            .map(|t| t.trim().to_string())
            .collect()
    }

    pub fn partial_as_dict(partial: &SettingsPartial) -> eyre::Result<toml::Table> {
        let s = toml::to_string(partial)?;
        let table = toml::from_str(&s)?;
        Ok(table)
    }

    pub fn default_inline_shell(&self) -> Result<Vec<String>> {
        let sa = if cfg!(windows) {
            &self.windows_default_inline_shell_args
        } else {
            &self.unix_default_inline_shell_args
        };
        Ok(shell_words::split(sa)?)
    }

    pub fn default_file_shell(&self) -> Result<Vec<String>> {
        let sa = if cfg!(windows) {
            &self.windows_default_file_shell_args
        } else {
            &self.unix_default_file_shell_args
        };
        Ok(shell_words::split(sa)?)
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

    pub fn configure_cmd(&self, install_path: &Path) -> String {
        let mut configure_cmd = format!("./configure --prefix={}", install_path.display());
        if self.ninja() {
            configure_cmd.push_str(" --ninja");
        }
        if let Some(opts) = self
            .configure_opts
            .clone()
            .or_else(|| env::var("NODE_CONFIGURE_OPTS").ok())
        {
            configure_cmd.push_str(&format!(" {opts}"));
        }
        configure_cmd
    }

    pub fn make_cmd(&self) -> String {
        let mut make_cmd = self.make.clone().unwrap_or_else(|| "make".into());
        if let Some(concurrency) = self.concurrency() {
            make_cmd.push_str(&format!(" -j{concurrency}"));
        }
        if let Some(opts) = self
            .make_opts
            .clone()
            .or_else(|| env::var("NODE_MAKE_OPTS").ok())
        {
            make_cmd.push_str(&format!(" {opts}"));
        }
        make_cmd
    }

    pub fn make_install_cmd(&self) -> String {
        let make = self.make.clone().unwrap_or_else(|| "make".into());
        let mut make_install_cmd = format!("{} install", make);
        if let Some(opts) = self
            .make_install_opts
            .clone()
            .or_else(|| env::var("NODE_MAKE_INSTALL_OPTS").ok())
        {
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

/// Parse URL replacements from JSON string format
/// Expected format: {"source_domain": "replacement_domain", ...}
pub fn parse_url_replacements(input: &str) -> Result<IndexMap<String, String>, serde_json::Error> {
    serde_json::from_str(input)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_offline_setting_enables_offline() {
        let mut partial = SettingsPartial::empty();
        partial.offline = Some(true);
        Settings::reset(Some(partial));
        let settings = Settings::get();
        assert!(settings.offline());
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
}
