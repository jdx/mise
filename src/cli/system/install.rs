use eyre::Result;

use super::driver::{self, Action, DriverOpts};
use crate::config::{Config, Settings};
use crate::system;

#[derive(Debug, Default)]
pub(crate) struct BootstrapApplyReport {
    /// Whether top-level `mise bootstrap` should print a user follow-up item
    /// for this phase after a successful apply or dry-run.
    pub needs_follow_up: bool,
    pub skipped_reason: Option<String>,
}

/// Apply system packages from `[bootstrap.packages]`
///
/// Checks which configured packages are missing and installs them with the
/// system package manager. This may elevate with sudo when not running as
/// root (see the `system_packages.sudo` setting).
///
/// Packages can also be given explicitly in `manager:package` form (e.g.
/// `apk:zlib-dev`, `apt:curl`, `brew:jq`); they are installed whether or not they appear in
/// the config. Explicit packages and `--manager` scope the run to packages
/// only. `install` is accepted as an alias for this command.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemInstall {
    /// Packages in `manager:package` form; defaults to everything configured
    /// in [bootstrap.packages]
    #[clap(value_name = "PACKAGE")]
    packages: Vec<String>,

    /// Only install packages for this manager, e.g. `apk`, `apt`, `brew`, `brew-cask`, or `mas`
    #[clap(long, short, value_parser = ["apk", "apt", "brew", "brew-cask", "dnf", "mas", "pacman"])]
    manager: Option<String>,

    /// Print the commands that would run without running them
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,

    /// Refresh package manager metadata first (apk: `--update-cache`, apt: `apt-get update`)
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
    apply_defaults_with_report(defaults, dry_run, yes, true)
        .await
        .map(|_| ())
}

pub(crate) async fn apply_defaults_with_report(
    defaults: Vec<system::defaults::DefaultsRequest>,
    dry_run: bool,
    yes: bool,
    print_follow_up: bool,
) -> Result<BootstrapApplyReport> {
    use crate::system::defaults::{self, DefaultsState};
    if defaults.is_empty() {
        return Ok(BootstrapApplyReport::default());
    }
    if !defaults::is_available() {
        // cross-platform config: [bootstrap.macos.defaults] is simply inert off-macOS
        let reason = defaults::unavailable_reason();
        debug!("defaults: skipping, {reason}");
        return Ok(BootstrapApplyReport {
            needs_follow_up: false,
            skipped_reason: Some(reason),
        });
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
        return Ok(BootstrapApplyReport::default());
    }
    let list = targets.iter().map(|r| r.to_string()).collect::<Vec<_>>();
    if !dry_run && !yes && console::user_attended_stderr() {
        let msg = format!("defaults: write {}?", list.join(", "));
        if !crate::ui::prompt::confirm(msg)? {
            info!("defaults: skipped");
            return Ok(BootstrapApplyReport::default());
        }
    }
    defaults::apply(&targets, dry_run).await?;
    if !dry_run {
        if print_follow_up {
            info!(
                "defaults: wrote {} — some apps only pick up changes after a relaunch \
                 (e.g. `killall Dock`)",
                list.join(", ")
            );
        } else {
            info!("defaults: wrote {}", list.join(", "));
        }
    }
    Ok(BootstrapApplyReport {
        needs_follow_up: true,
        skipped_reason: None,
    })
}

/// Apply `[bootstrap.user].login_shell` when it differs for `mise bootstrap`.
/// Inert off-Unix or when `chsh` is missing.
pub(crate) fn apply_login_shell(
    request: Option<system::login_shell::LoginShellRequest>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    apply_login_shell_with_report(request, dry_run, yes, true).map(|_| ())
}

pub(crate) fn apply_login_shell_with_report(
    request: Option<system::login_shell::LoginShellRequest>,
    dry_run: bool,
    yes: bool,
    print_follow_up: bool,
) -> Result<BootstrapApplyReport> {
    use crate::system::login_shell::{self, LoginShellState};
    let Some(request) = request else {
        return Ok(BootstrapApplyReport::default());
    };
    if !login_shell::is_available() {
        let reason = login_shell::unavailable_reason();
        debug!("login_shell: skipping, {reason}");
        return Ok(BootstrapApplyReport {
            needs_follow_up: false,
            skipped_reason: Some(reason),
        });
    }
    let status = login_shell::status(&request)?;
    if status.state == LoginShellState::Set {
        info!("login_shell: already set to {}", request.shell);
        return Ok(BootstrapApplyReport::default());
    }
    let needs_follow_up = status.state != LoginShellState::Set;
    if !dry_run && !yes && console::user_attended_stderr() {
        let msg = format!("login_shell: run `chsh -s {}`?", request.shell);
        if !crate::ui::prompt::confirm(msg)? {
            info!("login_shell: skipped");
            return Ok(BootstrapApplyReport::default());
        }
    }

    login_shell::apply(&request, dry_run)?;
    if !dry_run {
        if print_follow_up {
            info!(
                "login_shell: set to {} - start a new login session for it to take effect",
                request.shell
            );
        } else {
            info!("login_shell: set to {}", request.shell);
        }
    }
    Ok(BootstrapApplyReport {
        needs_follow_up,
        skipped_reason: None,
    })
}

/// Apply `[bootstrap.mise_shell_activate]` entries using dotfile edit blocks.
pub(crate) fn apply_shell_activation(
    config: &Config,
    requests: Vec<system::shell_activation::ShellActivationRequest>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    if requests.is_empty() {
        return Ok(());
    }
    let edits = requests
        .into_iter()
        .map(|request| request.edit)
        .collect::<Vec<_>>();
    let opts = system::edits::ApplyOpts {
        dry_run,
        verbose: Settings::get().verbose,
        yes,
    };
    system::edits::apply(config, &edits, &opts)
}

/// Apply `[bootstrap.repos]` entries that are missing or differ.
pub(crate) fn apply_repos(
    repos: Vec<system::repos::RepoRequest>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    use crate::system::repos;
    if repos.is_empty() {
        return Ok(());
    }
    let statuses = repos::status(&repos)?;
    repos::preflight_statuses(&statuses)?;
    let targets: Vec<_> = statuses
        .iter()
        .filter(|s| !s.state.is_current())
        .cloned()
        .collect();
    let current = statuses.len() - targets.len();
    if current > 0 {
        info!("repos: {current} repo(s) already current");
    }
    if targets.is_empty() {
        return Ok(());
    }
    let list = targets
        .iter()
        .map(|s| s.request.to_string())
        .collect::<Vec<_>>();
    if !dry_run && !yes && console::user_attended_stderr() {
        let msg = format!("repos: apply {}?", list.join(", "));
        if !crate::ui::prompt::confirm(msg)? {
            info!("repos: skipped");
            return Ok(());
        }
    }
    repos::apply_statuses(&targets, dry_run)?;
    if !dry_run {
        info!("repos: applied {}", list.join(", "));
    }
    Ok(())
}

/// Apply `[bootstrap.macos.launchd.agents]` entries that are missing, changed,
/// or not loaded. Inert off-macOS.
pub(crate) async fn apply_launchd(
    agents: Vec<system::launchd::LaunchdRequest>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    apply_launchd_with_report(agents, dry_run, yes)
        .await
        .map(|_| ())
}

pub(crate) async fn apply_launchd_with_report(
    agents: Vec<system::launchd::LaunchdRequest>,
    dry_run: bool,
    yes: bool,
) -> Result<BootstrapApplyReport> {
    use crate::system::launchd::{self, LaunchdState};
    if agents.is_empty() {
        return Ok(BootstrapApplyReport::default());
    }
    if !launchd::is_available() {
        let reason = launchd::unavailable_reason();
        debug!("launchd: skipping, {reason}");
        return Ok(BootstrapApplyReport {
            needs_follow_up: false,
            skipped_reason: Some(reason),
        });
    }
    let statuses = launchd::status(&agents).await?;
    let targets: Vec<_> = statuses
        .iter()
        .filter(|s| s.state != LaunchdState::Loaded)
        .map(|s| s.request.clone())
        .collect();
    let loaded = statuses.len() - targets.len();
    if loaded > 0 {
        info!("launchd: {loaded} agent(s) already loaded");
    }
    if targets.is_empty() {
        return Ok(BootstrapApplyReport::default());
    }
    let list = targets.iter().map(|r| r.to_string()).collect::<Vec<_>>();
    if !dry_run && !yes && console::user_attended_stderr() {
        let msg = format!("launchd: install/load {}?", list.join(", "));
        if !crate::ui::prompt::confirm(msg)? {
            info!("launchd: skipped");
            return Ok(BootstrapApplyReport::default());
        }
    }
    launchd::apply(&targets, dry_run).await?;
    if !dry_run {
        info!("launchd: installed/loaded {}", list.join(", "));
    }
    Ok(BootstrapApplyReport {
        needs_follow_up: false,
        skipped_reason: None,
    })
}

/// Apply `[bootstrap.linux.systemd.units]` entries that are missing, changed,
/// or inactive. Inert off-Linux.
pub(crate) async fn apply_systemd(
    units: Vec<system::systemd::SystemdRequest>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    apply_systemd_with_report(units, dry_run, yes)
        .await
        .map(|_| ())
}

pub(crate) async fn apply_systemd_with_report(
    units: Vec<system::systemd::SystemdRequest>,
    dry_run: bool,
    yes: bool,
) -> Result<BootstrapApplyReport> {
    use crate::system::systemd;
    if units.is_empty() {
        return Ok(BootstrapApplyReport::default());
    }
    if !systemd::is_available() {
        let reason = systemd::unavailable_reason();
        debug!("systemd: skipping, {reason}");
        return Ok(BootstrapApplyReport {
            needs_follow_up: false,
            skipped_reason: Some(reason),
        });
    }
    let statuses = systemd::status(&units).await?;
    let targets: Vec<_> = statuses
        .iter()
        .filter(|s| !s.is_desired())
        .map(|s| s.request.clone())
        .collect();
    let applied = statuses.len() - targets.len();
    if applied > 0 {
        info!("systemd: {applied} unit(s) already applied");
    }
    if targets.is_empty() {
        return Ok(BootstrapApplyReport::default());
    }
    let list = targets.iter().map(|r| r.to_string()).collect::<Vec<_>>();
    if !dry_run && !yes && console::user_attended_stderr() {
        let msg = format!("systemd: apply {}?", list.join(", "));
        if !crate::ui::prompt::confirm(msg)? {
            info!("systemd: skipped");
            return Ok(BootstrapApplyReport::default());
        }
    }
    systemd::apply(&targets, dry_run).await?;
    if !dry_run {
        info!("systemd: applied {}", list.join(", "));
    }
    Ok(BootstrapApplyReport {
        needs_follow_up: false,
        skipped_reason: None,
    })
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages apply</bold>
    $ <bold>mise bootstrap packages apply apk:zlib-dev apt:curl brew:jq brew-cask:firefox mas:497799835</bold>
    $ <bold>mise bootstrap packages apply --dry-run</bold>
    $ <bold>mise bootstrap packages apply --manager apt --yes</bold>
"#
);
