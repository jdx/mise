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
/// 1. `mise system install` — install missing `[system.packages]` and apply
///    `[system.files]`
/// 2. `mise install` — install missing tools from `[tools]`
/// 3. `mise run bootstrap` — if a task named `bootstrap` is defined
///
/// Steps with nothing to do are skipped, so `mise bootstrap` is idempotent
/// and safe to re-run. Use a `bootstrap` task for any project-specific setup
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
            debug!("bootstrap: no [system.files] configured, skipping");
        } else {
            info!("bootstrap: system files");
            let opts = system::files::ApplyOpts {
                dry_run: self.dry_run,
                // conflicts shouldn't be steamrolled by a bootstrap;
                // `mise system install --force` is the explicit way
                force: false,
                yes: self.yes,
            };
            system::files::apply(&config, &files, &opts)?;
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
