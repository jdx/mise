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
/// Supports Homebrew formulae and conservatively removable, mise-owned casks.
/// Pruning keeps packages needed by the current config or by trusted, loadable
/// tracked configs.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemPrune {
    /// Only prune packages for this manager
    #[clap(long, short, default_value = "brew", value_parser = ["brew", "brew-cask"])]
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
        match self.manager.as_str() {
            "brew" => self.run_brew().await,
            "brew-cask" => self.run_brew_cask().await,
            _ => unreachable!(),
        }
    }

    #[cfg(unix)]
    async fn run_brew(self) -> Result<()> {
        debug_assert_eq!(self.manager, "brew");
        let manager = brew::BrewManager::new();
        if !manager.is_available() {
            bail!("brew is not available: {}", manager.unavailable_reason());
        }
        let config = Config::get().await?;
        let configured = system::packages_from_config_and_tracked_config_files(&config)
            .await?
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
        if !self.yes && !Settings::get().yes && console::user_attended_stderr() {
            let msg = format!("brew: prune {}?", remove.join(", "));
            if !prompt::confirm(msg)? {
                info!("brew: skipped");
                return Ok(());
            }
        }
        let removed = plan.remove.len();
        brew::apply_prune_plan(&plan, false)?;
        info!("brew: pruned {removed} formulae");
        Ok(())
    }

    #[cfg(unix)]
    async fn run_brew_cask(self) -> Result<()> {
        debug_assert_eq!(self.manager, "brew-cask");
        let manager = brew::BrewCaskManager::new();
        if !manager.is_available() {
            bail!(
                "brew-cask is not available: {}",
                manager.unavailable_reason()
            );
        }
        let config = Config::get().await?;
        let configured = system::packages_from_config_and_tracked_config_files(&config)
            .await?
            .into_iter()
            .find(|mp| mp.manager.name() == "brew-cask")
            .map(|mp| mp.requests)
            .unwrap_or_default();
        let plan = brew::cask_prune_plan(&configured).await?;
        for skipped in &plan.skipped {
            warn!("brew-cask:{}: skipped: {}", skipped.token, skipped.reason);
        }
        if plan.is_empty() {
            info!("brew-cask: nothing to prune");
            return Ok(());
        }
        if self.dry_run {
            brew::apply_cask_prune_plan(&plan, true)?;
            return Ok(());
        }
        let remove = plan
            .remove
            .iter()
            .map(|candidate| format!("{}@{}", candidate.token, candidate.version))
            .collect::<Vec<_>>();
        if !self.yes && !Settings::get().yes && console::user_attended_stderr() {
            let msg = format!("brew-cask: prune {}?", remove.join(", "));
            if !prompt::confirm(msg)? {
                info!("brew-cask: skipped");
                return Ok(());
            }
        }
        let removed = brew::apply_cask_prune_plan(&plan, false)?;
        info!("brew-cask: pruned {removed} casks");
        Ok(())
    }

    #[cfg(not(unix))]
    async fn run_brew(self) -> Result<()> {
        let _ = self.manager;
        bail!("brew prune is not supported on windows")
    }

    #[cfg(not(unix))]
    async fn run_brew_cask(self) -> Result<()> {
        let _ = self.manager;
        bail!("brew-cask prune is not supported on windows")
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages prune --manager brew</bold>
    $ <bold>mise bootstrap packages prune --manager brew --dry-run</bold>
    $ <bold>mise bootstrap packages prune --manager brew --yes</bold>
    $ <bold>mise bootstrap packages prune --manager brew-cask --dry-run</bold>
"#
);
