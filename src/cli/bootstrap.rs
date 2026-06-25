use std::collections::HashSet;
use std::sync::Arc;

use eyre::Result;
use serde_json::{Value, json};

use super::dotfiles::{DotfilesApply, DotfilesStatus};
use super::install::Install;
use super::run;
use super::system::driver::{self, Action, DriverOpts};
use super::system::{import, install, prune, status, upgrade, r#use};
use crate::config::{self, Config, Settings};
use crate::dirs;
use crate::path::PathExt;
use crate::system;
use crate::system::defaults::DefaultsState;
use crate::system::files::{FileMode, FileRequest, FileState};
use crate::system::hooks::{self, BootstrapHookPhase};
use crate::system::launchd::LaunchdState;
use crate::system::login_shell::LoginShellState;
use crate::system::packages::PackageState;
use crate::system::repos::RepoState;
use crate::system::systemd::SystemdState;
use crate::toolset::ResolveOptions;
use crate::ui::table::MiseTable;
use clap::{Subcommand, ValueEnum};

/// [experimental] Set up a machine for the current config in one command
///
/// Runs the bootstrap steps for the current config in order:
///
/// 0. `[bootstrap.hooks.pre-packages]` — optional setup hook
/// 1. `mise bootstrap packages apply` — install missing
///    `[bootstrap.packages]`
///    then `[bootstrap.hooks.post-packages]`
/// 2. `mise bootstrap repos apply` — clone/update `[bootstrap.repos]`
///    surrounded by `pre-repos`/`post-repos` hooks
/// 3. `mise bootstrap dotfiles apply` — apply dotfiles from `[dotfiles]`
///    surrounded by `pre-dotfiles`/`post-dotfiles` hooks
/// 4. `mise bootstrap mise-shell-activate apply` — configure shell activation
///    from `[bootstrap.mise_shell_activate]`
/// 5. `mise bootstrap macos defaults apply` — write
///    `[bootstrap.macos.defaults]` entries (macOS)
///    surrounded by `pre-defaults`/`post-defaults` hooks
/// 6. `mise bootstrap macos launchd-agents apply` — install/load
///    `[bootstrap.macos.launchd.agents]`
/// 7. `mise bootstrap linux systemd-units apply` — install/start
///    `[bootstrap.linux.systemd.units]`
/// 8. `mise bootstrap user apply` — set `[bootstrap.user].login_shell`
///    (Unix)
///    surrounded by `pre-user`/`post-user` hooks
/// 9. `mise install` — install missing tools from `[tools]`
///    surrounded by `pre-tools`/`post-tools` hooks
/// 10. `mise run bootstrap` — if a task named `bootstrap` is defined
/// 11. `[bootstrap.hooks.final]` — optional final hook
///
/// The declarative steps converge — anything already in its desired state
/// is skipped, so re-running is safe. The `bootstrap` task runs on every
/// invocation; keep it idempotent. Use it for any project-specific setup
/// that doesn't fit the declarative sections (seeding databases, auth flows,
/// etc.) — it runs with the installed tools on PATH.
///
/// Use `--skip <part>` to skip named parts, or `--only <part>` to run just
/// named parts. Both flags can be repeated or comma-separated, but they
/// cannot be used together.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Bootstrap {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Print what would happen without installing anything
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip confirmation prompts
    #[clap(long, short)]
    yes: bool,

    /// Overwrite existing files that conflict with whole-file dotfile entries
    #[clap(long)]
    force_dotfiles: bool,

    /// Run only one or more bootstrap parts
    ///
    /// Can be passed multiple times or as a comma-separated list.
    /// Cannot be used with `--skip`.
    #[clap(long, value_enum, value_delimiter = ',', conflicts_with = "skip")]
    only: Vec<BootstrapPart>,

    /// Skip one or more bootstrap parts
    ///
    /// Can be passed multiple times or as a comma-separated list.
    #[clap(long, value_enum, value_delimiter = ',')]
    skip: Vec<BootstrapPart>,

    /// Refresh system package manager metadata first (apk: `--update-cache`, apt: `apt-get update`)
    #[clap(long)]
    update: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
enum BootstrapPart {
    Packages,
    Repos,
    Dotfiles,
    #[clap(name = "mise-shell-activate", alias = "shell")]
    Shell,
    #[clap(name = "macos-defaults", alias = "defaults")]
    Defaults,
    #[clap(name = "macos-launchd-agents", alias = "launchd")]
    Launchd,
    #[clap(name = "linux-systemd-units", alias = "systemd")]
    Systemd,
    User,
    Tools,
    Task,
    FinalHook,
}

impl BootstrapPart {
    // Keep this in sync with every enum variant. `--only` computes a
    // complement from ALL, so an omitted variant would always run.
    const ALL: [Self; 11] = [
        Self::Packages,
        Self::Repos,
        Self::Dotfiles,
        Self::Shell,
        Self::Defaults,
        Self::Launchd,
        Self::Systemd,
        Self::User,
        Self::Tools,
        Self::Task,
        Self::FinalHook,
    ];
}

#[derive(Debug, Subcommand)]
enum Commands {
    Dotfiles(BootstrapDotfiles),
    #[clap(hide = true)]
    Launchd(BootstrapLaunchd),
    Linux(BootstrapLinux),
    Macos(BootstrapMacos),
    #[clap(hide = true)]
    MacosDefaults(BootstrapMacosDefaults),
    #[clap(name = "mise-shell-activate", alias = "shell")]
    MiseShellActivate(BootstrapShell),
    Packages(BootstrapPackages),
    Repos(BootstrapRepos),
    Status(BootstrapStatus),
    #[clap(hide = true)]
    Systemd(BootstrapSystemd),
    User(BootstrapUser),
}

/// Show the aggregate bootstrap status
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "ls", verbatim_doc_comment)]
struct BootstrapStatus {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured bootstrap state is not in its desired state
    #[clap(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage dotfiles from `[dotfiles]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapDotfiles {
    #[clap(subcommand)]
    command: BootstrapDotfilesCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapDotfilesCommands {
    Apply(BootstrapDotfilesApply),
    Status(BootstrapDotfilesStatus),
}

/// Apply dotfiles from `[dotfiles]`
///
/// Applies configured whole-file entries and edits that aren't in their
/// desired state. Whole-file entries may symlink, copy, or render templates.
/// Edit entries manage a marker-delimited block or a single line in a file
/// mise doesn't otherwise own.
#[derive(Debug, clap::Args)]
#[clap(
    verbatim_doc_comment,
    after_long_help = BOOTSTRAP_DOTFILES_APPLY_AFTER_LONG_HELP
)]
struct BootstrapDotfilesApply {
    #[clap(flatten)]
    cmd: DotfilesApply,
}

/// Show the status of dotfiles from `[dotfiles]`
#[derive(Debug, clap::Args)]
#[clap(
    verbatim_doc_comment,
    after_long_help = BOOTSTRAP_DOTFILES_STATUS_AFTER_LONG_HELP
)]
struct BootstrapDotfilesStatus {
    #[clap(flatten)]
    cmd: DotfilesStatus,
}

static BOOTSTRAP_DOTFILES_APPLY_AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles apply</bold>
    $ <bold>mise bootstrap dotfiles apply --dry-run</bold>
    $ <bold>mise bootstrap dotfiles apply --force --yes</bold>
"#
);

static BOOTSTRAP_DOTFILES_STATUS_AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles status</bold>
    $ <bold>mise bootstrap dotfiles status ~/.zshrc</bold>
    $ <bold>mise bootstrap dotfiles status --json</bold>
    $ <bold>mise bootstrap dotfiles status --missing</bold> # exit 1 if anything is out of sync
"#
);

/// Manage bootstrap system packages from `[bootstrap.packages]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapPackages {
    #[clap(subcommand)]
    command: BootstrapPackagesCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapPackagesCommands {
    #[clap(alias = "install")]
    Apply(install::SystemInstall),
    #[cfg(unix)]
    Brew(super::system::brew::SystemBrew),
    Import(import::SystemImport),
    Prune(prune::SystemPrune),
    Status(status::SystemStatus),
    Upgrade(upgrade::SystemUpgrade),
    Use(r#use::SystemUse),
}

/// Manage git repo checkouts from `[bootstrap.repos]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapRepos {
    #[clap(subcommand)]
    command: BootstrapReposCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapReposCommands {
    Apply(BootstrapReposApply),
    Status(BootstrapReposStatus),
}

#[derive(Debug, clap::Args)]
struct BootstrapReposApply {
    /// Print the commands that would run without running them
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

#[derive(Debug, clap::Args)]
struct BootstrapReposStatus {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured repo is not in its desired state
    #[clap(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage macOS bootstrap config from `[bootstrap.macos]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapMacos {
    #[clap(subcommand)]
    command: BootstrapMacosCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapMacosCommands {
    Defaults(BootstrapMacosDefaults),
    #[clap(name = "launchd-agents", alias = "launchd")]
    LaunchdAgents(BootstrapLaunchd),
}

/// Manage Linux bootstrap config from `[bootstrap.linux]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapLinux {
    #[clap(subcommand)]
    command: BootstrapLinuxCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapLinuxCommands {
    #[clap(name = "systemd-units", alias = "systemd")]
    SystemdUnits(BootstrapSystemd),
}

/// Manage macOS defaults from `[bootstrap.macos.defaults]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapMacosDefaults {
    #[clap(subcommand)]
    command: BootstrapMacosDefaultsCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapMacosDefaultsCommands {
    Apply(BootstrapMacosDefaultsApply),
    Status(BootstrapMacosDefaultsStatus),
}

#[derive(Debug, clap::Args)]
struct BootstrapMacosDefaultsApply {
    /// Print the commands that would run without running them
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

#[derive(Debug, clap::Args)]
struct BootstrapMacosDefaultsStatus {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured defaults are not in their desired state
    #[clap(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage macOS LaunchAgents from `[bootstrap.macos.launchd.agents]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapLaunchd {
    #[clap(subcommand)]
    command: BootstrapLaunchdCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapLaunchdCommands {
    Apply(BootstrapLaunchdApply),
    Status(BootstrapLaunchdStatus),
}

#[derive(Debug, clap::Args)]
struct BootstrapLaunchdApply {
    /// Print the commands that would run without running them
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

#[derive(Debug, clap::Args)]
struct BootstrapLaunchdStatus {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured LaunchAgent is not in its desired state
    #[clap(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage systemd user services from `[bootstrap.linux.systemd.units]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapSystemd {
    #[clap(subcommand)]
    command: BootstrapSystemdCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapSystemdCommands {
    Apply(BootstrapSystemdApply),
    Status(BootstrapSystemdStatus),
}

#[derive(Debug, clap::Args)]
struct BootstrapSystemdApply {
    /// Print the commands that would run without running them
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

#[derive(Debug, clap::Args)]
struct BootstrapSystemdStatus {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured systemd user service is not in its desired state
    #[clap(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage mise shell activation from `[bootstrap.mise_shell_activate]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapShell {
    #[clap(subcommand)]
    command: BootstrapShellCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapShellCommands {
    Apply(BootstrapShellApply),
    Status(BootstrapShellStatus),
}

#[derive(Debug, clap::Args)]
struct BootstrapShellApply {
    /// Print the actions that would run without writing anything
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

#[derive(Debug, clap::Args)]
struct BootstrapShellStatus {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured shell activation is not in its desired state
    #[clap(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage current-user bootstrap settings from `[bootstrap.user]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapUser {
    #[clap(subcommand)]
    command: BootstrapUserCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapUserCommands {
    Apply(BootstrapUserApply),
    Status(BootstrapUserStatus),
}

#[derive(Debug, clap::Args)]
struct BootstrapUserApply {
    /// Print the commands that would run without running them
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

#[derive(Debug, clap::Args)]
struct BootstrapUserStatus {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured user setting is not in its desired state
    #[clap(long, verbatim_doc_comment)]
    missing: bool,
}

impl Bootstrap {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        if let Some(command) = self.command {
            return command.run().await;
        }
        let mut config = Config::get().await?;
        let mut hooks = system::hooks_from_config(&config);
        let skip = self.skip_parts();
        let mut follow_up = BootstrapFollowUp::new(self.dry_run);
        let mut dry_run_config_files = None;

        if skip.contains(&BootstrapPart::Packages) {
            debug!("bootstrap: system packages skipped");
        } else {
            self.run_hooks(&hooks, BootstrapHookPhase::PrePackages)
                .await?;
            let mgrs = system::packages_from_config(&config);
            if mgrs.is_empty() {
                debug!("bootstrap: no [bootstrap.packages] configured, skipping");
            } else {
                info!("bootstrap: system packages");
                follow_up.add_package_skips(&mgrs);
                let opts = DriverOpts {
                    manager: None,
                    explicit: false,
                    dry_run: self.dry_run,
                    update: self.update,
                    yes: self.yes,
                };
                driver::run(mgrs, Action::Install, &opts).await?;
            }
            self.run_hooks(&hooks, BootstrapHookPhase::PostPackages)
                .await?;
        }

        if skip.contains(&BootstrapPart::Repos) {
            debug!("bootstrap: repos skipped");
        } else {
            self.run_hooks(&hooks, BootstrapHookPhase::PreRepos).await?;
            let repos = system::repos_from_config(&config);
            if repos.is_empty() {
                debug!("bootstrap: no [bootstrap.repos] configured, skipping");
            } else {
                info!("bootstrap: repos");
                install::apply_repos(repos, self.dry_run, self.yes)?;
            }
            self.run_hooks(&hooks, BootstrapHookPhase::PostRepos)
                .await?;
        }

        if skip.contains(&BootstrapPart::Dotfiles) {
            debug!("bootstrap: dotfiles skipped");
            if !self.dry_run {
                config = Config::reset().await?;
                hooks = system::hooks_from_config(&config);
            }
        } else {
            self.run_hooks(&hooks, BootstrapHookPhase::PreDotfiles)
                .await?;
            let files = system::files::files_from_config(&config);
            if files.is_empty() {
                debug!("bootstrap: no whole-file [dotfiles] entries configured, skipping");
            } else {
                info!("bootstrap: dotfiles");
                let opts = system::files::ApplyOpts {
                    dry_run: self.dry_run,
                    verbose: false,
                    force: self.force_dotfiles,
                    force_hint: "use --force-dotfiles or run `mise dotfiles apply --force`",
                    yes: self.yes,
                };
                system::files::apply(&config, &files, &opts)?;
            }

            let edits = system::edits::edits_from_config(&config);
            if edits.is_empty() {
                debug!("bootstrap: no edit [dotfiles] entries configured, skipping");
            } else {
                info!("bootstrap: dotfile edits");
                let opts = system::edits::ApplyOpts {
                    dry_run: self.dry_run,
                    verbose: false,
                    yes: self.yes,
                };
                system::edits::apply(&config, &edits, &opts)?;
            }
            if self.dry_run {
                let config_files =
                    self.config_files_after_dotfiles_dry_run(&config, &files, &edits)?;
                hooks = system::hooks_from_config_files(&config_files);
                dry_run_config_files = Some(config_files);
            } else {
                config = Config::reset().await?;
                hooks = system::hooks_from_config(&config);
            }
            self.run_hooks(&hooks, BootstrapHookPhase::PostDotfiles)
                .await?;
        }

        if skip.contains(&BootstrapPart::Shell) {
            debug!("bootstrap: shell activation skipped");
        } else {
            let activations = dry_run_config_files
                .as_ref()
                .map(system::shell_activation_from_config_files)
                .unwrap_or_else(|| system::shell_activation_from_config(&config));
            if activations.is_empty() {
                debug!("bootstrap: no [bootstrap.mise_shell_activate] configured, skipping");
            } else {
                info!("bootstrap: shell activation");
                install::apply_shell_activation(&config, activations, self.dry_run, self.yes)?;
            }
        }

        if skip.contains(&BootstrapPart::Defaults) {
            debug!("bootstrap: system defaults skipped");
        } else {
            self.run_hooks(&hooks, BootstrapHookPhase::PreDefaults)
                .await?;
            let defaults = system::defaults_from_config(&config);
            if defaults.is_empty() {
                debug!("bootstrap: no [bootstrap.macos.defaults] configured, skipping");
            } else {
                info!("bootstrap: system defaults");
                let requested = defaults.len();
                let report =
                    install::apply_defaults_with_report(defaults, self.dry_run, self.yes, false)
                        .await?;
                if report.needs_follow_up {
                    follow_up.add_macos_defaults();
                }
                if let Some(reason) = report.skipped_reason {
                    follow_up.add_skipped(format!(
                        "macOS defaults: {requested} entry(ies) skipped ({reason})"
                    ));
                }
            }
            self.run_hooks(&hooks, BootstrapHookPhase::PostDefaults)
                .await?;
        }

        if skip.contains(&BootstrapPart::Launchd) {
            debug!("bootstrap: launchd agents skipped");
        } else {
            let agents = system::launchd_from_config(&config);
            if agents.is_empty() {
                debug!("bootstrap: no [bootstrap.macos.launchd.agents] configured, skipping");
            } else {
                info!("bootstrap: launchd agents");
                let requested = agents.len();
                let report =
                    install::apply_launchd_with_report(agents, self.dry_run, self.yes).await?;
                if let Some(reason) = report.skipped_reason {
                    follow_up
                        .add_skipped(format!("launchd: {requested} agent(s) skipped ({reason})"));
                }
            }
        }

        if skip.contains(&BootstrapPart::Systemd) {
            debug!("bootstrap: systemd user services skipped");
        } else {
            let units = system::systemd_from_config(&config);
            if units.is_empty() {
                debug!("bootstrap: no [bootstrap.linux.systemd.units] configured, skipping");
            } else {
                info!("bootstrap: systemd user services");
                let requested = units.len();
                let report =
                    install::apply_systemd_with_report(units, self.dry_run, self.yes).await?;
                if let Some(reason) = report.skipped_reason {
                    follow_up
                        .add_skipped(format!("systemd: {requested} unit(s) skipped ({reason})"));
                }
            }
        }

        if skip.contains(&BootstrapPart::User) {
            debug!("bootstrap: login shell skipped");
        } else {
            self.run_hooks(&hooks, BootstrapHookPhase::PreUser).await?;
            let login_shell = system::login_shell_from_config(&config);
            if login_shell.is_none() {
                debug!("bootstrap: no [bootstrap.user].login_shell configured, skipping");
            } else {
                let shell = login_shell.as_ref().map(|r| r.shell.clone());
                info!("bootstrap: login shell");
                let report = install::apply_login_shell_with_report(
                    login_shell,
                    self.dry_run,
                    self.yes,
                    false,
                )?;
                if report.needs_follow_up
                    && let Some(shell) = shell.as_ref()
                {
                    follow_up.add_login_shell(shell);
                }
                if let Some(reason) = report.skipped_reason
                    && let Some(shell) = shell
                {
                    follow_up.add_skipped(format!("login shell {shell}: skipped ({reason})"));
                }
            }
            self.run_hooks(&hooks, BootstrapHookPhase::PostUser).await?;
        }

        if skip.contains(&BootstrapPart::Tools) {
            debug!("bootstrap: tools skipped");
        } else {
            self.run_hooks(&hooks, BootstrapHookPhase::PreTools).await?;
            info!("bootstrap: tools");
            Install::new_bare(self.dry_run).run().await?;
            if !self.dry_run {
                config = Config::reset().await?;
                hooks = system::hooks_from_config(&config);
            }
            self.run_hooks(&hooks, BootstrapHookPhase::PostTools)
                .await?;
        }

        if skip.contains(&BootstrapPart::Task) {
            debug!("bootstrap: `bootstrap` task skipped");
        } else {
            let tasks = config.tasks().await?;
            if tasks.iter().any(|(_, t)| t.is_match("bootstrap")) {
                info!("bootstrap: running `bootstrap` task");
                self.run_task("bootstrap", skip.contains(&BootstrapPart::Tools))
                    .await?;
            } else {
                debug!("bootstrap: no `bootstrap` task defined, skipping");
            }
        }
        if skip.contains(&BootstrapPart::FinalHook) {
            debug!("bootstrap: final hook skipped");
        } else {
            self.run_hooks(&hooks, BootstrapHookPhase::Final).await?;
        }
        follow_up.print()?;
        Ok(())
    }

    async fn run_hooks(
        &self,
        hooks: &[hooks::BootstrapHook],
        phase: BootstrapHookPhase,
    ) -> Result<()> {
        hooks::run_phase(hooks, phase, self.dry_run).await
    }

    fn skip_parts(&self) -> HashSet<BootstrapPart> {
        if self.only.is_empty() {
            self.skip.iter().copied().collect()
        } else {
            let only = self.only.iter().copied().collect::<HashSet<_>>();
            BootstrapPart::ALL
                .into_iter()
                .filter(|part| !only.contains(part))
                .collect()
        }
    }

    fn config_files_after_dotfiles_dry_run(
        &self,
        config: &Config,
        files: &[FileRequest],
        edits: &[system::edits::EditRequest],
    ) -> Result<config::ConfigMap> {
        let mut config_files = config.config_files.clone();
        let mut bodies = indexmap::IndexMap::new();
        for file in files {
            if !is_mise_config_target(&file.target) || !file.source.is_file() {
                continue;
            }
            match dotfile_mise_config_body(config, file) {
                Ok(body) => match parse_mise_config_body(&file.target, &body) {
                    Ok(cf) => {
                        bodies.insert(file.target.clone(), body);
                        config_files.insert(file.target.clone(), cf);
                    }
                    Err(err) => {
                        warn!(
                            "[dotfiles].\"{}\": failed to parse config source {}: {err}",
                            file.target_raw,
                            file.source.display()
                        );
                    }
                },
                Err(err) => {
                    warn!(
                        "[dotfiles].\"{}\": failed to read config source {}: {err}",
                        file.target_raw,
                        file.source.display()
                    );
                }
            }
        }
        for edit in edits {
            if !is_mise_config_target(&edit.path) {
                continue;
            }
            let body = match bodies.get(&edit.path) {
                Some(body) => body.clone(),
                None if edit.path.exists() => match crate::file::read_to_string(&edit.path) {
                    Ok(body) => body,
                    Err(err) => {
                        warn!(
                            "[dotfiles].\"{}\": failed to read config target {}: {err}",
                            edit.config_key(),
                            edit.path.display()
                        );
                        continue;
                    }
                },
                None => String::new(),
            };
            match system::edits::apply_dry_run_to_string(config, edit, &body) {
                Ok(Some(body)) => match parse_mise_config_body(&edit.path, &body) {
                    Ok(cf) => {
                        bodies.insert(edit.path.clone(), body);
                        config_files.insert(edit.path.clone(), cf);
                    }
                    Err(err) => {
                        warn!(
                            "[dotfiles].\"{}\": failed to parse edited config target {}: {err}",
                            edit.config_key(),
                            edit.path.display()
                        );
                    }
                },
                Ok(None) => {
                    debug!(
                        "bootstrap: edited config target {} skipped in dry-run config simulation \
                         because the edit requires template rendering",
                        edit.path.display()
                    );
                }
                Err(err) => {
                    warn!(
                        "[dotfiles].\"{}\": failed to simulate config edit for {}: {err}",
                        edit.config_key(),
                        edit.path.display()
                    );
                }
            }
        }
        Ok(config_files)
    }

    async fn run_task(&self, task: &str, skip_tools: bool) -> Result<()> {
        run::Run {
            task: task.into(),
            args: vec![],
            args_last: vec![],
            cd: None,
            continue_on_error: false,
            dry_run: self.dry_run,
            force: false,
            is_linear: false,
            jobs: None,
            no_timings: false,
            output: None,
            shell: None,
            quiet: false,
            silent: false,
            raw: false,
            timings: false,
            tmpdir: Default::default(),
            tool: Default::default(),
            output_handler: None,
            context_builder: Default::default(),
            executor: None,
            no_cache: Default::default(),
            timeout: None,
            skip_deps: false,
            // a dry run must not auto-install tools before the (not actually
            // run) task, and --skip tools must keep the task runner from
            // installing them implicitly before bootstrap tasks
            skip_tools: self.dry_run || skip_tools,
            no_deps: false,
            fresh_env: false,
            deny_all: false,
            deny_read: false,
            deny_write: false,
            deny_net: false,
            deny_env: false,
            allow_read: vec![],
            allow_write: vec![],
            allow_net: vec![],
            allow_env: vec![],
        }
        .run()
        .await
    }
}

struct BootstrapFollowUp {
    dry_run: bool,
    items: Vec<String>,
    printed: bool,
}

impl BootstrapFollowUp {
    fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            items: vec![],
            printed: false,
        }
    }

    fn add_package_skips(&mut self, mgrs: &[system::ManagerPackages]) {
        for mp in mgrs {
            let name = mp.manager.name();
            let reason = if mp.disabled {
                Some("excluded by the system_packages.managers setting".to_string())
            } else if !mp.manager.is_available() {
                Some(mp.manager.unavailable_reason())
            } else {
                None
            };
            if let Some(reason) = reason {
                self.add_skipped(format!(
                    "{name}: {} package(s) skipped ({reason})",
                    mp.requests.len()
                ));
            }
        }
    }

    fn add_macos_defaults(&mut self) {
        self.items.push(
            "relaunch apps that read changed macOS defaults (for example: `killall Dock`, \
             `killall Finder`, `killall SystemUIServer`)"
                .to_string(),
        );
    }

    fn add_login_shell(&mut self, shell: &str) {
        self.items.push(format!(
            "start a new login session for {shell} to take effect"
        ));
    }

    fn add_skipped(&mut self, message: String) {
        self.items.push(message);
    }

    fn print(&mut self) -> Result<()> {
        self.printed = true;
        self.print_inner()
    }

    fn print_best_effort(&mut self) {
        if self.printed {
            return;
        }
        self.printed = true;
        if let Err(err) = self.print_inner() {
            debug!("bootstrap: failed to print follow-up after error: {err}");
        }
    }

    fn print_inner(&self) -> Result<()> {
        if self.items.is_empty() {
            return Ok(());
        }
        if self.dry_run {
            miseprintln!("bootstrap: follow-up if applied");
        } else {
            miseprintln!("bootstrap: follow-up");
        }
        for item in &self.items {
            miseprintln!("  - {item}");
        }
        Ok(())
    }
}

impl Drop for BootstrapFollowUp {
    fn drop(&mut self) {
        self.print_best_effort();
    }
}

fn dotfile_mise_config_body(config: &Config, file: &FileRequest) -> Result<String> {
    match file.mode {
        FileMode::Template => system::files::render_template(config, file),
        _ => crate::file::read_to_string(&file.source),
    }
}

fn parse_mise_config_body(
    path: &std::path::Path,
    body: &str,
) -> Result<Arc<dyn config::config_file::ConfigFile>> {
    Ok(Arc::new(
        config::config_file::mise_toml::MiseToml::from_str(body, path)?,
    ))
}

fn is_mise_config_target(path: &std::path::Path) -> bool {
    path.starts_with(*dirs::CONFIG)
        || path.starts_with(*dirs::SYSTEM_CONFIG)
        || config::DEFAULT_CONFIG_FILENAMES.iter().any(|filename| {
            filename.ends_with(".toml") && !filename.contains('*') && path.ends_with(filename)
        })
        || (path.extension().is_some_and(|ext| ext == "toml")
            && path
                .parent()
                .is_some_and(|parent| parent.ends_with(".config/mise/conf.d")))
}

impl Commands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Dotfiles(cmd) => cmd.run().await,
            Self::Launchd(cmd) => cmd.run().await,
            Self::Linux(cmd) => cmd.run().await,
            Self::Macos(cmd) => cmd.run().await,
            Self::MacosDefaults(cmd) => cmd.run().await,
            Self::MiseShellActivate(cmd) => cmd.run().await,
            Self::Packages(cmd) => cmd.run().await,
            Self::Repos(cmd) => cmd.run().await,
            Self::Status(cmd) => cmd.run().await,
            Self::Systemd(cmd) => cmd.run().await,
            Self::User(cmd) => cmd.run().await,
        }
    }
}

struct BootstrapStatusReport {
    rows: Vec<Vec<String>>,
    json: serde_json::Map<String, Value>,
    any_missing: bool,
}

impl BootstrapStatusReport {
    fn new() -> Self {
        Self {
            rows: vec![],
            json: serde_json::Map::new(),
            any_missing: false,
        }
    }

    fn row(
        &mut self,
        part: impl Into<String>,
        item: impl Into<String>,
        current: impl Into<String>,
        state: impl Into<String>,
        missing: bool,
    ) {
        self.any_missing |= missing;
        self.rows
            .push(vec![part.into(), item.into(), current.into(), state.into()]);
    }
}

impl BootstrapStatus {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        let config = Config::get().await?;
        let report = self.collect(&config).await?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&report.json)?);
        } else if report.rows.is_empty() {
            info!("nothing configured for bootstrap");
        } else {
            let mut table = MiseTable::new(false, &["Part", "Item", "Current", "State"]);
            for row in report.rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && report.any_missing {
            crate::exit(1);
        }
        Ok(())
    }

    async fn collect(&self, config: &Arc<Config>) -> Result<BootstrapStatusReport> {
        let mut report = BootstrapStatusReport::new();
        self.collect_packages(config, &mut report).await?;
        self.collect_repos(config, &mut report)?;
        self.collect_dotfiles(config, &mut report)?;
        self.collect_shell(config, &mut report)?;
        self.collect_defaults(config, &mut report).await?;
        self.collect_launchd(config, &mut report).await?;
        self.collect_systemd(config, &mut report).await?;
        self.collect_user(config, &mut report)?;
        self.collect_tools(config, &mut report).await?;
        Ok(report)
    }

    async fn collect_packages(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let mut json_out = serde_json::Map::new();
        for mp in system::packages_from_config(config) {
            let name = mp.manager.name();
            if mp.disabled || !mp.manager.is_available() {
                let reason = if mp.disabled {
                    "excluded by the system_packages.managers setting".to_string()
                } else {
                    mp.manager.unavailable_reason()
                };
                for req in &mp.requests {
                    report.row(
                        "packages",
                        format!("{name}:{req}"),
                        "",
                        format!("skipped ({reason})"),
                        false,
                    );
                }
                json_out.insert(
                    name.to_string(),
                    json!({
                        "available": false,
                        "reason": reason,
                        "packages": mp.requests.iter().map(|req| {
                            json!({
                                "package": req.name,
                                "requested_version": req.version.clone().unwrap_or_else(|| "latest".to_string()),
                                "state": "skipped",
                            })
                        }).collect::<Vec<_>>(),
                    }),
                );
                continue;
            }
            let statuses = mp.manager.installed(&mp.requests).await?;
            let mut json_pkgs = vec![];
            for s in statuses {
                let (installed_version, state, missing) = match &s.state {
                    PackageState::Installed { version } => (version.clone(), "installed", false),
                    PackageState::Missing => ("".to_string(), "missing", true),
                    PackageState::VersionMismatch { installed } => {
                        (installed.clone(), "version mismatch", true)
                    }
                };
                report.row(
                    "packages",
                    format!("{name}:{}", s.request),
                    installed_version.clone(),
                    state,
                    missing,
                );
                json_pkgs.push(json!({
                    "package": s.request.name,
                    "requested_version": s.request.version.clone().unwrap_or_else(|| "latest".to_string()),
                    "state": state.replace(' ', "_"),
                    "installed_version": installed_version,
                }));
            }
            json_out.insert(
                name.to_string(),
                json!({ "available": true, "packages": json_pkgs }),
            );
        }
        report.json.insert("packages".to_string(), json!(json_out));
        Ok(())
    }

    fn collect_repos(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let repos = system::repos_from_config(config);
        let mut json_entries = vec![];
        for s in system::repos::status(&repos)? {
            let state = s.state.as_str();
            let (row_state, reason, missing) = match &s.state {
                RepoState::Current => ("current".to_string(), "".to_string(), false),
                RepoState::Missing => ("missing".to_string(), "".to_string(), true),
                RepoState::Differs => ("differs".to_string(), "".to_string(), true),
                RepoState::Dirty => (
                    "dirty (local changes)".to_string(),
                    "local changes".to_string(),
                    true,
                ),
                RepoState::Conflict(reason) => {
                    (format!("conflict ({reason})"), reason.clone(), true)
                }
            };
            report.row(
                "repos",
                s.request.path_raw.clone(),
                s.current_ref.clone().unwrap_or_default(),
                row_state,
                missing,
            );
            json_entries.push(json!({
                "path": s.request.path,
                "path_raw": s.request.path_raw,
                "url": s.request.url,
                "ref": s.request.git_ref,
                "origin": s.origin,
                "current_ref": s.current_ref,
                "current_sha": s.current_sha,
                "state": state,
                "reason": reason,
            }));
        }
        report.json.insert("repos".to_string(), json!(json_entries));
        Ok(())
    }

    fn collect_dotfiles(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let mut json_files = vec![];
        for req in system::files::files_from_config(config) {
            let state = match system::files::check(config, &req) {
                Ok(state) => state,
                Err(err) => system::files::FileState::Differs(format!("{err}")),
            };
            let (state_str, state_json, missing) = match &state {
                system::files::FileState::Applied => ("applied".to_string(), "applied", false),
                system::files::FileState::Missing => ("missing".to_string(), "missing", true),
                system::files::FileState::SourceMissing => {
                    ("source missing".to_string(), "source_missing", true)
                }
                system::files::FileState::Differs(reason) => {
                    (format!("differs ({reason})"), "differs", true)
                }
            };
            report.row(
                "dotfiles",
                req.target_raw.clone(),
                format!("{} {}", req.mode.name(), req.source.display_user()),
                state_str,
                missing,
            );
            json_files.push(json!({
                "target": req.target_raw,
                "source": req.source.display_user(),
                "mode": req.mode.name(),
                "state": state_json,
            }));
        }

        let mut json_edits = vec![];
        for req in system::edits::edits_from_config(config) {
            let state = match system::edits::check(config, &req) {
                Ok(state) => state,
                Err(err) => system::files::FileState::Differs(format!("{err}")),
            };
            let (state_str, state_json, missing) = match &state {
                system::files::FileState::Applied => ("applied".to_string(), "applied", false),
                system::files::FileState::Missing => ("missing".to_string(), "missing", true),
                system::files::FileState::SourceMissing => {
                    ("source missing".to_string(), "source_missing", true)
                }
                system::files::FileState::Differs(reason) => {
                    (format!("differs ({reason})"), "differs", true)
                }
            };
            report.row(
                "dotfiles",
                req.path_raw.clone(),
                req.describe_op(),
                state_str,
                missing,
            );
            json_edits.push(json!({
                "path": req.path_raw,
                "edit": req.describe_op(),
                "state": state_json,
            }));
        }
        report.json.insert(
            "dotfiles".to_string(),
            json!({ "files": json_files, "edits": json_edits }),
        );
        Ok(())
    }

    fn collect_shell(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let activations = system::shell_activation_from_config(config);
        let mut json_entries = vec![];
        for request in &activations {
            let state = match system::edits::check(config, &request.edit) {
                Ok(state) => state,
                Err(err) => FileState::Differs(format!("{err}")),
            };
            let missing = state != FileState::Applied;
            report.row(
                "shell",
                request.target.name(),
                format!(
                    "{} {} {}",
                    request.shell.name(),
                    request.edit.path_raw,
                    request.mode.name()
                ),
                file_state_display(&state),
                missing,
            );
            let mut entry = json!({
                "target": request.target.name(),
                "shell": request.shell.name(),
                "path": request.edit.path_raw,
                "mode": request.mode.name(),
                "state": file_state_json(&state),
            });
            if let FileState::Differs(reason) = &state {
                entry["reason"] = json!(reason);
            }
            json_entries.push(entry);
        }
        report
            .json
            .insert("mise_shell_activate".to_string(), json!(json_entries));
        Ok(())
    }

    async fn collect_defaults(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let defaults = system::defaults_from_config(config);
        if defaults.is_empty() {
            report
                .json
                .insert("macos_defaults".to_string(), json!({ "entries": [] }));
            return Ok(());
        }
        if !system::defaults::is_available() {
            let reason = system::defaults::unavailable_reason();
            for req in &defaults {
                report.row(
                    "defaults",
                    format!("{} {}", req.domain, req.key),
                    "",
                    format!("skipped ({reason})"),
                    false,
                );
            }
            report.json.insert(
                "macos_defaults".to_string(),
                json!({
                    "available": false,
                    "reason": reason,
                    "entries": defaults.iter().map(|req| {
                        json!({
                            "domain": req.domain,
                            "key": req.key,
                            "value": req.value.to_json(),
                            "state": "skipped",
                        })
                    }).collect::<Vec<_>>(),
                }),
            );
            return Ok(());
        }

        let mut json_entries = vec![];
        for s in system::defaults::status(&defaults).await? {
            let (current, state, missing) = match &s.state {
                DefaultsState::Set => (s.request.value.to_string(), "set", false),
                DefaultsState::Differs { current } => (current.clone(), "differs", true),
                DefaultsState::Unset => ("".to_string(), "unset", true),
            };
            report.row(
                "defaults",
                format!("{} {}", s.request.domain, s.request.key),
                current.clone(),
                state,
                missing,
            );
            json_entries.push(json!({
                "domain": s.request.domain,
                "key": s.request.key,
                "value": s.request.value.to_json(),
                "current": current,
                "state": state,
            }));
        }
        report.json.insert(
            "macos_defaults".to_string(),
            json!({ "available": true, "entries": json_entries }),
        );
        Ok(())
    }

    async fn collect_launchd(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let agents = system::launchd_from_config(config);
        if agents.is_empty() {
            report
                .json
                .insert("launchd".to_string(), json!({ "agents": [] }));
            return Ok(());
        }
        if !system::launchd::is_available() {
            let reason = system::launchd::unavailable_reason();
            for req in &agents {
                report.row(
                    "launchd",
                    req.name.clone(),
                    req.label.clone(),
                    format!("skipped ({reason})"),
                    false,
                );
            }
            report.json.insert(
                "launchd".to_string(),
                json!({
                    "available": false,
                    "reason": reason,
                    "agents": agents.iter().map(|req| {
                        json!({
                            "name": req.name,
                            "label": req.label,
                            "state": "skipped",
                        })
                    }).collect::<Vec<_>>(),
                }),
            );
            return Ok(());
        }

        let mut json_entries = vec![];
        for s in system::launchd::status(&agents).await? {
            let (state, missing) = match &s.state {
                LaunchdState::Loaded => ("loaded", false),
                LaunchdState::Unloaded => ("unloaded", true),
                LaunchdState::Differs => ("differs", true),
                LaunchdState::Missing => ("missing", true),
            };
            report.row(
                "launchd",
                s.request.name.clone(),
                s.path.display().to_string(),
                state,
                missing,
            );
            json_entries.push(json!({
                "name": s.request.name,
                "label": s.request.label,
                "path": s.path,
                "loaded": s.loaded,
                "state": state,
            }));
        }
        report.json.insert(
            "launchd".to_string(),
            json!({ "available": true, "agents": json_entries }),
        );
        Ok(())
    }

    async fn collect_systemd(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let units = system::systemd_from_config(config);
        if units.is_empty() {
            report
                .json
                .insert("systemd".to_string(), json!({ "units": [] }));
            return Ok(());
        }
        if !system::systemd::is_available() {
            let reason = system::systemd::unavailable_reason();
            for req in &units {
                report.row(
                    "systemd",
                    req.name.clone(),
                    req.unit.clone(),
                    format!("skipped ({reason})"),
                    false,
                );
            }
            report.json.insert(
                "systemd".to_string(),
                json!({
                    "available": false,
                    "reason": reason,
                    "units": units.iter().map(|req| {
                        json!({
                            "name": req.name,
                            "unit": req.unit,
                            "state": "skipped",
                        })
                    }).collect::<Vec<_>>(),
                }),
            );
            return Ok(());
        }

        let mut json_entries = vec![];
        for s in system::systemd::status(&units).await? {
            let desired = s.is_desired();
            let state = match &s.state {
                SystemdState::Active => "active",
                SystemdState::Inactive => "inactive",
                SystemdState::Differs => "differs",
                SystemdState::Missing => "missing",
            };
            let missing = !desired;
            report.row(
                "systemd",
                s.request.name.clone(),
                s.path.display().to_string(),
                state,
                missing,
            );
            json_entries.push(json!({
                "name": s.request.name,
                "unit": s.request.unit,
                "path": s.path,
                "active": s.active,
                "enabled": s.enabled,
                "desired": desired,
                "state": state,
            }));
        }
        report.json.insert(
            "systemd".to_string(),
            json!({ "available": true, "units": json_entries }),
        );
        Ok(())
    }

    fn collect_user(&self, config: &Arc<Config>, report: &mut BootstrapStatusReport) -> Result<()> {
        let Some(req) = system::login_shell_from_config(config) else {
            report.json.insert("login_shell".to_string(), json!(null));
            return Ok(());
        };
        if !system::login_shell::is_available() {
            let reason = system::login_shell::unavailable_reason();
            report.row(
                "user",
                "login_shell",
                "",
                format!("skipped ({reason})"),
                false,
            );
            report.json.insert(
                "login_shell".to_string(),
                json!({
                    "available": false,
                    "reason": reason,
                    "shell": req.shell,
                    "state": "skipped",
                }),
            );
            return Ok(());
        }

        let status = system::login_shell::status(&req)?;
        let (row_state, json_state, missing) = match &status.state {
            LoginShellState::Set => ("set", "set", false),
            LoginShellState::Differs { .. } => ("differs", "differs", true),
            LoginShellState::MissingFromShells { .. } => {
                ("missing from /etc/shells", "missing_from_shells", true)
            }
        };
        report.row(
            "user",
            "login_shell",
            status.current.clone(),
            row_state,
            missing,
        );
        report.json.insert(
            "login_shell".to_string(),
            json!({
                "available": true,
                "shell": status.request.shell,
                "user": status.user,
                "current": status.current,
                "shell_listed": status.shell_listed,
                "state": json_state,
            }),
        );
        Ok(())
    }

    async fn collect_tools(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let trs = config.get_tool_request_set().await?;
        let mut json_tools = vec![];
        for ba in &trs.unknown_tools {
            report.row("tools", ba.to_string(), "", "unknown", true);
            json_tools.push(json!({
                "tool": ba.to_string(),
                "requested_version": null,
                "resolved_version": null,
                "state": "unknown",
                "installed": false,
            }));
        }
        for tr in trs.tools.values().flatten() {
            if !tr.is_os_supported() {
                continue;
            }
            let item = tr.to_string();
            let resolved = match tr.resolve(config, &ResolveOptions::default()).await {
                Ok(tv) => tv,
                Err(err) => {
                    let err = format!("{err:#}");
                    report.row(
                        "tools",
                        item.clone(),
                        "",
                        format!("resolve error ({err})"),
                        true,
                    );
                    json_tools.push(json!({
                        "tool": tr.ba().to_string(),
                        "requested_version": tr.version(),
                        "resolved_version": null,
                        "state": "resolve_error",
                        "installed": false,
                        "error": err,
                    }));
                    continue;
                }
            };
            let installed = {
                crate::backend::get(tr.ba())
                    .is_some_and(|backend| backend.is_version_installed(config, &resolved, true))
            };
            let resolved_version = resolved.version;
            let state = if installed { "installed" } else { "missing" };
            report.row(
                "tools",
                item.clone(),
                resolved_version.clone(),
                state,
                !installed,
            );
            json_tools.push(json!({
                "tool": tr.ba().to_string(),
                "requested_version": tr.version(),
                "resolved_version": resolved_version,
                "state": state,
                "installed": installed,
            }));
        }
        report.json.insert("tools".to_string(), json!(json_tools));
        Ok(())
    }
}

impl BootstrapDotfiles {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapDotfilesCommands::Apply(cmd) => cmd.run().await,
            BootstrapDotfilesCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapDotfilesApply {
    async fn run(self) -> Result<()> {
        self.cmd.run().await
    }
}

impl BootstrapDotfilesStatus {
    async fn run(self) -> Result<()> {
        self.cmd.run().await
    }
}

impl BootstrapPackages {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapPackagesCommands::Apply(cmd) => cmd.run().await,
            #[cfg(unix)]
            BootstrapPackagesCommands::Brew(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Import(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Prune(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Status(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Upgrade(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Use(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapRepos {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapReposCommands::Apply(cmd) => cmd.run().await,
            BootstrapReposCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapReposApply {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_repos(system::repos_from_config(&config), self.dry_run, self.yes)
    }
}

impl BootstrapReposStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let repos = system::repos_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_entries = vec![];
        for s in system::repos::status(&repos)? {
            if !s.state.is_current() {
                any_missing = true;
            }
            let state = s.state.as_str();
            let reason = match &s.state {
                RepoState::Conflict(reason) => reason.clone(),
                RepoState::Dirty => "local changes".to_string(),
                RepoState::Current | RepoState::Missing | RepoState::Differs => "".to_string(),
            };
            if self.json {
                json_entries.push(json!({
                    "path": s.request.path,
                    "path_raw": s.request.path_raw,
                    "url": s.request.url,
                    "ref": s.request.git_ref,
                    "origin": s.origin,
                    "current_ref": s.current_ref,
                    "current_sha": s.current_sha,
                    "state": state,
                    "reason": reason,
                }));
            } else {
                rows.push(vec![
                    s.request.path_raw,
                    s.request.url,
                    s.request.git_ref.unwrap_or_default(),
                    state.to_string(),
                    reason,
                ]);
            }
        }
        if self.json {
            let mut json_out = serde_json::Map::new();
            json_out.insert("repos".to_string(), json!(json_entries));
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.repos]");
        } else {
            let mut table = MiseTable::new(false, &["Path", "URL", "Ref", "State", "Reason"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            crate::exit(1);
        }
        Ok(())
    }
}

impl BootstrapMacos {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapMacosCommands::Defaults(cmd) => cmd.run().await,
            BootstrapMacosCommands::LaunchdAgents(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapLinux {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapLinuxCommands::SystemdUnits(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapMacosDefaults {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapMacosDefaultsCommands::Apply(cmd) => cmd.run().await,
            BootstrapMacosDefaultsCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapLaunchd {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapLaunchdCommands::Apply(cmd) => cmd.run().await,
            BootstrapLaunchdCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapLaunchdApply {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_launchd(system::launchd_from_config(&config), self.dry_run, self.yes).await
    }
}

impl BootstrapLaunchdStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let agents = system::launchd_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        if !agents.is_empty() {
            if !system::launchd::is_available() {
                let reason = system::launchd::unavailable_reason();
                if self.json {
                    json_out.insert(
                        "launchd".to_string(),
                        json!({ "available": false, "reason": reason }),
                    );
                } else {
                    for req in &agents {
                        rows.push(vec![
                            req.name.clone(),
                            req.label.clone(),
                            "".to_string(),
                            format!("skipped ({reason})"),
                        ]);
                    }
                }
            } else {
                let statuses = system::launchd::status(&agents).await?;
                let mut json_entries = vec![];
                for s in statuses {
                    let state = match &s.state {
                        LaunchdState::Loaded => "loaded",
                        LaunchdState::Unloaded => {
                            any_missing = true;
                            "unloaded"
                        }
                        LaunchdState::Differs => {
                            any_missing = true;
                            "differs"
                        }
                        LaunchdState::Missing => {
                            any_missing = true;
                            "missing"
                        }
                    };
                    if self.json {
                        json_entries.push(json!({
                            "name": s.request.name,
                            "label": s.request.label,
                            "path": s.path,
                            "loaded": s.loaded,
                            "state": state,
                        }));
                    } else {
                        rows.push(vec![
                            s.request.name.clone(),
                            s.request.label.clone(),
                            s.path.display().to_string(),
                            state.to_string(),
                        ]);
                    }
                }
                if self.json {
                    json_out.insert(
                        "launchd".to_string(),
                        json!({ "available": true, "agents": json_entries }),
                    );
                }
            }
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.macos.launchd.agents]");
        } else {
            let mut table = MiseTable::new(false, &["Name", "Label", "Path", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            crate::exit(1);
        }
        Ok(())
    }
}

impl BootstrapSystemd {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapSystemdCommands::Apply(cmd) => cmd.run().await,
            BootstrapSystemdCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapSystemdApply {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_systemd(system::systemd_from_config(&config), self.dry_run, self.yes).await
    }
}

impl BootstrapSystemdStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let units = system::systemd_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        if !units.is_empty() {
            if !system::systemd::is_available() {
                let reason = system::systemd::unavailable_reason();
                if self.json {
                    json_out.insert(
                        "systemd".to_string(),
                        json!({ "available": false, "reason": reason }),
                    );
                } else {
                    for req in &units {
                        rows.push(vec![
                            req.name.clone(),
                            req.unit.clone(),
                            "".to_string(),
                            format!("skipped ({reason})"),
                        ]);
                    }
                }
            } else {
                let statuses = system::systemd::status(&units).await?;
                let mut json_entries = vec![];
                for s in statuses {
                    let desired = s.is_desired();
                    let state = match &s.state {
                        SystemdState::Active => "active",
                        SystemdState::Inactive => "inactive",
                        SystemdState::Differs => {
                            any_missing = true;
                            "differs"
                        }
                        SystemdState::Missing => {
                            any_missing = true;
                            "missing"
                        }
                    };
                    if !desired {
                        any_missing = true;
                    }
                    if self.json {
                        json_entries.push(json!({
                            "name": s.request.name,
                            "unit": s.request.unit,
                            "path": s.path,
                            "active": s.active,
                            "enabled": s.enabled,
                            "desired": desired,
                            "state": state,
                        }));
                    } else {
                        rows.push(vec![
                            s.request.name.clone(),
                            s.request.unit.clone(),
                            s.path.display().to_string(),
                            state.to_string(),
                        ]);
                    }
                }
                if self.json {
                    json_out.insert(
                        "systemd".to_string(),
                        json!({ "available": true, "units": json_entries }),
                    );
                }
            }
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.linux.systemd.units]");
        } else {
            let mut table = MiseTable::new(false, &["Name", "Unit", "Path", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            crate::exit(1);
        }
        Ok(())
    }
}

impl BootstrapMacosDefaultsApply {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_defaults(
            system::defaults_from_config(&config),
            self.dry_run,
            self.yes,
        )
        .await
    }
}

impl BootstrapMacosDefaultsStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let defaults = system::defaults_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        if !defaults.is_empty() {
            if !system::defaults::is_available() {
                let reason = system::defaults::unavailable_reason();
                if self.json {
                    json_out.insert(
                        "macos_defaults".to_string(),
                        json!({ "available": false, "reason": reason }),
                    );
                } else {
                    for req in &defaults {
                        rows.push(vec![
                            req.domain.clone(),
                            req.key.clone(),
                            req.value.to_string(),
                            "".to_string(),
                            format!("skipped ({reason})"),
                        ]);
                    }
                }
            } else {
                let statuses = system::defaults::status(&defaults).await?;
                let mut json_entries = vec![];
                for s in statuses {
                    let (current, state) = match &s.state {
                        DefaultsState::Set => (s.request.value.to_string(), "set"),
                        DefaultsState::Differs { current } => {
                            any_missing = true;
                            (current.clone(), "differs")
                        }
                        DefaultsState::Unset => {
                            any_missing = true;
                            ("".to_string(), "unset")
                        }
                    };
                    if self.json {
                        json_entries.push(json!({
                            "domain": s.request.domain,
                            "key": s.request.key,
                            "value": s.request.value.to_json(),
                            "current": current,
                            "state": state,
                        }));
                    } else {
                        rows.push(vec![
                            s.request.domain.clone(),
                            s.request.key.clone(),
                            s.request.value.to_string(),
                            current,
                            state.to_string(),
                        ]);
                    }
                }
                if self.json {
                    json_out.insert(
                        "macos_defaults".to_string(),
                        json!({ "available": true, "entries": json_entries }),
                    );
                }
            }
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.macos.defaults]");
        } else {
            let mut table = MiseTable::new(false, &["Domain", "Key", "Value", "Current", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            crate::exit(1);
        }
        Ok(())
    }
}

impl BootstrapShell {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapShellCommands::Apply(cmd) => cmd.run().await,
            BootstrapShellCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapShellApply {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_shell_activation(
            &config,
            system::shell_activation_from_config(&config),
            self.dry_run,
            self.yes,
        )
    }
}

impl BootstrapShellStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let activations = system::shell_activation_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_entries = vec![];
        for request in &activations {
            let state = match system::edits::check(&config, &request.edit) {
                Ok(state) => state,
                Err(err) => FileState::Differs(format!("{err}")),
            };
            any_missing |= state != FileState::Applied;
            if self.json {
                let mut entry = json!({
                    "target": request.target.name(),
                    "shell": request.shell.name(),
                    "path": request.edit.path_raw,
                    "mode": request.mode.name(),
                    "state": file_state_json(&state),
                });
                if let FileState::Differs(reason) = &state {
                    entry["reason"] = json!(reason);
                }
                json_entries.push(entry);
            } else {
                rows.push(vec![
                    request.target.name().to_string(),
                    request.shell.name().to_string(),
                    request.edit.path_raw.clone(),
                    request.mode.name().to_string(),
                    file_state_display(&state),
                ]);
            }
        }
        if self.json {
            miseprintln!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "mise_shell_activate": json_entries,
                }))?
            );
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.mise_shell_activate]");
        } else {
            let mut table = MiseTable::new(false, &["Target", "Shell", "Path", "Mode", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            crate::exit(1);
        }
        Ok(())
    }
}

impl BootstrapUser {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            BootstrapUserCommands::Apply(cmd) => cmd.run().await,
            BootstrapUserCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapUserApply {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_login_shell(
            system::login_shell_from_config(&config),
            self.dry_run,
            self.yes,
        )
    }
}

impl BootstrapUserStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let login_shell = system::login_shell_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        if let Some(req) = login_shell {
            if !system::login_shell::is_available() {
                let reason = system::login_shell::unavailable_reason();
                if self.json {
                    json_out.insert(
                        "login_shell".to_string(),
                        json!({
                            "available": false,
                            "reason": reason,
                            "shell": req.shell,
                        }),
                    );
                } else {
                    rows.push(vec![
                        req.shell,
                        "".to_string(),
                        format!("skipped ({reason})"),
                    ]);
                }
            } else {
                let status = system::login_shell::status(&req)?;
                let state = match &status.state {
                    LoginShellState::Set => "set",
                    LoginShellState::Differs { .. } => {
                        any_missing = true;
                        "differs"
                    }
                    LoginShellState::MissingFromShells { .. } => {
                        any_missing = true;
                        "missing from /etc/shells"
                    }
                };
                if self.json {
                    json_out.insert(
                        "login_shell".to_string(),
                        json!({
                            "available": true,
                            "shell": status.request.shell,
                            "user": status.user,
                            "current": status.current,
                            "shell_listed": status.shell_listed,
                            "state": state,
                        }),
                    );
                } else {
                    rows.push(vec![
                        status.request.shell,
                        status.current,
                        state.to_string(),
                    ]);
                }
            }
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.user]");
        } else {
            let mut table = MiseTable::new(false, &["Shell", "Current", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            crate::exit(1);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap</bold>                    # packages + repos + dotfiles + tools + bootstrap task
    $ <bold>mise bootstrap --force-dotfiles</bold>   # replace conflicting dotfile targets
    $ <bold>mise bootstrap --skip tools,task</bold>  # skip tool installation and the bootstrap task
    $ <bold>mise bootstrap --only tools</bold>       # run just tool installation
    $ <bold>mise bootstrap status --missing</bold>
    $ <bold>mise bootstrap packages apply --yes</bold>
    $ <bold>mise bootstrap repos status</bold>
    $ <bold>mise bootstrap repos apply --dry-run</bold>
    $ <bold>mise bootstrap dotfiles status</bold>
    $ <bold>mise bootstrap mise-shell-activate apply --dry-run</bold>
    $ <bold>mise bootstrap macos defaults status</bold>
    $ <bold>mise bootstrap macos launchd-agents apply --dry-run</bold>
    $ <bold>mise bootstrap linux systemd-units apply --dry-run</bold>
    $ <bold>mise bootstrap user apply --dry-run</bold>
"#
);

fn file_state_display(state: &FileState) -> String {
    match state {
        FileState::Applied => "applied".to_string(),
        FileState::Missing => "missing".to_string(),
        FileState::SourceMissing => "source missing".to_string(),
        FileState::Differs(reason) => format!("differs ({reason})"),
    }
}

fn file_state_json(state: &FileState) -> &'static str {
    match state {
        FileState::Applied => "applied",
        FileState::Missing => "missing",
        FileState::SourceMissing => "source_missing",
        FileState::Differs(_) => "differs",
    }
}
