use crate::cli::run::TaskOutput;
use crate::config::{Config, Settings};
use crate::exit::exit;
use crate::ui::ctrlc;
use crate::{Result, backend};
use crate::{cli::args::ToolArg, path::PathExt};
use crate::{logger, migrate, shims};
use clap::{ArgAction, CommandFactory, Parser, Subcommand};
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
mod install_into;
mod latest;
mod link;
mod local;
mod lock;
mod ls;
mod ls_remote;
mod mcp;
mod outdated;
mod plugins;
mod prune;
mod registry;
#[cfg(debug_assertions)]
mod render_help;
#[cfg(feature = "clap_mangen")]
mod render_mangen;
mod reshim;
pub mod run;
mod search;
#[cfg_attr(not(feature = "self_update"), path = "self_update_stub.rs")]
pub mod self_update;
mod set;
mod settings;
mod shell;
mod sync;
mod tasks;
mod test_tool;
mod tool;
pub mod tool_stub;
mod trust;
mod uninstall;
mod unset;
mod unuse;
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

#[derive(clap::Parser)]
#[clap(name = "mise", about, long_about = LONG_ABOUT, after_long_help = AFTER_LONG_HELP, author = "Jeff Dickey <@jdx>", arg_required_else_help = true)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Option<Commands>,
    /// Task to run
    #[clap(name = "TASK", long_help = LONG_TASK_ABOUT)]
    pub task: Option<String>,
    /// Task arguments
    #[clap(allow_hyphen_values = true, hide = true)]
    pub task_args: Option<Vec<String>>,
    #[clap(last = true, hide = true)]
    pub task_args_last: Vec<String>,
    /// Change directory before running command
    #[clap(short='C', long, global=true, value_name="DIR", value_hint=clap::ValueHint::DirPath)]
    pub cd: Option<PathBuf>,
    /// Continue running tasks even if one fails
    #[clap(long, short = 'c', hide = true, verbatim_doc_comment)]
    pub continue_on_error: bool,
    /// Dry run, don't actually do anything
    #[clap(short = 'n', long, hide = true)]
    pub dry_run: bool,
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
    #[clap(long, short, hide = true, overrides_with = "interleave")]
    pub prefix: bool,
    #[clap(long)]
    pub output: Option<TaskOutput>,
    /// Set the profile (environment)
    #[clap(short = 'P', long, global = true, hide = true, conflicts_with = "env")]
    pub profile: Option<Vec<String>>,
    #[clap(long, short, hide = true)]
    pub shell: Option<String>,
    /// Tool(s) to run in addition to what is in mise.toml files
    /// e.g.: node@20 python@3.10
    #[clap(
        short,
        long,
        hide = true,
        value_name = "TOOL@VERSION",
        env = "MISE_QUIET"
    )]
    pub tool: Vec<ToolArg>,
    /// Read/write directly to stdin/stdout/stderr instead of by line
    #[clap(long, global = true)]
    pub raw: bool,
    /// Shows elapsed time after each task completes
    ///
    /// Default to always show with `MISE_TASK_TIMINGS=1`
    #[clap(long, alias = "timing", verbatim_doc_comment, hide = true)]
    pub timings: bool,
    /// Do not load any config files
    ///
    /// Can also use `MISE_NO_CONFIG=1`
    #[clap(long)]
    pub no_config: bool,
    /// Hides elapsed time after each task completes
    ///
    /// Default to always hide with `MISE_TASK_TIMINGS=0`
    #[clap(long, alias = "no-timing", hide = true, verbatim_doc_comment)]
    pub no_timings: bool,

    #[clap(long, short = 'V', hide = true)]
    pub version: bool,
    /// Answer yes to all confirmation prompts
    #[clap(short = 'y', long, global = true)]
    pub yes: bool,

    #[clap(flatten)]
    pub global_output_flags: CliGlobalOutputFlags,
}

#[derive(clap::Args)]
#[group(multiple = false)]
pub struct CliGlobalOutputFlags {
    /// Sets log level to debug
    #[clap(long, global = true, hide = true, overrides_with_all = &["quiet", "trace", "verbose", "silent", "log_level"])]
    pub debug: bool,
    #[clap(long, global = true, hide = true, value_name = "LEVEL", value_enum, overrides_with_all = &["quiet", "trace", "verbose", "silent", "debug"])]
    pub log_level: Option<LevelFilter>,
    /// Suppress non-error messages
    #[clap(short = 'q', long, global = true, overrides_with_all = &["silent", "trace", "verbose", "debug", "log_level"])]
    pub quiet: bool,
    /// Suppress all task output and mise non-error messages
    #[clap(long, global = true, overrides_with_all = &["quiet", "trace", "verbose", "debug", "log_level"])]
    pub silent: bool,
    /// Sets log level to trace
    #[clap(long, global = true, hide = true, overrides_with_all = &["quiet", "silent", "verbose", "debug", "log_level"])]
    pub trace: bool,
    /// Show extra output (use -vv for even more)
    #[clap(short='v', long, global=true, action=ArgAction::Count, overrides_with_all = &["quiet", "silent", "trace", "debug"])]
    pub verbose: u8,
}

#[derive(Subcommand, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Commands {
    Activate(activate::Activate),
    Alias(Box<alias::Alias>),
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
    InstallInto(install_into::InstallInto),
    Latest(latest::Latest),
    Link(link::Link),
    Local(local::Local),
    Lock(lock::Lock),
    Ls(ls::Ls),
    LsRemote(ls_remote::LsRemote),
    Mcp(mcp::Mcp),
    Outdated(outdated::Outdated),
    Plugins(plugins::Plugins),
    Prune(prune::Prune),
    Registry(registry::Registry),
    Reshim(reshim::Reshim),
    Run(Box<run::Run>),
    Search(search::Search),
    #[cfg(feature = "self_update")]
    SelfUpdate(self_update::SelfUpdate),
    Set(set::Set),
    Settings(settings::Settings),
    Shell(shell::Shell),
    Sync(sync::Sync),
    Tasks(tasks::Tasks),
    TestTool(test_tool::TestTool),
    Tool(tool::Tool),
    ToolStub(tool_stub::ToolStub),
    Trust(trust::Trust),
    Uninstall(uninstall::Uninstall),
    Unset(unset::Unset),
    Unuse(unuse::Unuse),
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
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run(),
            Self::Alias(cmd) => cmd.run().await,
            Self::Asdf(cmd) => cmd.run().await,
            Self::Backends(cmd) => cmd.run().await,
            Self::BinPaths(cmd) => cmd.run().await,
            Self::Cache(cmd) => cmd.run(),
            Self::Completion(cmd) => cmd.run().await,
            Self::Config(cmd) => cmd.run().await,
            Self::Current(cmd) => cmd.run().await,
            Self::Deactivate(cmd) => cmd.run(),
            Self::Direnv(cmd) => cmd.run().await,
            Self::Doctor(cmd) => cmd.run().await,
            Self::En(cmd) => cmd.run().await,
            Self::Env(cmd) => cmd.run().await,
            Self::Exec(cmd) => cmd.run().await,
            Self::Fmt(cmd) => cmd.run(),
            Self::Generate(cmd) => cmd.run().await,
            Self::Global(cmd) => cmd.run().await,
            Self::HookEnv(cmd) => cmd.run().await,
            Self::HookNotFound(cmd) => cmd.run().await,
            Self::Implode(cmd) => cmd.run(),
            Self::Install(cmd) => cmd.run().await,
            Self::InstallInto(cmd) => cmd.run().await,
            Self::Latest(cmd) => cmd.run().await,
            Self::Link(cmd) => cmd.run().await,
            Self::Local(cmd) => cmd.run().await,
            Self::Lock(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run().await,
            Self::LsRemote(cmd) => cmd.run().await,
            Self::Mcp(cmd) => cmd.run().await,
            Self::Outdated(cmd) => cmd.run().await,
            Self::Plugins(cmd) => cmd.run().await,
            Self::Prune(cmd) => cmd.run().await,
            Self::Registry(cmd) => cmd.run().await,
            Self::Reshim(cmd) => cmd.run().await,
            Self::Run(cmd) => (*cmd).run().await,
            Self::Search(cmd) => cmd.run().await,
            #[cfg(feature = "self_update")]
            Self::SelfUpdate(cmd) => cmd.run().await,
            Self::Set(cmd) => cmd.run().await,
            Self::Settings(cmd) => cmd.run().await,
            Self::Shell(cmd) => cmd.run().await,
            Self::Sync(cmd) => cmd.run().await,
            Self::Tasks(cmd) => cmd.run().await,
            Self::TestTool(cmd) => cmd.run().await,
            Self::Tool(cmd) => cmd.run().await,
            Self::ToolStub(cmd) => cmd.run().await,
            Self::Trust(cmd) => cmd.run().await,
            Self::Uninstall(cmd) => cmd.run().await,
            Self::Unset(cmd) => cmd.run().await,
            Self::Unuse(cmd) => cmd.run().await,
            Self::Upgrade(cmd) => cmd.run().await,
            Self::Usage(cmd) => cmd.run(),
            Self::Use(cmd) => cmd.run().await,
            Self::Version(cmd) => cmd.run().await,
            Self::Watch(cmd) => cmd.run().await,
            Self::Where(cmd) => cmd.run().await,
            Self::Which(cmd) => cmd.run().await,

            #[cfg(debug_assertions)]
            Self::RenderHelp(cmd) => cmd.run(),

            #[cfg(feature = "clap_mangen")]
            Self::RenderMangen(cmd) => cmd.run(),
        }
    }
}

fn get_global_flags(cmd: &clap::Command) -> (Vec<String>, Vec<String>) {
    let mut flags_with_values = Vec::new();
    let mut boolean_flags = Vec::new();

    for arg in cmd.get_arguments() {
        let takes_value = matches!(
            arg.get_action(),
            clap::ArgAction::Set | clap::ArgAction::Append
        );
        let is_bool = matches!(
            arg.get_action(),
            clap::ArgAction::SetTrue | clap::ArgAction::SetFalse
        );

        if takes_value {
            if let Some(long) = arg.get_long() {
                flags_with_values.push(format!("--{}", long));
            }
            if let Some(short) = arg.get_short() {
                flags_with_values.push(format!("-{}", short));
            }
        } else if is_bool {
            if let Some(long) = arg.get_long() {
                boolean_flags.push(format!("--{}", long));
            }
            if let Some(short) = arg.get_short() {
                boolean_flags.push(format!("-{}", short));
            }
        }
    }

    (flags_with_values, boolean_flags)
}

fn preprocess_args_for_naked_run(cmd: &clap::Command, args: &[String]) -> Vec<String> {
    // Check if this might be a naked run (no subcommand)
    if args.len() < 2 {
        return args.to_vec();
    }

    // If there's a '--' separator, let clap handle everything normally
    // The '--' tells clap where mise args end and task args begin
    if args.contains(&"--".to_string()) {
        return args.to_vec();
    }

    let (flags_with_values, _) = get_global_flags(cmd);

    // Skip global flags to find the first non-flag argument (subcommand or task)
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if !arg.starts_with('-') {
            // Found first non-flag argument
            break;
        }

        // Check if this flag takes a value
        let flag_takes_value = if arg.starts_with("--") {
            if arg.contains('=') {
                // --flag=value format, doesn't consume next arg
                i += 1;
                continue;
            } else {
                let flag_name = arg.split('=').next().unwrap();
                flags_with_values.iter().any(|f| f == flag_name)
            }
        } else {
            // Short form: check if it's in flags_with_values list
            if arg.len() >= 2 {
                let flag_name = &arg[..2]; // Get -X part
                flags_with_values.iter().any(|f| f == flag_name)
            } else {
                false
            }
        };

        if flag_takes_value && i + 1 < args.len() {
            // Skip both the flag and its value
            i += 2;
        } else {
            // Skip just the flag
            i += 1;
        }
    }

    // No non-flag argument found
    if i >= args.len() {
        return args.to_vec();
    }

    // Extract all known subcommand names and aliases from the clap Command
    let mut known_subcommands = Vec::new();
    for subcmd in cmd.get_subcommands() {
        known_subcommands.push(subcmd.get_name());
        known_subcommands.extend(subcmd.get_all_aliases());
    }

    // Check if the first non-flag argument is a known subcommand
    if known_subcommands.contains(&args[i].as_str()) {
        return args.to_vec();
    }

    // This is a naked run - the task name is at position i
    // Truncate everything after the task name
    args[..=i].to_vec()
}

impl Cli {
    pub async fn run(args: &Vec<String>) -> Result<()> {
        crate::env::ARGS.write().unwrap().clone_from(args);
        if *crate::env::MISE_TOOL_STUB && args.len() >= 2 {
            tool_stub::short_circuit_stub(&args[2..]).await?;
        }
        measure!("logger", { logger::init() });
        check_working_directory();
        measure!("handle_shim", { shims::handle_shim().await })?;
        ctrlc::init();
        let print_version = version::print_version_if_requested(args)?;
        let _ = measure!("backend::load_tools", { backend::load_tools().await });

        // Pre-process args to handle naked runs before clap parsing
        let cmd = Cli::command();
        let processed_args = preprocess_args_for_naked_run(&cmd, args);

        let cli = measure!("get_matches_from", {
            Cli::parse_from(processed_args.iter())
        });
        measure!("add_cli_matches", { Settings::add_cli_matches(&cli) });
        let _ = measure!("settings", { Settings::try_get() });
        measure!("logger", { logger::init() });
        measure!("migrate", { migrate::run().await });
        if let Err(err) = crate::cache::auto_prune() {
            warn!("auto_prune failed: {err:?}");
        }

        debug!("ARGS: {}", &args.join(" "));
        trace!("MISE_BIN: {}", crate::env::MISE_BIN.display_user());
        if print_version {
            version::show_latest().await;
            exit(0);
        }
        let cmd = cli.get_command().await?;
        measure!("run {cmd}", { cmd.run().await })
    }

    async fn get_command(self) -> Result<Commands> {
        if let Some(cmd) = self.command {
            Ok(cmd)
        } else {
            if let Some(task) = self.task {
                let config = Config::get().await?;

                // Store the original task name before expansion for arg extraction
                let original_task_name = task.clone();

                // Expand :task pattern to match tasks in current directory's config root
                let task = crate::task::expand_colon_task_syntax(&task, &config)?;

                // For monorepo task patterns (starting with //), we need to load
                // tasks from the entire monorepo, not just the current hierarchy
                let tasks = if task.starts_with("//") {
                    let ctx = crate::task::TaskLoadContext::from_pattern(&task);
                    config.tasks_with_context(Some(&ctx)).await?
                } else {
                    config.tasks().await?
                };
                if tasks.iter().any(|(_, t)| t.is_match(&task)) {
                    // For naked runs (mise mytask instead of mise run mytask),
                    // extract arguments from the original command line to avoid
                    // global flags consuming task-specific flags.
                    // Use the original (unexpanded) task name to find its position.
                    let args = crate::env::ARGS.read().unwrap();
                    let task_args = if let Some(task_idx) =
                        args.iter().position(|a| a == &original_task_name)
                    {
                        // Check if there's a '--' separator after the task name
                        let after_task = &args[task_idx + 1..];
                        if let Some(sep_idx) = after_task.iter().position(|a| a == "--") {
                            // Task args start after the '--' separator
                            after_task[sep_idx + 1..].to_vec()
                        } else {
                            // No separator - naked run. If there are positional args,
                            // skip global output flags before them. If no positional args,
                            // pass all flags to the task (they might be task flags).
                            let has_positional = after_task.iter().any(|a| !a.starts_with('-'));

                            if !has_positional {
                                // No positional args - pass everything to task
                                after_task.to_vec()
                            } else {
                                // Has positional args - skip global output flags before first one
                                let global_output_flags = ["-q",
                                    "--quiet",
                                    "-S",
                                    "--silent",
                                    "-v",
                                    "-vv",
                                    "-vvv",
                                    "--verbose",
                                    "--debug",
                                    "--trace",
                                    "--log-level"];

                                let mut task_args = Vec::new();
                                let mut seen_positional = false;
                                let mut i = 0;

                                while i < after_task.len() {
                                    let arg = &after_task[i];

                                    if !seen_positional && arg.starts_with('-') {
                                        // Before first positional - check if global output flag
                                        let is_global_output =
                                            global_output_flags.iter().any(|f| {
                                                arg == f || arg.starts_with(&format!("{}=", f))
                                            });

                                        if is_global_output {
                                            i += 1; // Skip flag
                                            // Skip value for --log-level
                                            if arg == "--log-level"
                                                && i < after_task.len()
                                                && !after_task[i].starts_with('-')
                                            {
                                                i += 1;
                                            }
                                        } else {
                                            // Not a global output flag - it's a task flag
                                            task_args.push(arg.clone());
                                            i += 1;
                                        }
                                    } else {
                                        // Positional or after first positional - include everything
                                        if !arg.starts_with('-') {
                                            seen_positional = true;
                                        }
                                        task_args.push(arg.clone());
                                        i += 1;
                                    }
                                }
                                task_args
                            }
                        }
                    } else {
                        // Fallback to what clap parsed
                        self.task_args
                            .unwrap_or_default()
                            .into_iter()
                            .chain(self.task_args_last)
                            .collect()
                    };

                    return Ok(Commands::Run(Box::new(run::Run {
                        task,
                        args: task_args,
                        args_last: vec![],
                        cd: self.cd,
                        continue_on_error: self.continue_on_error,
                        dry_run: self.dry_run,
                        failed_tasks: Default::default(),
                        force: self.force,
                        interleave: self.interleave,
                        is_linear: false,
                        jobs: self.jobs,
                        no_timings: self.no_timings,
                        output: self.output,
                        prefix: self.prefix,
                        shell: self.shell,
                        quiet: self.global_output_flags.quiet,
                        silent: self.global_output_flags.silent,
                        raw: self.raw,
                        timings: self.timings,
                        tmpdir: Default::default(),
                        tool: Default::default(),
                        keep_order_output: Default::default(),
                        task_prs: Default::default(),
                        timed_outputs: Default::default(),
                        toolset_cache: Default::default(),
                        tool_request_set_cache: Default::default(),
                        env_resolution_cache: Default::default(),
                        no_cache: Default::default(),
                        timeout: None,
                    })));
                } else if let Some(cmd) = external::COMMANDS.get(&task) {
                    external::execute(
                        &task.into(),
                        cmd.clone(),
                        self.task_args
                            .unwrap_or_default()
                            .into_iter()
                            .chain(self.task_args_last)
                            .collect(),
                    )?;
                    exit(0);
                }
            }
            Cli::command().print_help()?;
            exit(1)
        }
    }
}

const LONG_ABOUT: &str =
    "mise manages dev tools, env vars, and runs tasks. https://github.com/jdx/mise";

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

/// Check if the current working directory exists and warn if not
fn check_working_directory() {
    if std::env::current_dir().is_err() {
        // Try to get the directory path from PWD env var, which might still contain the old path
        let dir_path = std::env::var("PWD")
            .or_else(|_| std::env::var("OLDPWD"))
            .unwrap_or_else(|_| "(unknown)".to_string());
        warn!(
            "Current directory does not exist or is not accessible: {}",
            dir_path
        );
    }
}
