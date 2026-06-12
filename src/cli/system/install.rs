use eyre::{Result, bail};

use crate::config::{Config, Settings};
use crate::system;
use crate::system::packages::{InstallOpts, PackageState};
use crate::ui::prompt;

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
        if let Some(only) = &self.manager
            && !mgrs.iter().any(|mp| mp.manager.name() == only)
        {
            // distinguish "not configured" from "filtered out by settings" —
            // the aggregation above drops managers excluded by
            // system_packages.managers before we ever see them
            if let Some(enabled) = &Settings::get().system_packages.managers
                && !enabled.contains(only)
            {
                bail!(
                    "manager '{only}' is excluded by the system_packages.managers setting \
                     (currently: {})",
                    enabled.join(", ")
                );
            }
            bail!("no packages requested for manager '{only}'");
        }
        if mgrs.is_empty() {
            info!("no system packages configured in [system.packages]");
            return Ok(());
        }
        let opts = InstallOpts {
            dry_run: self.dry_run,
            update: self.update,
        };
        for mp in mgrs {
            if let Some(only) = &self.manager
                && mp.manager.name() != only
            {
                continue;
            }
            let name = mp.manager.name();
            if mp.disabled {
                if self.manager.is_some() {
                    bail!("manager '{name}' is excluded by the system_packages.managers setting");
                }
                debug!("{name}: skipping, excluded by system_packages.managers");
                continue;
            }
            if !mp.manager.is_available() {
                if self.manager.is_some() {
                    // explicitly requested — failing silently would be a lie
                    bail!(
                        "{name} is not available: {}",
                        mp.manager.unavailable_reason()
                    );
                }
                debug!("{name}: skipping, {}", mp.manager.unavailable_reason());
                continue;
            }
            let statuses = mp.manager.installed(&mp.requests).await?;
            let missing: Vec<_> = statuses
                .iter()
                .filter(|s| !matches!(s.state, PackageState::Installed { .. }))
                .map(|s| s.request.clone())
                .collect();
            let satisfied = statuses.len() - missing.len();
            if satisfied > 0 {
                info!("{name}: {satisfied} package(s) already installed");
            }
            if missing.is_empty() {
                continue;
            }
            let list = missing.iter().map(|r| r.to_string()).collect::<Vec<_>>();
            if !self.dry_run && !self.yes && console::user_attended_stderr() {
                let msg = format!("{name}: install {}?", list.join(", "));
                if !prompt::confirm(msg)? {
                    info!("{name}: skipped");
                    continue;
                }
            }
            mp.manager.install(&missing, &opts).await?;
            if !self.dry_run {
                info!("{name}: installed {}", list.join(", "));
            }
        }
        Ok(())
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
