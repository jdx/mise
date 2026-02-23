use std::collections::{BTreeMap, HashSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use eyre::{Result, bail};

use crate::config::{Config, Settings};
use crate::env;

pub use engine::{PrepareEngine, PrepareOptions, PrepareStepResult};
pub use rule::PrepareConfig;

mod engine;
pub mod providers;
mod rule;

/// A command to execute for preparation
#[derive(Debug, Clone)]
pub struct PrepareCommand {
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

impl PrepareCommand {
    /// Create a PrepareCommand from a run string like "npm install"
    ///
    /// Uses shell-aware parsing to handle quoted arguments correctly.
    pub fn from_string(
        run: &str,
        project_root: &Path,
        config: &rule::PrepareProviderConfig,
    ) -> Result<Self> {
        let parts = shell_words::split(run).map_err(|e| eyre::eyre!("invalid command: {e}"))?;

        if parts.is_empty() {
            bail!("prepare run command cannot be empty");
        }

        let (program, args) = parts.split_first().unwrap();

        Ok(Self {
            program: program.to_string(),
            args: args.to_vec(),
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

/// Trait for prepare providers that can check and install dependencies
pub trait PrepareProvider: Debug + Send + Sync {
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
    fn prepare_command(&self) -> Result<PrepareCommand>;

    /// Whether this provider is applicable (e.g., lockfile exists)
    fn is_applicable(&self) -> bool;

    /// Whether this provider should auto-run before mise x/run
    fn is_auto(&self) -> bool {
        self.base().is_auto()
    }

    /// Whether to update mtime of output files/dirs after a successful run
    fn touch_outputs(&self) -> bool {
        self.base().touch_outputs()
    }
}

/// Warn if any auto-enabled prepare providers are stale
pub fn notify_if_stale(config: &Arc<Config>) {
    // Skip in shims or quiet mode
    if *env::__MISE_SHIM || Settings::get().quiet {
        return;
    }

    // Check if this feature is enabled
    if !Settings::get().status.show_prepare_stale {
        return;
    }

    let Ok(engine) = PrepareEngine::new(config) else {
        return;
    };

    let stale = engine.check_staleness();
    if !stale.is_empty() {
        let providers = stale.join(", ");
        warn!("prepare: {providers} may need update, run `mise prep`");
    }
}

/// Tracks directories created during this session that should be considered stale
/// for prepare freshness checks (e.g., venvs auto-created before prepare runs)
static STALE_OUTPUTS: LazyLock<Mutex<HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Mark a directory as freshly created (stale for prepare purposes)
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

/// Clear stale status for a path (after prepare runs successfully)
pub fn clear_output_stale(path: &PathBuf) {
    if let Ok(mut set) = STALE_OUTPUTS.lock() {
        set.remove(path);
    }
}

/// Detect which built-in prepare providers are applicable for a given directory
///
/// This checks if the lockfiles/config files for each provider exist.
pub fn detect_applicable_providers(project_root: &Path) -> Vec<String> {
    use providers::*;
    use rule::PrepareProviderConfig;

    let default_config = PrepareProviderConfig::default();
    let mut applicable = Vec::new();

    // Check each built-in provider
    let checks: &[(&str, Box<dyn PrepareProvider>)] = &[
        (
            "npm",
            Box::new(NpmPrepareProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "yarn",
            Box::new(YarnPrepareProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "pnpm",
            Box::new(PnpmPrepareProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "bun",
            Box::new(BunPrepareProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "go",
            Box::new(GoPrepareProvider::new(project_root, default_config.clone())),
        ),
        (
            "pip",
            Box::new(PipPrepareProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "poetry",
            Box::new(PoetryPrepareProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "uv",
            Box::new(UvPrepareProvider::new(project_root, default_config.clone())),
        ),
        (
            "bundler",
            Box::new(BundlerPrepareProvider::new(
                project_root,
                default_config.clone(),
            )),
        ),
        (
            "composer",
            Box::new(ComposerPrepareProvider::new(
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
