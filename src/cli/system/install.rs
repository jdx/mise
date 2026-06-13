use eyre::Result;

use super::driver::{self, Action, DriverOpts};
use crate::config::{Config, Settings};
use crate::system;

/// Install missing system packages from `[bootstrap.packages]`
///
/// Checks which configured packages are missing and installs them with the
/// system package manager. This may elevate with sudo when not running as
/// root (see the `system_packages.sudo` setting).
///
/// Packages can also be given explicitly in `manager:package` form (e.g.
/// `apt:curl`, `brew:jq`); they are installed whether or not they appear in
/// the config. Explicit packages and `--manager` scope the run to packages
/// only.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemInstall {
    /// Packages in `manager:package` form; defaults to everything configured
    /// in [bootstrap.packages]
    #[clap(value_name = "PACKAGE")]
    packages: Vec<String>,

    /// Only install packages for this manager, e.g. `apt`, `brew`, `brew-cask`, or `mas`
    #[clap(long, short, value_parser = ["apt", "brew", "brew-cask", "dnf", "mas", "pacman"])]
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
        Settings::get().ensure_experimental("mise bootstrap")?;
        let mgrs = if self.packages.is_empty() {
            let config = Config::get().await?;
            system::packages_from_config(&config)
        } else {
            let config = Config::get().await?;
            system::packages_from_specs_with_config(&self.packages, Some(&config))?
        };
        let opts = DriverOpts {
            manager: self.manager.clone(),
            explicit: !self.packages.is_empty(),
            dry_run: self.dry_run,
            update: self.update,
            yes: self.yes,
        };
        driver::run(mgrs, Action::Install, &opts).await
    }
}

/// Apply `[bootstrap.macos.defaults]` entries that are unset or differ.
/// Inert off-macOS.
pub(crate) async fn apply_defaults(
    defaults: Vec<system::defaults::DefaultsRequest>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    use crate::system::defaults::{self, DefaultsState};
    if defaults.is_empty() {
        return Ok(());
    }
    if !defaults::is_available() {
        // cross-platform config: [bootstrap.macos.defaults] is simply inert off-macOS
        debug!("defaults: skipping, {}", defaults::unavailable_reason());
        return Ok(());
    }
    let statuses = defaults::status(&defaults).await?;
    let targets: Vec<_> = statuses
        .iter()
        .filter(|s| s.state != DefaultsState::Set)
        .map(|s| s.request.clone())
        .collect();
    let set = statuses.len() - targets.len();
    if set > 0 {
        info!("defaults: {set} value(s) already set");
    }
    if targets.is_empty() {
        return Ok(());
    }
    let list = targets.iter().map(|r| r.to_string()).collect::<Vec<_>>();
    if !dry_run && !yes && console::user_attended_stderr() {
        let msg = format!("defaults: write {}?", list.join(", "));
        if !crate::ui::prompt::confirm(msg)? {
            info!("defaults: skipped");
            return Ok(());
        }
    }
    defaults::apply(&targets, dry_run).await?;
    if !dry_run {
        info!(
            "defaults: wrote {} — some apps only pick up changes after a relaunch \
             (e.g. `killall Dock`)",
            list.join(", ")
        );
    }
    Ok(())
}

/// Apply `[bootstrap.user].login_shell` when it differs for `mise bootstrap`.
/// Inert off-Unix or when `chsh` is missing.
pub(crate) fn apply_login_shell(
    request: Option<system::login_shell::LoginShellRequest>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    use crate::system::login_shell::{self, LoginShellState};
    let Some(request) = request else {
        return Ok(());
    };
    if !login_shell::is_available() {
        debug!(
            "login_shell: skipping, {}",
            login_shell::unavailable_reason()
        );
        return Ok(());
    }
    let status = login_shell::status(&request)?;
    if status.state == LoginShellState::Set {
        info!("login_shell: already set to {}", request.shell);
        return Ok(());
    }
    if !dry_run && !yes && console::user_attended_stderr() {
        let msg = format!("login_shell: run `chsh -s {}`?", request.shell);
        if !crate::ui::prompt::confirm(msg)? {
            info!("login_shell: skipped");
            return Ok(());
        }
    }
    login_shell::apply(&request, dry_run)?;
    if !dry_run {
        info!(
            "login_shell: set to {} - start a new login session for it to take effect",
            request.shell
        );
    }
    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages install</bold>
    $ <bold>mise bootstrap packages install apt:curl brew:jq brew-cask:firefox mas:497799835</bold>
    $ <bold>mise bootstrap packages install --dry-run</bold>
    $ <bold>mise bootstrap packages install --manager apt --yes</bold>
"#
);
