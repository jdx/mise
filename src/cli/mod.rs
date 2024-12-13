use crate::cli::args::ToolArg;
use crate::config::{Config, Settings};
use crate::exit::exit;
use crate::ui::ctrlc;
use crate::{logger, migrate, shims};
use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use color_eyre::Result;
use std::path::PathBuf;

mod activate;
mod alias;
pub mod args;
mod asdf;
pub mod backends;
mod bin_paths;
mod cache;
mod completion;
mod config;
mod current;
mod deactivate;
mod direnv;
mod doctor;
mod en;
mod env;
pub mod exec;
mod external;
mod fmt;
mod generate;
mod global;
mod hook_env;
mod hook_not_found;
mod implode;
mod install;
mod latest;
mod link;
mod local;
mod ls;
mod ls_remote;
mod outdated;
mod plugins;
mod prune;
mod registry;
#[cfg(debug_assertions)]
mod render_help;
#[cfg(feature = "clap_mangen")]
mod render_mangen;
mod reshim;
mod run;
pub(crate) mod self_update;
mod set;
mod settings;
mod shell;
mod sync;
mod tasks;
mod test_tool;
mod tool;
mod trust;
mod uninstall;
mod unset;
mod upgrade;
mod usage;
mod r#use;
pub mod version;
mod watch;
mod r#where;
mod r#which;

#[derive(clap::ValueEnum, Debug, Clone, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LevelFilter {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(clap::Parser, Debug)]
#[clap(name = "mise", about, long_about = LONG_ABOUT, after_long_help = AFTER_LONG_HELP, author = "Jeff Dickey <@jdx>", arg_required_else_help = true)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Option<Commands>,
    /// Task to run
    #[clap(name = "TASK", long_help = LONG_TASK_ABOUT)]
    pub task: Option<String>,
    /// Task arguments
    #[clap(hide = true)]
    pub task_args: Option<Vec<String>>,
    /// Change directory before running command
    #[clap(short='C', long, global=true, value_name="DIR", value_hint=clap::ValueHint::DirPath)]
    pub cd: Option<PathBuf>,
    /// Dry run, don't actually do anything
    #[clap(short = 'n', long, hide = true)]
    pub dry_run: bool,
    /// Sets log level to debug
    #[clap(long, global = true, hide = true)]
    pub debug: bool,
    /// Set the environment for loading `mise.<ENV>.toml`
    #[clap(short = 'E', long, global = true)]
    pub env: Option<Vec<String>>,
    /// Force the operation
    #[clap(long, short, hide = true)]
    pub force: bool,
    /// Set the log output verbosity
    #[clap(long, short, hide = true, overrides_with = "prefix")]
    pub interleave: bool,
    /// How many jobs to run in parallel [default: 8]
    #[clap(long, short, global = true, env = "MISE_JOBS")]
    pub jobs: Option<usize>,
    #[clap(long, global = true, hide = true, value_name = "LEVEL", value_enum)]
    pub log_level: Option<LevelFilter>,
    #[clap(long, short, hide = true, overrides_with = "interleave")]
    pub prefix: bool,
    /// Set the profile (environment)
    #[clap(short = 'P', long, global = true, hide = true, conflicts_with = "env")]
    pub profile: Option<Vec<String>>,
    #[clap(long, short, hide = true)]
    pub shell: Option<String>,
    /// Tool(s) to run in addition to what is in mise.toml files
    /// e.g.: node@20 python@3.10
    #[clap(short, long, hide = true, value_name = "TOOL@VERSION")]
    pub tool: Vec<ToolArg>,
    /// Suppress non-error messages
    #[clap(short = 'q', long, global = true, overrides_with = "verbose")]
    pub quiet: bool,
    /// Read/write directly to stdin/stdout/stderr instead of by line
    #[clap(long, global = true)]
    pub raw: bool,
    /// Shows elapsed time after each task completes
    ///
    /// Default to always show with `MISE_TASK_TIMINGS=1`
    #[clap(long, alias = "timing", verbatim_doc_comment, hide = true)]
    pub timings: bool,
    /// Hides elapsed time after each task completes
    ///
    /// Default to always hide with `MISE_TASK_TIMINGS=0`
    #[clap(long, alias = "no-timing", hide = true, verbatim_doc_comment)]
    pub no_timings: bool,

    /// Sets log level to trace
    #[clap(long, global = true, hide = true)]
    pub trace: bool,
    /// Show extra output (use -vv for even more)
    #[clap(short='v', long, global=true, overrides_with="quiet", action=ArgAction::Count)]
    pub verbose: u8,
    #[clap(long, short = 'V', hide = true)]
    pub version: bool,
    /// Answer yes to all confirmation prompts
    #[clap(short = 'y', long, global = true)]
    pub yes: bool,
}

#[derive(Debug, Subcommand, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Commands {
    Activate(activate::Activate),
    Alias(alias::Alias),
    Asdf(asdf::Asdf),
    Backends(backends::Backends),
    BinPaths(bin_paths::BinPaths),
    Cache(cache::Cache),
    Completion(completion::Completion),
    Config(config::Config),
    Current(current::Current),
    Deactivate(deactivate::Deactivate),
    Direnv(direnv::Direnv),
    Doctor(doctor::Doctor),
    En(en::En),
    Env(env::Env),
    Exec(exec::Exec),
    Fmt(fmt::Fmt),
    Generate(generate::Generate),
    Global(global::Global),
    HookEnv(hook_env::HookEnv),
    HookNotFound(hook_not_found::HookNotFound),
    Implode(implode::Implode),
    Install(install::Install),
    Latest(latest::Latest),
    Link(link::Link),
    Local(local::Local),
    Ls(ls::Ls),
    LsRemote(ls_remote::LsRemote),
    Outdated(outdated::Outdated),
    Plugins(plugins::Plugins),
    Prune(prune::Prune),
    Registry(registry::Registry),
    Reshim(reshim::Reshim),
    Run(run::Run),
    SelfUpdate(self_update::SelfUpdate),
    Set(set::Set),
    Settings(settings::Settings),
    Shell(shell::Shell),
    Sync(sync::Sync),
    Tasks(tasks::Tasks),
    TestTool(test_tool::TestTool),
    Tool(tool::Tool),
    Trust(trust::Trust),
    Uninstall(uninstall::Uninstall),
    Unset(unset::Unset),
    Upgrade(upgrade::Upgrade),
    Usage(usage::Usage),
    Use(r#use::Use),
    Version(version::Version),
    Watch(Box<watch::Watch>),
    Where(r#where::Where),
    Which(which::Which),

    #[cfg(debug_assertions)]
    RenderHelp(render_help::RenderHelp),

    #[cfg(feature = "clap_mangen")]
    RenderMangen(render_mangen::RenderMangen),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run(),
            Self::Alias(cmd) => cmd.run(),
            Self::Asdf(cmd) => cmd.run(),
            Self::Backends(cmd) => cmd.run(),
            Self::BinPaths(cmd) => cmd.run(),
            Self::Cache(cmd) => cmd.run(),
            Self::Completion(cmd) => cmd.run(),
            Self::Config(cmd) => cmd.run(),
            Self::Current(cmd) => cmd.run(),
            Self::Deactivate(cmd) => cmd.run(),
            Self::Direnv(cmd) => cmd.run(),
            Self::Doctor(cmd) => cmd.run(),
            Self::En(cmd) => cmd.run(),
            Self::Env(cmd) => cmd.run(),
            Self::Exec(cmd) => cmd.run(),
            Self::Fmt(cmd) => cmd.run(),
            Self::Generate(cmd) => cmd.run(),
            Self::Global(cmd) => cmd.run(),
            Self::HookEnv(cmd) => cmd.run(),
            Self::HookNotFound(cmd) => cmd.run(),
            Self::Implode(cmd) => cmd.run(),
            Self::Install(cmd) => cmd.run(),
            Self::Latest(cmd) => cmd.run(),
            Self::Link(cmd) => cmd.run(),
            Self::Local(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run(),
            Self::LsRemote(cmd) => cmd.run(),
            Self::Outdated(cmd) => cmd.run(),
            Self::Plugins(cmd) => cmd.run(),
            Self::Prune(cmd) => cmd.run(),
            Self::Registry(cmd) => cmd.run(),
            Self::Reshim(cmd) => cmd.run(),
            Self::Run(cmd) => cmd.run(),
            Self::SelfUpdate(cmd) => cmd.run(),
            Self::Set(cmd) => cmd.run(),
            Self::Settings(cmd) => cmd.run(),
            Self::Shell(cmd) => cmd.run(),
            Self::Sync(cmd) => cmd.run(),
            Self::Tasks(cmd) => cmd.run(),
            Self::TestTool(cmd) => cmd.run(),
            Self::Tool(cmd) => cmd.run(),
            Self::Trust(cmd) => cmd.run(),
            Self::Uninstall(cmd) => cmd.run(),
            Self::Unset(cmd) => cmd.run(),
            Self::Upgrade(cmd) => cmd.run(),
            Self::Usage(cmd) => cmd.run(),
            Self::Use(cmd) => cmd.run(),
            Self::Version(cmd) => cmd.run(),
            Self::Watch(cmd) => cmd.run(),
            Self::Where(cmd) => cmd.run(),
            Self::Which(cmd) => cmd.run(),

            #[cfg(debug_assertions)]
            Self::RenderHelp(cmd) => cmd.run(),

            #[cfg(feature = "clap_mangen")]
            Self::RenderMangen(cmd) => cmd.run(),
        }
    }
}

impl Cli {
    pub fn run(args: &Vec<String>) -> Result<()> {
        crate::env::ARGS.write().unwrap().clone_from(args);
        measure!("hande_shim", { shims::handle_shim() })?;
        ctrlc::init();
        let print_version = version::print_version_if_requested(args)?;

        let cli = measure!("pre_settings", { Self::pre_settings(args) })?;
        measure!("add_cli_matches", { Settings::add_cli_matches(&cli) });
        measure!("settings", {
            let _ = Settings::try_get();
        });
        measure!("logger", { logger::init() });
        measure!("migrate", { migrate::run() });
        if let Err(err) = crate::cache::auto_prune() {
            warn!("auto_prune failed: {err:?}");
        }

        debug!("ARGS: {}", &args.join(" "));
        if print_version {
            version::show_latest();
            exit(0);
        }
        let cmd = cli.get_command()?;
        measure!("run {cmd}", { cmd.run() })
    }

    fn pre_settings(args: &Vec<String>) -> Result<Cli> {
        let mut results = vec![];
        let mut cli = None;
        rayon::scope(|r| {
            r.spawn(|_| {
                measure!("install_state", {
                    results.push(crate::install_state::init())
                });
            });
            measure!("get_matches_from", {
                cli = Some(Cli::parse_from(args));
            });
        });
        results.into_iter().try_for_each(|r| r)?;
        Ok(cli.unwrap())
    }

    fn get_command(self) -> Result<Commands> {
        if let Some(cmd) = self.command {
            Ok(cmd)
        } else {
            if let Some(task) = self.task {
                if Config::get().tasks()?.contains_key(&task) {
                    return Ok(Commands::Run(run::Run {
                        task,
                        args: self.task_args.unwrap_or_default(),
                        cd: self.cd,
                        dry_run: self.dry_run,
                        failed_tasks: Default::default(),
                        force: self.force,
                        interleave: self.interleave,
                        is_linear: false,
                        jobs: self.jobs,
                        no_timings: self.no_timings,
                        output: run::TaskOutput::Prefix,
                        prefix: self.prefix,
                        shell: self.shell,
                        quiet: self.quiet,
                        raw: self.raw,
                        timings: self.timings,
                        tmpdir: Default::default(),
                        tool: Default::default(),
                    }));
                } else if let Some(cmd) = external::COMMANDS.get(&task) {
                    external::execute(
                        &task.into(),
                        cmd.clone(),
                        self.task_args.unwrap_or_default(),
                    )?;
                    exit(0);
                }
            }
            Cli::command().print_help()?;
            exit(1)
        }
    }
}

const LONG_ABOUT: &str = "
mise is a tool for managing runtime versions. https://github.com/jdx/mise

It's a replacement for tools like nvm, nodenv, rbenv, rvm, chruby, pyenv, etc.
that works for any language. It's also great for managing linters/tools like
jq and shellcheck.

It is inspired by asdf and uses asdf's plugin ecosystem under the hood:
https://asdf-vm.com/";

const LONG_TASK_ABOUT: &str = r#"Task to run.

Shorthand for `mise task run <TASK>`."#;

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise install node@20.0.0</bold>       Install a specific node version
    $ <bold>mise install node@20</bold>           Install a version matching a prefix
    $ <bold>mise install node</bold>              Install the node version defined in config
    $ <bold>mise install</bold>                   Install all plugins/tools defined in config

    $ <bold>mise install cargo:ripgrep            Install something via cargo
    $ <bold>mise install npm:prettier             Install something via npm

    $ <bold>mise use node@20</bold>               Use node-20.x in current project
    $ <bold>mise use -g node@20</bold>            Use node-20.x as default
    $ <bold>mise use node@latest</bold>           Use latest node in current directory

    $ <bold>mise up --interactive</bold>          Show a menu to upgrade tools

    $ <bold>mise x -- npm install</bold>          `npm install` w/ config loaded into PATH
    $ <bold>mise x node@20 -- node app.js</bold>  `node app.js` w/ config + node-20.x on PATH

    $ <bold>mise set NODE_ENV=production</bold>   Set NODE_ENV=production in config

    $ <bold>mise run build</bold>                 Run `build` tasks
    $ <bold>mise watch build</bold>               Run `build` tasks repeatedly when files change

    $ <bold>mise settings</bold>                  Show settings in use
    $ <bold>mise settings color=0</bold>          Disable color by modifying global config file
"#
);
