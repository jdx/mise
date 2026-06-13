use eyre::Result;

use super::install::Install;
use super::run;
use super::system::driver::{self, Action, DriverOpts};
use crate::config::{Config, Settings};
use crate::system;

/// [experimental] Set up a machine for the current config in one command
///
/// Runs the bootstrap steps for the current config in order:
///
/// 1. `mise system install` — install missing `[system.packages]` and write
///    `[system.defaults]` (macOS)
/// 2. `mise dotfiles install` — apply dotfiles from `[dotfiles]`
/// 3. set `[system].login_shell` (Unix)
/// 4. `mise install` — install missing tools from `[tools]`
/// 5. `mise run bootstrap` — if a task named `bootstrap` is defined
///
/// The declarative steps converge — anything already in its desired state
/// is skipped, so re-running is safe. The `bootstrap` task runs on every
/// invocation; keep it idempotent. Use it for any project-specific setup
/// that doesn't fit the declarative sections (cloning repos, seeding
/// databases, etc.) — it runs with the installed tools on PATH.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Bootstrap {
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

impl Bootstrap {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        let config = Config::get().await?;

        let mgrs = system::packages_from_config(&config);
        if mgrs.is_empty() {
            debug!("bootstrap: no [system.packages] configured, skipping");
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
                // conflicts shouldn't be steamrolled by a bootstrap;
                // `mise dotfiles install --force` is the explicit way
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
                yes: self.yes,
            };
            system::edits::apply(&config, &edits, &opts)?;
        }

        let defaults = system::defaults_from_config(&config);
        if defaults.is_empty() {
            debug!("bootstrap: no [system.defaults] configured, skipping");
        } else {
            info!("bootstrap: system defaults");
            super::system::install::apply_defaults(defaults, self.dry_run, self.yes).await?;
        }

        let login_shell = system::login_shell_from_config(&config);
        if login_shell.is_none() {
            debug!("bootstrap: no [system].login_shell configured, skipping");
        } else {
            info!("bootstrap: login shell");
            super::system::install::apply_login_shell(login_shell, self.dry_run, self.yes)?;
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap</bold>            # system packages + tools + bootstrap task
    $ <bold>mise bootstrap --yes</bold>      # don't prompt before installing system packages
    $ <bold>mise bootstrap --dry-run</bold>  # show what would happen
"#
);
