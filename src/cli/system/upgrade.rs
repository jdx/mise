use eyre::Result;

use crate::config::Config;
use crate::system;
use crate::system::driver::{self, Action, DriverOpts};
use crate::system::history::OperationScope;

/// Upgrade installed packages from `[bootstrap.packages]`
///
/// Refreshes package manager metadata, then upgrades the configured packages
/// that are already installed. Each manager decides which version is current.
/// Version pins in config are honored where the manager can install them;
/// other pinned entries are skipped with a warning. Packages that are not
/// installed yet are skipped; install them with `mise bootstrap packages apply`.
/// See https://mise.jdx.dev/bootstrap/packages/ for what each manager does.
///
/// Name packages as `manager:package` to upgrade only those.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "up",
    verbatim_doc_comment,
    example(
        "mise bootstrap packages upgrade --dry-run",
        help = "Show what would be upgraded"
    ),
    example(
        "mise bootstrap packages upgrade",
        help = "Upgrade every configured package that is already installed"
    ),
    example(
        "mise bootstrap packages upgrade --manager apt --yes",
        help = "Upgrade only apt packages, without a prompt"
    ),
    example(
        "mise bootstrap packages upgrade brew:postgresql@17",
        help = "Upgrade one Homebrew formula"
    )
)]
pub(crate) struct SystemUpgrade {
    /// Packages as `manager:package`; defaults to every package in `[bootstrap.packages]`
    #[usage(value_name = "PACKAGE")]
    packages: Vec<String>,

    /// Only upgrade packages for this built-in or plugin manager
    #[usage(long, short)]
    manager: Option<String>,

    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

impl SystemUpgrade {
    pub(crate) async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap packages upgrade", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let mgrs = if self.packages.is_empty() {
            let config = Config::get().await?;
            system::packages_from_config(&config)?
        } else {
            let config = Config::get().await?;
            system::packages_from_specs_with_config(&self.packages, Some(&config))?
        };
        let opts = DriverOpts {
            manager: self.manager,
            explicit: !self.packages.is_empty(),
            allow_unavailable_manager: false,
            dry_run: self.dry_run,
            // upgrades refresh metadata themselves (stale lists would make
            // them silent no-ops), so no separate --update flag
            update: false,
            yes: self.yes,
        };
        driver::run(mgrs, Action::Upgrade, &opts).await
    }
}
