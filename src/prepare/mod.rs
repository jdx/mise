use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::PathBuf;

use async_trait::async_trait;
use eyre::Result;

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

/// Trait for prepare providers that can check and install dependencies
#[async_trait]
pub trait PrepareProvider: Debug + Send + Sync {
    /// Unique identifier for this provider (e.g., "npm", "cargo", "codegen")
    fn id(&self) -> &str;

    /// Returns the source files to check for freshness (lock files, config files)
    /// These are the files that, when modified, indicate dependencies may need updating
    fn sources(&self) -> Vec<PathBuf>;

    /// Returns the output files/directories that should be newer than sources
    /// These indicate that dependencies have been installed
    fn outputs(&self) -> Vec<PathBuf>;

    /// The command to run when outputs are stale relative to sources
    fn prepare_command(&self) -> Result<PrepareCommand>;

    /// Whether this provider is applicable to the current project
    /// (e.g., npm provider is applicable if package-lock.json exists)
    fn is_applicable(&self) -> bool;

    /// Whether this provider should auto-run before mise x/run (default: false)
    fn is_auto(&self) -> bool {
        false
    }

    /// Priority - higher priority providers run first (default: 100)
    fn priority(&self) -> u32 {
        100
    }
}
