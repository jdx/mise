use eyre::Result;

use super::driver::{self, Action, DriverOpts};
use crate::config::{Config, Settings};
use crate::system;

/// Install missing system packages from `[system.packages]`
///
/// Checks which configured packages are missing and installs them with the
/// system package manager. This may elevate with sudo when not running as
/// root (see the `system_packages.sudo` setting).
///
/// Packages can also be given explicitly in `manager:package` form (e.g.
/// `apt:curl`, `brew:jq`); they are installed whether or not they appear in
/// the config.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemInstall {
    /// Packages in `manager:package` form; defaults to everything configured
    /// in [system.packages]
    #[clap(value_name = "PACKAGE")]
    packages: Vec<String>,

    /// Only install packages for this manager, e.g. `apt` or `brew`
    #[clap(long, short, value_parser = ["apt", "brew", "dnf", "pacman"])]
    manager: Option<String>,

    /// Print the commands that would run without running them
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,

    /// Refresh package manager metadata first (apt: `apt-get update`)
    #[clap(long)]
    update: bool,
}

impl SystemInstall {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise system")?;
        let mgrs = if self.packages.is_empty() {
            let config = Config::get().await?;
            system::packages_from_config(&config)
        } else {
            system::packages_from_specs(&self.packages)?
        };
        let opts = DriverOpts {
            manager: self.manager,
            explicit: !self.packages.is_empty(),
            dry_run: self.dry_run,
            update: self.update,
            yes: self.yes,
        };
        driver::run(mgrs, Action::Install, &opts).await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise system install</bold>
    $ <bold>mise system install apt:curl brew:jq</bold>
    $ <bold>mise system install --dry-run</bold>
    $ <bold>mise system install --manager apt --yes</bold>
"#
);
