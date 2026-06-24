use eyre::{Result, bail};

#[cfg(unix)]
use crate::config::Config;
use crate::config::Settings;
#[cfg(unix)]
use crate::system;
#[cfg(unix)]
use crate::system::packages::SystemPackageManager;
#[cfg(unix)]
use crate::system::packages::brew;
#[cfg(unix)]
use crate::ui::prompt;

/// Prune installed system packages no longer declared in `[bootstrap.packages]`
///
/// Currently supports Homebrew formulae only. Pruning is ledger-based: mise
/// removes only formulae it installed or adopted with
/// `mise bootstrap packages import`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemPrune {
    /// Only prune packages for this manager. Currently only `brew` is supported.
    #[clap(long, short, default_value = "brew", value_parser = ["brew"])]
    manager: String,

    /// Print what would be removed without deleting anything
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

impl SystemPrune {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        if Settings::get()
            .system_packages
            .managers
            .as_ref()
            .is_some_and(|enabled| !enabled.contains(&self.manager))
        {
            bail!(
                "manager '{}' is excluded by the system_packages.managers setting",
                self.manager
            );
        }
        self.run_brew().await
    }

    #[cfg(unix)]
    async fn run_brew(self) -> Result<()> {
        debug_assert_eq!(self.manager, "brew");
        let manager = brew::BrewManager::new();
        if !manager.is_available() {
            bail!("brew is not available: {}", manager.unavailable_reason());
        }
        let config = Config::get().await?;
        let configured = system::packages_from_config(&config)
            .into_iter()
            .find(|mp| mp.manager.name() == "brew")
            .map(|mp| mp.requests)
            .unwrap_or_default();
        let plan = brew::prune_plan(&configured).await?;
        if plan.is_empty() {
            info!("brew: nothing to prune");
            return Ok(());
        }
        if self.dry_run {
            brew::apply_prune_plan(&plan, true)?;
            return Ok(());
        }
        let remove = plan
            .remove
            .iter()
            .map(|c| format!("{}@{}", c.name, c.version))
            .collect::<Vec<_>>();
        let forget = plan
            .forget
            .iter()
            .map(|name| format!("{name} (ledger only)"))
            .collect::<Vec<_>>();
        let targets = remove.into_iter().chain(forget).collect::<Vec<_>>();
        if !self.yes && !Settings::get().yes && console::user_attended_stderr() {
            let msg = format!("brew: prune {}?", targets.join(", "));
            if !prompt::confirm(msg)? {
                info!("brew: skipped");
                return Ok(());
            }
        }
        let removed = plan.remove.len();
        let forgotten = plan.forget.len();
        brew::apply_prune_plan(&plan, false)?;
        if forgotten > 0 {
            info!("brew: pruned {removed} formulae and forgot {forgotten} stale ledger entries");
        } else {
            info!("brew: pruned {removed} formulae");
        }
        Ok(())
    }

    #[cfg(not(unix))]
    async fn run_brew(self) -> Result<()> {
        let _ = self.manager;
        bail!("brew prune is not supported on windows")
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages prune --manager brew</bold>
    $ <bold>mise bootstrap packages prune --manager brew --dry-run</bold>
    $ <bold>mise bootstrap packages prune --manager brew --yes</bold>
"#
);
