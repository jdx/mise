use std::collections::{BTreeMap, HashSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use eyre::{Result, bail};

use crate::config::{Config, Settings};
use crate::env;

pub use engine::{DepsEngine, DepsOptions, DepsStepResult};
pub use rule::DepsConfig;

pub(crate) mod deps_ordering;
mod engine;
pub mod providers;
mod rule;
pub mod state;

/// Result of a freshness check for a deps provider
#[derive(Debug, Clone)]
pub enum FreshnessResult {
    /// Outputs are up to date with sources
    Fresh,
    /// One or more output paths don't exist
    OutputsMissing,
    /// Sources have changed since last successful run
    Stale(String),
    /// Provider has no sources, consider fresh
    NoSources,
    /// Force flag was used
    Forced,
}

impl FreshnessResult {
    /// Returns true if the provider should be considered fresh (no work needed)
    pub fn is_fresh(&self) -> bool {
        matches!(self, FreshnessResult::Fresh | FreshnessResult::NoSources)
    }

    /// Human-readable reason string for display
    pub fn reason(&self) -> &str {
        match self {
            FreshnessResult::Fresh => "outputs are up to date",
            FreshnessResult::OutputsMissing => "outputs missing",
            FreshnessResult::Stale(reason) => reason,
            FreshnessResult::NoSources => "no sources to check",
            FreshnessResult::Forced => "forced",
        }
    }
}

/// A command to execute for dependency management
#[derive(Debug, Clone)]
pub struct DepsCommand {
    /// The program to execute
    pub program: String,
    /// Arguments to pass to the program
    pub args: Vec<String>,
    /// Environment variables to set
    pub env: BTreeMap<String, String>,
    /// Working directory (defaults to project root)
    pub cwd: Option<PathBuf>,
    /// Human-readable description of what this command does
    pub description: String,
}

impl DepsCommand {
    /// Create a DepsCommand from a run string like "npm install"
    ///
    /// Wraps the command with `sh -c` (matching task execution behavior)
    /// so shell features like pipes, redirects, and `&&` work.
    pub fn from_string(
        run: &str,
        project_root: &Path,
        config: &rule::DepsProviderConfig,
    ) -> Result<Self> {
        if run.trim().is_empty() {
            bail!("deps run command cannot be empty");
        }

        let shell = Settings::get().default_inline_shell()?;
        let (program, shell_args) = shell.split_first().ok_or_else(|| {
            eyre::eyre!("default inline shell is empty; check unix_default_inline_shell_args / windows_default_inline_shell_args")
        })?;

        let mut args: Vec<String> = shell_args.to_vec();
        args.push(run.to_string());

        Ok(Self {
            program: program.to_string(),
            args,
            env: config.env.clone(),
            cwd: config
                .dir
                .as_ref()
                .map(|d| project_root.join(d))
                .or_else(|| Some(project_root.to_path_buf())),
            description: config
                .description
                .clone()
                .unwrap_or_else(|| run.to_string()),
        })
    }
}

/// Trait for deps providers that can check and install dependencies
pub trait DepsProvider: Debug + Send + Sync {
    /// Access the shared base (project root + config)
    fn base(&self) -> &providers::ProviderBase;

    /// Unique identifier for this provider (e.g., "npm", "cargo", "codegen")
    fn id(&self) -> &str {
        &self.base().id
    }

    /// Returns the source files to check for freshness (lock files, config files)
    fn sources(&self) -> Vec<PathBuf>;

    /// Returns the output files/directories that should be newer than sources
    fn outputs(&self) -> Vec<PathBuf>;

    /// The command to run when outputs are stale relative to sources
    fn install_command(&self) -> Result<DepsCommand>;

    /// Whether this provider is applicable (e.g., lockfile exists)
    fn is_applicable(&self) -> bool;

    /// Whether this provider should auto-run before mise x/run
    fn is_auto(&self) -> bool {
        self.base().is_auto()
    }

    /// Other deps providers that must complete before this one runs
    fn depends(&self) -> Vec<String> {
        self.base().config.depends.clone()
    }

    /// Timeout duration for this provider's run command
    fn timeout(&self) -> Option<std::time::Duration> {
        self.base().config.timeout.as_deref().and_then(|t| {
            match crate::duration::parse_duration(t) {
                Ok(d) => Some(d),
                Err(err) => {
                    warn!("deps: {}: invalid timeout {t:?}: {err}", self.id());
                    None
                }
            }
        })
    }

    /// Command to add one or more package dependencies
    fn add_command(&self, _packages: &[&str], _dev: bool) -> Result<DepsCommand> {
        bail!("provider '{}' does not support adding packages", self.id())
    }

    /// Command to remove one or more package dependencies
    fn remove_command(&self, _packages: &[&str]) -> Result<DepsCommand> {
        bail!(
            "provider '{}' does not support removing packages",
            self.id()
        )
    }
}

/// Warn if any auto-enabled deps providers are stale
pub fn notify_if_stale(config: &Arc<Config>) {
    // Skip in shims or quiet mode
    if *env::__MISE_SHIM || Settings::get().quiet {
        return;
    }

    // Check if this feature is enabled
    if !Settings::get().status.show_deps_stale {
        return;
    }

    let Ok(engine) = DepsEngine::new(config) else {
        return;
    };

    let stale = engine.check_staleness();
    if !stale.is_empty() {
        let providers: Vec<String> = stale
            .iter()
            .map(|(id, reason)| format!("{id} ({reason})"))
            .collect();
        let summary = providers.join(", ");
        warn!("deps: {summary} — run `mise deps`");
    }
}

/// Tracks directories created during this session that should be considered stale
/// for deps freshness checks (e.g., venvs auto-created before deps runs)
static STALE_OUTPUTS: LazyLock<Mutex<HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Mark a directory as freshly created (stale for deps purposes)
pub fn mark_output_stale(path: PathBuf) {
    if let Ok(mut set) = STALE_OUTPUTS.lock() {
        set.insert(path);
    }
}

/// Check if a directory was created this session
pub fn is_output_stale(path: &PathBuf) -> bool {
    STALE_OUTPUTS
        .lock()
        .map(|set| set.contains(path))
        .unwrap_or(false)
}

/// Clear stale status for a path (after deps runs successfully)
pub fn clear_output_stale(path: &PathBuf) {
    if let Ok(mut set) = STALE_OUTPUTS.lock() {
        set.remove(path);
    }
}

/// Detect which built-in deps providers are applicable for a given directory
///
/// This checks if the lockfiles/config files for each provider exist.
pub fn detect_applicable_providers(project_root: &Path) -> Vec<String> {
    use providers::*;
    use rule::DepsProviderConfig;

    let default_config = DepsProviderConfig::default();
    let mut applicable = Vec::new();

    // Check each built-in provider
    let checks: &[(&str, Box<dyn DepsProvider>)] = &[
        (
            "npm",
            Box::new(NpmDepsProvider::new(project_root, default_config.clone())),
        ),
        (
            "yarn",
            Box::new(YarnDepsProvider::new(project_root, default_config.clone())),
        ),
        (
            "pnpm",
            Box::new(PnpmDepsProvider::new(project_root, default_config.clone())),
        ),
        (
            "bun",
            Box::new(BunDepsProvider::new(project_root, default_config.clone())),
        ),
        (
            "aube",
            Box::new(AubeDepsProvider::new(project_root, default_config.clone())),
        ),
        (
            "go",
            Box::new(GoDepsProvider::new(project_root, default_config.clone())),
        ),
        (
            "pip",
            Box::new(PipDepsProvider::new(project_root, default_config.clone())),
        ),
        (
            "poetry",
            Box::new(PoetryDepsProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "uv",
            Box::new(UvDepsProvider::new(project_root, default_config.clone())),
        ),
        (
            "bundler",
            Box::new(BundlerDepsProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "composer",
            Box::new(ComposerDepsProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "git-submodule",
            Box::new(GitSubmoduleDepsProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
    ];

    for (name, provider) in checks {
        if provider.is_applicable() {
            applicable.push(name.to_string());
        }
    }

    applicable
}

/// Create a provider for add/remove operations.
///
/// If a `Config` is provided, looks up user-defined settings (env, dir, timeout)
/// from the `[deps.<ecosystem>]` section. Falls back to defaults otherwise.
pub fn create_provider(
    ecosystem: &str,
    project_root: &Path,
    config: Option<&crate::config::Config>,
) -> Result<Box<dyn DepsProvider>> {
    let (provider_root, provider_config) = config
        .and_then(|c| {
            c.config_files.values().find_map(|cf| {
                cf.deps_config()
                    .and_then(|dc| dc.providers.get(ecosystem).cloned())
                    .map(|provider_config| (cf.config_root(), provider_config))
            })
        })
        .unwrap_or_else(|| {
            (
                project_root.to_path_buf(),
                rule::DepsProviderConfig::default(),
            )
        });

    DepsEngine::build_provider(ecosystem, &provider_root, provider_config)
        .ok_or_else(|| eyre::eyre!("unknown deps provider '{ecosystem}'"))
}
