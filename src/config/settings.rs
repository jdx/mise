use std::collections::{BTreeSet, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::iter::once;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

#[allow(unused_imports)]
use confique::env::parse::{list_by_colon, list_by_comma};
use confique::{Config, Partial};
use eyre::Result;
use once_cell::sync::Lazy;
use serde::ser::Error;
use serde_derive::{Deserialize, Serialize};

use crate::config::{system_config_files, DEFAULT_CONFIG_FILENAMES};
use crate::file::FindUp;
use crate::{config, dirs, env, file};

#[rustfmt::skip]
#[derive(Config, Default, Debug, Clone, Serialize)]
#[config(partial_attr(derive(Clone, Serialize, Default)))]
#[config(partial_attr(serde(deny_unknown_fields)))]
pub struct Settings {
    /// push tools to the front of PATH instead of allowing modifications of PATH after activation to take precedence
    #[config(env = "MISE_ACTIVATE_AGGRESSIVE", default = false)]
    pub activate_aggressive: bool,
    #[config(env = "MISE_ALL_COMPILE", default = false)]
    pub all_compile: bool,
    #[config(env = "MISE_ALWAYS_KEEP_DOWNLOAD", default = false)]
    pub always_keep_download: bool,
    #[config(env = "MISE_ALWAYS_KEEP_INSTALL", default = false)]
    pub always_keep_install: bool,
    /// default to asdf-compatible behavior
    /// this means that the global config file will be ~/.tool-versions
    /// also, the default behavior of `mise global` will be --pin
    #[config(env = "MISE_ASDF_COMPAT", default = false)]
    pub asdf_compat: bool,
    /// use cargo-binstall instead of cargo install if available
    #[config(env = "MISE_CARGO_BINSTALL", default = true)]
    pub cargo_binstall: bool,
    #[config(env = "MISE_COLOR", default = true)]
    pub color: bool,
    #[config(env = "MISE_DISABLE_DEFAULT_SHORTHANDS", default = false)]
    pub disable_default_shorthands: bool,
    #[config(env = "MISE_DISABLE_TOOLS", default = [], parse_env = list_by_comma)]
    pub disable_tools: BTreeSet<String>,
    #[config(env = "MISE_EXPERIMENTAL", default = false)]
    pub experimental: bool,
    /// after installing a go version, run `go install` on packages listed in this file
    #[config(env = "MISE_GO_DEFAULT_PACKAGES_FILE", default = "~/.default-go-packages")]
    pub go_default_packages_file: PathBuf,
    /// url to fetch go sdks from
    #[config(env = "MISE_GO_DOWNLOAD_MIRROR", default = "https://dl.google.com/go")]
    pub go_download_mirror: String,
    /// used for fetching go versions
    #[config(env = "MISE_GO_REPO", default = "https://github.com/golang/go")]
    pub go_repo: String,
    /// changes where `go install` installs binaries to
    /// defaults to ~/.local/share/mise/installs/go/.../bin
    /// set to true to override GOBIN if previously set
    /// set to false to not set GOBIN (default is ${GOPATH:-$HOME/go}/bin)
    #[config(env = "MISE_GO_SET_GOBIN")]
    pub go_set_gobin: Option<bool>,
    /// [deprecated] set to true to set GOPATH=~/.local/share/mise/installs/go/.../packages
    /// use to make mise behave like asdf but there are no known use-cases where this is necessary.
    /// See https://github.com/jdx/mise/discussions/1638
    #[config(env = "MISE_GO_SET_GOPATH", default = false)]
    pub go_set_gopath: bool,
    /// sets GOROOT=~/.local/share/mise/installs/go/.../
    /// you probably always want this set to be set unless you want GOROOT to point to something
    /// other than the sdk mise is currently set to
    #[config(env = "MISE_GO_SET_GOROOT", default = true)]
    pub go_set_goroot: bool,
    /// set to true to skip checksum verification when downloading go sdk tarballs
    #[config(env = "MISE_GO_SKIP_CHECKSUM", default = false)]
    pub go_skip_checksum: bool,
    #[config(env = "MISE_JOBS", default = 4)]
    pub jobs: usize,
    #[config(env = "MISE_LEGACY_VERSION_FILE", default = true)]
    pub legacy_version_file: bool,
    #[config(env = "MISE_LEGACY_VERSION_FILE_DISABLE_TOOLS", default = [], parse_env = list_by_comma)]
    pub legacy_version_file_disable_tools: BTreeSet<String>,
    #[config(env = "MISE_NODE_COMPILE", default = false)]
    pub node_compile: bool,
    #[config(env = "MISE_NOT_FOUND_AUTO_INSTALL", default = true)]
    pub not_found_auto_install: bool,
    #[config(env = "MISE_PARANOID", default = false)]
    pub paranoid: bool,
    #[config(env = "MISE_PLUGIN_AUTOUPDATE_LAST_CHECK_DURATION", default = "7d")]
    pub plugin_autoupdate_last_check_duration: String,
    #[config(env = "MISE_PYTHON_COMPILE", default = false)]
    pub python_compile: bool,
    #[config(env = "MISE_PYTHON_DEFAULT_PACKAGES_FILE")]
    pub python_default_packages_file: Option<PathBuf>,
    #[config(env = "MISE_PYTHON_PATCH_URL")]
    pub python_patch_url: Option<String>,
    #[config(env = "MISE_PYTHON_PATCHES_DIRECTORY")]
    pub python_patches_directory: Option<PathBuf>,
    #[config(env = "MISE_PYTHON_PRECOMPILED_ARCH")]
    pub python_precompiled_arch: Option<String>,
    #[config(env = "MISE_PYTHON_PRECOMPILED_OS")]
    pub python_precompiled_os: Option<String>,
    #[config(env = "MISE_PYENV_REPO", default = "https://github.com/pyenv/pyenv.git")]
    pub python_pyenv_repo: String,
    #[config(env = "MISE_RAW", default = false)]
    pub raw: bool,
    #[config(env = "MISE_SHORTHANDS_FILE")]
    pub shorthands_file: Option<PathBuf>,
    /// what level of status messages to display when entering directories
    #[config(nested)]
    pub status: SettingsStatus,
    #[config(env = "MISE_TASK_OUTPUT")]
    pub task_output: Option<String>,
    #[config(env = "MISE_TRUSTED_CONFIG_PATHS", default = [], parse_env = list_by_colon)]
    pub trusted_config_paths: BTreeSet<PathBuf>,
    #[config(env = "MISE_QUIET", default = false)]
    pub quiet: bool,
    #[config(env = "MISE_VERBOSE", default = false)]
    pub verbose: bool,
    #[config(env = "MISE_YES", default = false)]
    pub yes: bool,

    // hidden settings
    #[config(env = "CI", default = false)]
    pub ci: bool,
    #[config(env = "MISE_CD")]
    pub cd: Option<PathBuf>,
    #[config(env = "MISE_DEBUG", default = false)]
    pub debug: bool,
    #[config(env = "MISE_ENV_FILE")]
    pub env_file: Option<PathBuf>,
    #[config(env = "MISE_TRACE", default = false)]
    pub trace: bool,
    #[config(env = "MISE_LOG_LEVEL", default = "info")]
    pub log_level: String,
    #[config(env = "MISE_PYTHON_VENV_AUTO_CREATE", default = false)]
    pub python_venv_auto_create: bool,
}

#[derive(Config, Default, Debug, Clone, Serialize)]
#[config(partial_attr(derive(Clone, Serialize, Default)))]
#[config(partial_attr(serde(deny_unknown_fields)))]
pub struct SettingsStatus {
    /// warn if a tool is missing
    #[config(
        env = "MISE_STATUS_MESSAGE_MISSING_TOOLS",
        default = "if_other_versions_installed"
    )]
    pub missing_tools: SettingsStatusMissingTools,
    /// show env var keys when entering directories
    #[config(env = "MISE_STATUS_MESSAGE_SHOW_ENV", default = false)]
    pub show_env: bool,
    /// show active tools when entering directories
    #[config(env = "MISE_STATUS_MESSAGE_SHOW_TOOLS", default = false)]
    pub show_tools: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, EnumString, Display)]
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

static SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);
static CLI_SETTINGS: Mutex<Option<SettingsPartial>> = Mutex::new(None);
static DEFAULT_SETTINGS: Lazy<SettingsPartial> = Lazy::new(|| {
    let mut s = SettingsPartial::empty();
    s.python_default_packages_file = Some(env::HOME.join(".default-python-packages"));
    if let Some("alpine" | "nixos") = env::LINUX_DISTRO.as_ref().map(|s| s.as_str()) {
        if !cfg!(test) {
            s.all_compile = Some(true);
        }
    }
    s
});

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
        if let Some(settings) = SETTINGS.read().unwrap().as_ref() {
            return Ok(settings.clone());
        }

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
        if settings.verbose {
            settings.quiet = false;
            if settings.log_level != "trace" {
                settings.log_level = "debug".to_string();
            }
        }
        if !settings.color {
            console::set_colors_enabled(false);
            console::set_colors_enabled_stderr(false);
        }
        if settings.ci {
            settings.yes = true;
        }
        if settings.all_compile {
            settings.node_compile = true;
            settings.python_compile = true;
        }
        let settings = Arc::new(settings);
        *SETTINGS.write().unwrap() = Some(settings.clone());
        Ok(settings)
    }
    pub fn add_cli_matches(m: &clap::ArgMatches) {
        let mut s = SettingsPartial::empty();
        for arg in &*env::ARGS.read().unwrap() {
            if arg == "--" {
                break;
            }
            if arg == "--raw" {
                s.raw = Some(true);
            }
        }
        if let Some(cd) = m.get_one::<PathBuf>("cd") {
            s.cd = Some(cd.clone());
        }
        if let Some(true) = m.get_one::<bool>("yes") {
            s.yes = Some(true);
        }
        if let Some(true) = m.get_one::<bool>("quiet") {
            s.quiet = Some(true);
        }
        if let Some(true) = m.get_one::<bool>("trace") {
            s.log_level = Some("trace".to_string());
        }
        if let Some(true) = m.get_one::<bool>("debug") {
            s.log_level = Some("debug".to_string());
        }
        if let Some(log_level) = m.get_one::<String>("log-level") {
            s.log_level = Some(log_level.to_string());
        }
        if *m.get_one::<u8>("verbose").unwrap() > 0 {
            s.verbose = Some(true);
        }
        if *m.get_one::<u8>("verbose").unwrap() > 1 {
            s.log_level = Some("trace".to_string());
        }
        Self::reset(Some(s));
    }

    fn config_settings() -> Result<SettingsPartial> {
        let global_config = &*env::MISE_GLOBAL_CONFIG_FILE;
        let filename = global_config
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        // if the file doesn't exist or is actually a .tool-versions config
        if !global_config.exists()
            || filename == *env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME
            || filename == ".tool-versions"
        {
            return Ok(Default::default());
        }
        Self::parse_settings_file(global_config)
    }

    fn deprecated_settings_file() -> Result<SettingsPartial> {
        // TODO: show warning and merge with config file in a few weeks
        let settings_file = &*env::MISE_SETTINGS_FILE;
        if !settings_file.exists() {
            return Ok(Default::default());
        }
        Self::from_file(settings_file)
    }

    fn parse_settings_file(path: &PathBuf) -> Result<SettingsPartial> {
        let raw = file::read_to_string(path)?;
        let settings_file: SettingsFile = toml::from_str(&raw)?;
        Ok(settings_file.settings)
    }

    fn all_settings_files() -> Vec<SettingsPartial> {
        config::load_config_paths(&DEFAULT_CONFIG_FILENAMES)
            .iter()
            .filter(|p| {
                let filename = p.file_name().unwrap_or_default().to_string_lossy();
                filename != *env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME
                    && filename != ".tool-versions"
            })
            .map(Self::parse_settings_file)
            .chain(once(Self::config_settings()))
            .chain(once(Self::deprecated_settings_file()))
            .chain(system_config_files().iter().map(Self::parse_settings_file))
            .filter_map(|cfg| match cfg {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    eprintln!("Error loading settings file: {}", e);
                    None
                }
            })
            .collect()
    }

    pub fn from_file(path: &PathBuf) -> Result<SettingsPartial> {
        let raw = file::read_to_string(path)?;
        let settings: SettingsPartial = toml::from_str(&raw)?;
        Ok(settings)
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
        *SETTINGS.write().unwrap() = None;
    }

    pub fn ensure_experimental(&self, what: &str) -> Result<()> {
        if !self.experimental {
            bail!("{what} is experimental. Enable it with `mise settings set experimental true`");
        }
        Ok(())
    }

    pub fn trusted_config_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.trusted_config_paths.iter().map(file::replace_path)
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
        Ok(self.to_string().parse()?)
    }
}

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match toml::to_string_pretty(self) {
            Ok(s) => write!(f, "{}", s),
            Err(e) => Err(std::fmt::Error::custom(e)),
        }
    }
}
