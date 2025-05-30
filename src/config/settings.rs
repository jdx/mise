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
use serde::{Deserialize, Deserializer};
use serde_derive::Serialize;
use std::env::consts::ARCH;
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
}

pub struct SettingsMeta {
    // pub key: String,
    pub type_: SettingsType,
    pub description: &'static str,
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

pub type SettingsPartial = <Settings as Config>::Partial;

static BASE_SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);
static CLI_SETTINGS: Mutex<Option<SettingsPartial>> = Mutex::new(None);
static DEFAULT_SETTINGS: Lazy<SettingsPartial> = Lazy::new(|| {
    let mut s = SettingsPartial::empty();
    s.python.default_packages_file = Some(env::HOME.join(".default-python-packages"));
    if let Some("alpine" | "nixos") = env::LINUX_DISTRO.as_ref().map(|s| s.as_str()) {
        if !cfg!(test) {
            s.all_compile = Some(true);
        }
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
            settings.idiomatic_version_file = false;
        }
        if settings.raw {
            settings.jobs = 1;
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
            settings.node.compile = Some(true);
            if settings.python.compile.is_none() {
                settings.python.compile = Some(true);
            }
            if settings.erlang.compile.is_none() {
                settings.erlang.compile = Some(true);
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
        if !self.auto_install {
            self.exec_auto_install = false;
            self.not_found_auto_install = false;
            self.task_run_auto_install = false;
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
    }

    pub fn add_cli_matches(cli: &Cli) {
        let mut s = SettingsPartial::empty();
        for arg in &*env::ARGS.read().unwrap() {
            if arg == "--" {
                break;
            }
            if arg == "--raw" {
                s.raw = Some(true);
            }
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
        if cli.global_output_flags.quiet {
            s.quiet = Some(true);
        }
        if cli.global_output_flags.trace {
            s.log_level = Some("trace".to_string());
        }
        if cli.global_output_flags.debug {
            s.log_level = Some("debug".to_string());
        }
        if let Some(log_level) = &cli.global_output_flags.log_level {
            s.log_level = Some(log_level.to_string());
        }
        if cli.global_output_flags.verbose > 0 {
            s.verbose = Some(true);
        }
        if cli.global_output_flags.verbose > 1 {
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
        if let Some(cwd) = &*dirs::CWD {
            if let Some(env_file) = &self.env_file {
                let env_file = env_file.to_string_lossy().to_string();
                for p in FindUp::new(cwd, &[env_file]) {
                    files.push(p);
                }
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
        if env::PREFER_OFFLINE.load(Ordering::Relaxed) {
            None
        } else {
            Some(duration::parse_duration(&self.fetch_remote_versions_cache).unwrap())
        }
    }

    pub fn http_timeout(&self) -> Duration {
        duration::parse_duration(&self.http_timeout).unwrap()
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

    pub fn arch(&self) -> &str {
        self.arch.as_deref().unwrap_or(ARCH)
    }

    pub fn no_config() -> bool {
        *env::MISE_NO_CONFIG
            || env::ARGS
                .read()
                .unwrap()
                .iter()
                .take_while(|a| *a != "--")
                .any(|a| a == "--no-config")
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
        .map(T::from_str)
        // collect into HashSet to remove duplicates
        .collect::<Result<BTreeSet<_>, _>>()
        .map(|set| set.into_iter().collect())
}
