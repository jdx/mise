use eyre::Result;

use super::driver::{self, Action, DriverOpts};
use crate::config::{Config, Settings};
use crate::system;

/// Upgrade installed bootstrap packages from `[bootstrap.packages]`
///
/// Refreshes package manager metadata and upgrades the configured packages
/// that are already installed: apk/apt/dnf/pacman upgrade to the newest available
/// version (apk, apt, and dnf honor a version pinned in config), brew pours the
/// formula's current bottle and replaces the old keg, brew-cask installs
/// the current cask artifact, and mas upgrades App Store apps. Packages that
/// are not installed yet are skipped — use `mise bootstrap packages apply`
/// for those.
///
/// Packages can also be given explicitly in `manager:package` form.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "up", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemUpgrade {
    /// Packages in `manager:package` form; defaults to everything configured
    /// in [bootstrap.packages]
    #[clap(value_name = "PACKAGE")]
    packages: Vec<String>,

    /// Only upgrade packages for this manager, e.g. `apk`, `apt`, `brew`, `brew-cask`, or `mas`
    #[clap(long, short, value_parser = ["apk", "apt", "brew", "brew-cask", "dnf", "mas", "pacman"])]
    manager: Option<String>,

    /// Print the commands that would run without running them
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

impl SystemUpgrade {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        let mgrs = if self.packages.is_empty() {
            let config = Config::get().await?;
            system::packages_from_config(&config)
        } else {
            let config = Config::get().await?;
            system::packages_from_specs_with_config(&self.packages, Some(&config))?
        };
        let opts = DriverOpts {
            manager: self.manager,
            explicit: !self.packages.is_empty(),
            dry_run: self.dry_run,
            // upgrades refresh metadata themselves (stale lists would make
            // them silent no-ops), so no separate --update flag
            update: false,
            yes: self.yes,
        };
        driver::run(mgrs, Action::Upgrade, &opts).await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages upgrade</bold>
    $ <bold>mise bootstrap packages upgrade brew:postgresql@17</bold>
    $ <bold>mise bootstrap packages upgrade --manager brew-cask</bold>
    $ <bold>mise bootstrap packages upgrade --manager mas</bold>
    $ <bold>mise bootstrap packages upgrade --manager apt --yes</bold>
    $ <bold>mise bootstrap packages upgrade --dry-run</bold>
"#
);
