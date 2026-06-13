use eyre::Result;

use super::driver::{self, Action, DriverOpts};
use crate::config::{Config, Settings};
use crate::system;

/// Install missing system packages from `[system.packages]`, apply files
/// from `[system.files]` and edits from `[system.edits]`, write macOS
/// defaults from `[system.defaults]`, and set `[system].login_shell`
///
/// Checks which configured packages are missing and installs them with the
/// system package manager. This may elevate with sudo when not running as
/// root (see the `system_packages.sudo` setting). Afterwards, `[system.files]`
/// and `[system.edits]` entries that aren't in their desired state are
/// applied, on macOS any `[system.defaults]` entries that are unset or
/// differ are written, and on Unix `[system].login_shell` is applied with
/// `chsh -s` when it differs.
///
/// Packages can also be given explicitly in `manager:package` form (e.g.
/// `apt:curl`, `brew:jq`); they are installed whether or not they appear in
/// the config. Explicit packages and `--manager` scope the run to packages
/// only.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemInstall {
    /// Packages in `manager:package` form; defaults to everything configured
    /// in [system.packages]
    #[clap(value_name = "PACKAGE")]
    packages: Vec<String>,

    /// Overwrite existing files that conflict with `[system.files]` entries
    #[clap(long, short)]
    force: bool,

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
        // defaults only participate in the full converge-everything form —
        // explicit package specs and --manager filters scope the run to
        // packages
        let mut defaults = vec![];
        let mut login_shell = None;
        let mgrs = if self.packages.is_empty() {
            let config = Config::get().await?;
            if self.manager.is_none() {
                defaults = system::defaults_from_config(&config);
                login_shell = system::login_shell_from_config(&config);
            }
            system::packages_from_config(&config)
        } else {
            let config = Config::get().await?;
            system::packages_from_specs_with_config(&self.packages, Some(&config))?
        };
        // explicit packages or a --manager filter narrow the run to those
        // packages; files, edits, and defaults are part of the "apply
        // everything" form only
        let packages_only = !self.packages.is_empty() || self.manager.is_some();
        let opts = DriverOpts {
            manager: self.manager.clone(),
            explicit: !self.packages.is_empty(),
            dry_run: self.dry_run,
            update: self.update,
            yes: self.yes,
        };
        let (files, edits) = if packages_only {
            (vec![], vec![])
        } else {
            let config = Config::get().await?;
            (
                system::files::files_from_config(&config),
                system::edits::edits_from_config(&config),
            )
        };
        // when only defaults/files/edits are configured, skip the driver so
        // it doesn't print "no system packages configured"
        if !mgrs.is_empty()
            || (defaults.is_empty()
                && login_shell.is_none()
                && files.is_empty()
                && edits.is_empty())
        {
            driver::run(mgrs, Action::Install, &opts).await?;
        }
        if !files.is_empty() {
            let config = Config::get().await?;
            let apply_opts = system::files::ApplyOpts {
                dry_run: self.dry_run,
                force: self.force,
                yes: self.yes,
            };
            system::files::apply(&config, &files, &apply_opts)?;
        }
        if !edits.is_empty() {
            let config = Config::get().await?;
            let apply_opts = system::edits::ApplyOpts {
                dry_run: self.dry_run,
                yes: self.yes,
            };
            system::edits::apply(&config, &edits, &apply_opts)?;
        }
        apply_defaults(defaults, self.dry_run, self.yes).await?;
        apply_login_shell(login_shell, self.dry_run, self.yes)
    }
}

/// Apply `[system.defaults]` entries that are unset or differ — shared by
/// `mise system install` and `mise bootstrap`. Inert off-macOS.
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
        // cross-platform config: [system.defaults] is simply inert off-macOS
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

/// Apply `[system].login_shell` when it differs - shared by `mise system
/// install` and `mise bootstrap`. Inert off-Unix or when `chsh` is missing.
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

    $ <bold>mise system install</bold>
    $ <bold>mise system install apt:curl brew:jq</bold>
    $ <bold>mise system install --dry-run</bold>
    $ <bold>mise system install --manager apt --yes</bold>
"#
);
