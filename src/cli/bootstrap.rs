use std::collections::HashSet;
use std::sync::Arc;

use eyre::Result;
use serde_json::json;

use super::install::Install;
use super::run;
use super::system::driver::{self, Action, DriverOpts};
use super::system::{install, status, upgrade, r#use};
use crate::config::{self, Config, Settings};
use crate::dirs;
use crate::system;
use crate::system::defaults::DefaultsState;
use crate::system::files::{FileMode, FileRequest};
use crate::system::hooks::{self, BootstrapHookPhase};
use crate::system::launchd::LaunchdState;
use crate::system::login_shell::LoginShellState;
use crate::system::systemd::SystemdState;
use crate::ui::table::MiseTable;
use clap::{Subcommand, ValueEnum};

/// [experimental] Set up a machine for the current config in one command
///
/// Runs the bootstrap steps for the current config in order:
///
/// 0. `[bootstrap.hooks.pre-packages]` — optional setup hook
/// 1. `mise bootstrap packages install` — install missing
///    `[bootstrap.packages]`
///    then `[bootstrap.hooks.post-packages]`
/// 2. `mise dotfiles apply` — apply dotfiles from `[dotfiles]`
///    surrounded by `pre-dotfiles`/`post-dotfiles` hooks
/// 3. `mise bootstrap macos-defaults apply` — write
///    `[bootstrap.macos.defaults]` entries (macOS)
///    surrounded by `pre-defaults`/`post-defaults` hooks
/// 4. `mise bootstrap launchd apply` — install/load macOS LaunchAgents
/// 5. `mise bootstrap systemd apply` — install/start systemd user services
///    (Linux)
/// 6. `mise bootstrap user apply` — set `[bootstrap.user].login_shell`
///    (Unix)
///    surrounded by `pre-user`/`post-user` hooks
/// 7. `mise install` — install missing tools from `[tools]`
///    surrounded by `pre-tools`/`post-tools` hooks
/// 8. `mise run bootstrap` — if a task named `bootstrap` is defined
/// 9. `[bootstrap.hooks.final]` — optional final hook
///
/// The declarative steps converge — anything already in its desired state
/// is skipped, so re-running is safe. The `bootstrap` task runs on every
/// invocation; keep it idempotent. Use it for any project-specific setup
/// that doesn't fit the declarative sections (cloning repos, seeding
/// databases, etc.) — it runs with the installed tools on PATH.
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
    Dotfiles,
    Defaults,
    Launchd,
    Systemd,
    User,
    Tools,
    Task,
    FinalHook,
}

impl BootstrapPart {
    // Keep this in sync with every enum variant. `--only` computes a
    // complement from ALL, so an omitted variant would always run.
    const ALL: [Self; 9] = [
        Self::Packages,
        Self::Dotfiles,
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
    Launchd(BootstrapLaunchd),
    MacosDefaults(BootstrapMacosDefaults),
    Packages(BootstrapPackages),
    Systemd(BootstrapSystemd),
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
                hooks = self.hooks_after_dotfiles_dry_run(&config, &files)?;
            } else {
                config = Config::reset().await?;
                hooks = system::hooks_from_config(&config);
            }
            self.run_hooks(&hooks, BootstrapHookPhase::PostDotfiles)
                .await?;
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
                if report.changed {
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
                if report.changed
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

    fn hooks_after_dotfiles_dry_run(
        &self,
        config: &Config,
        files: &[FileRequest],
    ) -> Result<Vec<hooks::BootstrapHook>> {
        let mut config_files = config.config_files.clone();
        for file in files {
            if !is_mise_config_target(&file.target) || !file.source.is_file() {
                continue;
            }
            match parse_dotfile_mise_config(config, file) {
                Ok(cf) => {
                    config_files.insert(file.target.clone(), cf);
                }
                Err(err) => {
                    warn!(
                        "[dotfiles].\"{}\": failed to parse config source {}: {err}",
                        file.target_raw,
                        file.source.display()
                    );
                }
            }
        }
        Ok(system::hooks_from_config_files(&config_files))
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
}

impl BootstrapFollowUp {
    fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            items: vec![],
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

    fn print(&self) -> Result<()> {
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

fn parse_dotfile_mise_config(
    config: &Config,
    file: &FileRequest,
) -> Result<Arc<dyn config::config_file::ConfigFile>> {
    let body = match file.mode {
        FileMode::Template => system::files::render_template(config, file)?,
        _ => crate::file::read_to_string(&file.source)?,
    };
    Ok(Arc::new(
        config::config_file::mise_toml::MiseToml::from_str(&body, &file.target)?,
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
            Self::Launchd(cmd) => cmd.run().await,
            Self::MacosDefaults(cmd) => cmd.run().await,
            Self::Packages(cmd) => cmd.run().await,
            Self::Systemd(cmd) => cmd.run().await,
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
    $ <bold>mise bootstrap --force-dotfiles</bold>   # replace conflicting dotfile targets
    $ <bold>mise bootstrap --skip tools,task</bold>  # skip tool installation and the bootstrap task
    $ <bold>mise bootstrap --only tools</bold>       # run just tool installation
    $ <bold>mise bootstrap packages install --yes</bold>
    $ <bold>mise bootstrap macos-defaults status</bold>
    $ <bold>mise bootstrap launchd apply --dry-run</bold>
    $ <bold>mise bootstrap systemd apply --dry-run</bold>
    $ <bold>mise bootstrap user apply --dry-run</bold>
"#
);
