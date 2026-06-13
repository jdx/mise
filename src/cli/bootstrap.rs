use eyre::Result;
use serde_json::json;

use super::install::Install;
use super::run;
use super::system::driver::{self, Action, DriverOpts};
use super::system::{install, status, upgrade, r#use};
use crate::config::{Config, Settings};
use crate::system;
use crate::system::defaults::DefaultsState;
use crate::system::launchd::LaunchdState;
use crate::system::login_shell::LoginShellState;
use crate::ui::table::MiseTable;
use clap::Subcommand;

/// [experimental] Set up a machine for the current config in one command
///
/// Runs the bootstrap steps for the current config in order:
///
/// 1. `mise bootstrap packages install` — install missing
///    `[bootstrap.packages]`
/// 2. `mise dotfiles apply` — apply dotfiles from `[dotfiles]`
/// 3. `mise bootstrap macos-defaults apply` — write
///    `[bootstrap.macos.defaults]` entries (macOS)
/// 4. `mise bootstrap launchd apply` — install/load macOS LaunchAgents
/// 5. `mise bootstrap user apply` — set `[bootstrap.user].login_shell`
///    (Unix)
/// 6. `mise install` — install missing tools from `[tools]`
/// 7. `mise run bootstrap` — if a task named `bootstrap` is defined
///
/// The declarative steps converge — anything already in its desired state
/// is skipped, so re-running is safe. The `bootstrap` task runs on every
/// invocation; keep it idempotent. Use it for any project-specific setup
/// that doesn't fit the declarative sections (cloning repos, seeding
/// databases, etc.) — it runs with the installed tools on PATH.
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

    /// Refresh system package manager metadata first (apt: `apt-get update`)
    #[clap(long)]
    update: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Launchd(BootstrapLaunchd),
    MacosDefaults(BootstrapMacosDefaults),
    Packages(BootstrapPackages),
    User(BootstrapUser),
}

/// Manage bootstrap system packages from `[bootstrap.packages]`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct BootstrapPackages {
    #[clap(subcommand)]
    command: BootstrapPackagesCommands,
}

#[derive(Debug, Subcommand)]
enum BootstrapPackagesCommands {
    #[cfg(unix)]
    Brew(super::system::brew::SystemBrew),
    Install(install::SystemInstall),
    Status(status::SystemStatus),
    Upgrade(upgrade::SystemUpgrade),
    Use(r#use::SystemUse),
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
        let config = Config::get().await?;

        let mgrs = system::packages_from_config(&config);
        if mgrs.is_empty() {
            debug!("bootstrap: no [bootstrap.packages] configured, skipping");
        } else {
            info!("bootstrap: system packages");
            let opts = DriverOpts {
                manager: None,
                explicit: false,
                dry_run: self.dry_run,
                update: self.update,
                yes: self.yes,
            };
            driver::run(mgrs, Action::Install, &opts).await?;
        }

        let files = system::files::files_from_config(&config);
        if files.is_empty() {
            debug!("bootstrap: no whole-file [dotfiles] entries configured, skipping");
        } else {
            info!("bootstrap: dotfiles");
            let opts = system::files::ApplyOpts {
                dry_run: self.dry_run,
                verbose: false,
                // conflicts shouldn't be steamrolled by a bootstrap;
                // `mise dotfiles apply --force` is the explicit way
                force: false,
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

        let defaults = system::defaults_from_config(&config);
        if defaults.is_empty() {
            debug!("bootstrap: no [bootstrap.macos.defaults] configured, skipping");
        } else {
            info!("bootstrap: system defaults");
            install::apply_defaults(defaults, self.dry_run, self.yes).await?;
        }

        let agents = system::launchd_from_config(&config);
        if agents.is_empty() {
            debug!("bootstrap: no [bootstrap.macos.launchd.agents] configured, skipping");
        } else {
            info!("bootstrap: launchd agents");
            install::apply_launchd(agents, self.dry_run, self.yes).await?;
        }

        let login_shell = system::login_shell_from_config(&config);
        if login_shell.is_none() {
            debug!("bootstrap: no [bootstrap.user].login_shell configured, skipping");
        } else {
            info!("bootstrap: login shell");
            install::apply_login_shell(login_shell, self.dry_run, self.yes)?;
        }

        info!("bootstrap: tools");
        Install::new_bare(self.dry_run).run().await?;

        // installs may have changed the env (and `mise install` resets config
        // internally), so re-fetch before looking up tasks
        let config = Config::get().await?;
        let tasks = config.tasks().await?;
        if tasks.iter().any(|(_, t)| t.is_match("bootstrap")) {
            info!("bootstrap: running `bootstrap` task");
            self.run_task("bootstrap").await?;
        } else {
            debug!("bootstrap: no `bootstrap` task defined, skipping");
        }
        Ok(())
    }

    async fn run_task(&self, task: &str) -> Result<()> {
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
            // run) task
            skip_tools: self.dry_run,
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

impl Commands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Launchd(cmd) => cmd.run().await,
            Self::MacosDefaults(cmd) => cmd.run().await,
            Self::Packages(cmd) => cmd.run().await,
            Self::User(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapPackages {
    async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            #[cfg(unix)]
            BootstrapPackagesCommands::Brew(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Install(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Status(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Upgrade(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Use(cmd) => cmd.run().await,
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

    $ <bold>mise bootstrap</bold>                    # packages + dotfiles + tools + bootstrap task
    $ <bold>mise bootstrap packages install --yes</bold>
    $ <bold>mise bootstrap macos-defaults status</bold>
    $ <bold>mise bootstrap launchd apply --dry-run</bold>
    $ <bold>mise bootstrap user apply --dry-run</bold>
"#
);
