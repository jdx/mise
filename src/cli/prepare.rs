use eyre::{Result, bail};

use crate::config::Config;
use crate::prepare::{PrepareEngine, PrepareOptions, PrepareStepResult};
use crate::toolset::{InstallOptions, ToolsetBuilder};

/// [experimental] Ensure project dependencies are ready
///
/// Runs all applicable prepare steps for the current project.
/// This checks if dependency lockfiles are newer than installed outputs
/// (e.g., package-lock.json vs node_modules/) and runs install commands
/// if needed.
///
/// Providers with `auto = true` are automatically invoked before `mise x` and `mise run`
/// unless skipped with the --no-prepare flag.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "prep", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Prepare {
    /// Provider to operate on (runs only this provider, or use with --explain)
    pub provider: Option<String>,

    /// Show why a provider is fresh or stale (requires a provider argument)
    #[clap(long)]
    pub explain: bool,

    /// Force run all prepare steps even if outputs are fresh
    #[clap(long, short)]
    pub force: bool,

    /// Only check if prepare is needed, don't run commands
    #[clap(long, short = 'n')]
    pub dry_run: bool,

    /// Show what prepare steps are available
    #[clap(long)]
    pub list: bool,

    /// Run specific prepare rule(s) only
    #[clap(long)]
    pub only: Option<Vec<String>>,

    /// Skip specific prepare rule(s)
    #[clap(long)]
    pub skip: Option<Vec<String>>,
}

impl Prepare {
    pub async fn run(self) -> Result<()> {
        let mut config = Config::get().await?;
        let engine = PrepareEngine::new(&config)?;

        if self.list {
            self.list_providers(&engine)?;
            return Ok(());
        }

        if self.explain {
            let Some(ref provider_id) = self.provider else {
                bail!("--explain requires a provider argument, e.g.: mise prepare npm --explain");
            };
            return self.explain_provider(&engine, provider_id);
        }

        // Build and install toolset so tools like npm are available
        let mut ts = ToolsetBuilder::new()
            .with_default_to_latest(true)
            .build(&config)
            .await?;

        ts.install_missing_versions(&mut config, &InstallOptions::default())
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

        let opts = PrepareOptions {
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
                PrepareStepResult::Ran(id) => {
                    miseprintln!("Prepared: {}", id);
                }
                PrepareStepResult::WouldRun(id, reason) => {
                    miseprintln!("[dry-run] Would prepare: {} ({})", id, reason);
                }
                PrepareStepResult::Fresh(id) => {
                    debug!("Fresh: {}", id);
                }
                PrepareStepResult::Skipped(id) => {
                    debug!("Skipped: {}", id);
                }
                PrepareStepResult::Failed(id) => {
                    miseprintln!("Failed: {}", id);
                }
            }
        }

        if !result.had_work() && !self.dry_run {
            miseprintln!("All dependencies are up to date");
        }

        Ok(())
    }

    fn explain_provider(&self, engine: &PrepareEngine, provider_id: &str) -> Result<()> {
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
        if let Ok(cmd) = provider.prepare_command() {
            miseprintln!("Command: {} {}", cmd.program, cmd.args.join(" "));
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

    fn list_providers(&self, engine: &PrepareEngine) -> Result<()> {
        let providers = engine.list_providers();

        if providers.is_empty() {
            miseprintln!("No prepare providers found for this project");
            return Ok(());
        }

        miseprintln!("Available prepare providers:");
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise prepare</bold>              # Run all applicable prepare steps
    $ <bold>mise prepare npm</bold>          # Run only npm prepare
    $ <bold>mise prepare npm --explain</bold> # Show why npm is fresh or stale
    $ <bold>mise prepare --dry-run</bold>    # Show what would run without executing
    $ <bold>mise prepare --force</bold>      # Force run even if outputs are fresh
    $ <bold>mise prepare --list</bold>       # List available prepare providers
    $ <bold>mise prepare --skip npm</bold>   # Skip npm prepare

<bold><underline>Configuration:</underline></bold>

    Configure prepare providers in mise.toml:

    ```toml
    # Built-in npm provider (auto-detects lockfile)
    [prepare.npm]
    auto = true              # Auto-run before mise x/run

    # Custom provider
    [prepare.codegen]
    auto = true
    sources = ["schema/*.graphql"]
    outputs = ["src/generated/"]
    run = "npm run codegen"

    [prepare]
    disable = ["npm"]        # Disable specific providers at runtime
    ```
"#
);
