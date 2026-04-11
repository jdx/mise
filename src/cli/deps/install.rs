use eyre::{Result, bail};

use crate::config::Config;
use crate::deps::{DepsEngine, DepsOptions, DepsStepResult};
use crate::toolset::{InstallOptions, ToolsetBuilder};

/// Install all project dependencies
///
/// Checks if dependency lockfiles are newer than installed outputs
/// and runs install commands if needed.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct DepsInstall {
    /// Provider to operate on (runs only this provider, or use with --explain)
    pub provider: Option<String>,

    /// Show why a provider is fresh or stale (requires a provider argument)
    #[clap(long)]
    pub explain: bool,

    /// Force run all deps steps even if outputs are fresh
    #[clap(long, short)]
    pub force: bool,

    /// Only check if deps install is needed, don't run commands
    #[clap(long, short = 'n')]
    pub dry_run: bool,

    /// Show what deps providers are available
    #[clap(long)]
    pub list: bool,

    /// Run specific deps rule(s) only
    #[clap(long)]
    pub only: Option<Vec<String>>,

    /// Skip specific deps rule(s)
    #[clap(long)]
    pub skip: Option<Vec<String>>,
}

impl DepsInstall {
    pub async fn run(self) -> Result<()> {
        let mut config = Config::get().await?;
        let engine = DepsEngine::new(&config)?;

        if self.list {
            self.list_providers(&engine)?;
            return Ok(());
        }

        if self.explain {
            let Some(ref provider_id) = self.provider else {
                bail!(
                    "--explain requires a provider argument, e.g.: mise deps install npm --explain"
                );
            };
            return self.explain_provider(&engine, provider_id);
        }

        // Build and install toolset so tools like npm are available
        let mut ts = ToolsetBuilder::new()
            .with_default_to_latest(true)
            .build(&config)
            .await?;

        let install_opts = InstallOptions {
            missing_args_only: false,
            ..Default::default()
        };
        ts.install_missing_versions(&mut config, &install_opts)
            .await?;

        // Get toolset environment with PATH
        let env = ts.env_with_path(&config).await?;

        // If a provider is specified as a positional arg, treat it like --only
        let only = match (&self.provider, &self.only) {
            (Some(p), None) => Some(vec![p.clone()]),
            (Some(p), Some(list)) => {
                let mut combined = list.clone();
                if !combined.contains(p) {
                    combined.push(p.clone());
                }
                Some(combined)
            }
            (None, only) => only.clone(),
        };

        let opts = DepsOptions {
            dry_run: self.dry_run,
            force: self.force,
            only,
            skip: self.skip.unwrap_or_default(),
            env,
            ..Default::default()
        };

        let result = engine.run(opts).await?;

        // Report results
        for step in &result.steps {
            match step {
                DepsStepResult::Ran(id) => {
                    info!("Installed: {}", id);
                }
                DepsStepResult::WouldRun(id, reason) => {
                    info!("[dry-run] Would install: {} ({})", id, reason);
                }
                DepsStepResult::Fresh(id) => {
                    debug!("Fresh: {}", id);
                }
                DepsStepResult::Skipped(id) => {
                    debug!("Skipped: {}", id);
                }
                DepsStepResult::Failed(id) => {
                    error!("Failed: {}", id);
                }
            }
        }

        if !result.had_work() && !self.dry_run {
            info!("All dependencies are up to date");
        }

        Ok(())
    }

    fn explain_provider(&self, engine: &DepsEngine, provider_id: &str) -> Result<()> {
        let Some(provider) = engine.find_provider(provider_id) else {
            let available = engine
                .list_providers()
                .iter()
                .map(|p| format!("  {}", p.id()))
                .collect::<Vec<_>>()
                .join("\n");
            bail!("Provider '{provider_id}' not found.\n\nAvailable providers:\n{available}");
        };

        let freshness = engine.check_provider_freshness(provider)?;

        // Header
        miseprintln!("Provider: {}", provider.id());
        miseprintln!("Auto: {}", if provider.is_auto() { "yes" } else { "no" });

        // Sources
        let sources = provider.sources();
        miseprintln!("Sources:");
        for source in &sources {
            let exists = source.exists();
            let marker = if exists { "+" } else { "-" };
            miseprintln!("  {} {}", marker, source.display());
        }

        // Outputs
        let outputs = provider.outputs();
        miseprintln!("Outputs:");
        for output in &outputs {
            let exists = output.exists();
            let marker = if exists { "+" } else { "-" };
            miseprintln!("  {} {}", marker, output.display());
        }

        // Command
        if let Ok(cmd) = provider.install_command() {
            miseprintln!("Command: {}", cmd.description);
        }

        // Verdict
        miseprintln!("");
        if freshness.is_fresh() {
            miseprintln!("Status: fresh ({})", freshness.reason());
        } else {
            miseprintln!("Status: stale ({})", freshness.reason());
        }

        if !freshness.is_fresh() {
            bail!("provider '{}' is stale", provider.id());
        }

        Ok(())
    }

    fn list_providers(&self, engine: &DepsEngine) -> Result<()> {
        let providers = engine.list_providers();

        if providers.is_empty() {
            miseprintln!("No deps providers found for this project");
            return Ok(());
        }

        miseprintln!("Available deps providers:");
        for provider in providers {
            let sources = provider
                .sources()
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let outputs = provider
                .outputs()
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");

            miseprintln!("  {}", provider.id());
            miseprintln!("    sources: {}", sources);
            miseprintln!("    outputs: {}", outputs);
        }

        Ok(())
    }
}
