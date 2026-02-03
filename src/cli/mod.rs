use crate::config::{Config, Settings};
use crate::exit::exit;
use crate::task::TaskOutput;
use crate::ui::{self, ctrlc};
use crate::{Result, backend};
use crate::{cli::args::ToolArg, path::PathExt};
use crate::{hook_env as hook_env_module, logger, migrate, shims};
use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use eyre::bail;
use std::path::PathBuf;

mod activate;
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
mod tool_alias;

pub use hook_env::HookReason;
pub(crate) mod edit;
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
mod prepare;
mod prune;
mod registry;
#[cfg(debug_assertions)]
mod render_help;
mod reshim;
pub mod run;
mod search;
#[cfg_attr(not(feature = "self_update"), path = "self_update_stub.rs")]
pub mod self_update;
mod set;
mod settings;
mod shell;
mod shell_alias;
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
    /// Continue running tasks even if one fails
    #[clap(long, short = 'c', hide = true, verbatim_doc_comment)]
    pub continue_on_error: bool,
    /// Change directory before running command
    #[clap(short='C', long, global=true, value_name="DIR", value_hint=clap::ValueHint::DirPath)]
    pub cd: Option<PathBuf>,
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
    /// Dry run, don't actually do anything
    #[clap(short = 'n', long, hide = true)]
    pub dry_run: bool,
    #[clap(long, short, hide = true, overrides_with = "interleave")]
    pub prefix: bool,
    /// Set the profile (environment)
    #[clap(short = 'P', long, global = true, hide = true, conflicts_with = "env")]
    pub profile: Option<Vec<String>>,
    /// Suppress non-error messages
    #[clap(short = 'q', long, global = true, overrides_with_all = &["silent", "trace", "verbose", "debug", "log_level"])]
    pub quiet: bool,
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
    /// Show extra output (use -vv for even more)
    #[clap(short='v', long, global=true, action=ArgAction::Count, overrides_with_all = &["quiet", "silent", "trace", "debug"])]
    pub verbose: u8,
    #[clap(long, short = 'V', hide = true)]
    pub version: bool,
    /// Answer yes to all confirmation prompts
    #[clap(short = 'y', long, global = true)]
    pub yes: bool,
    /// Sets log level to debug
    #[clap(long, global = true, hide = true, overrides_with_all = &["quiet", "trace", "verbose", "silent", "log_level"])]
    pub debug: bool,
    #[clap(long, global = true, hide = true, value_name = "LEVEL", value_enum, overrides_with_all = &["quiet", "trace", "verbose", "silent", "debug"])]
    pub log_level: Option<LevelFilter>,
    /// Do not load any config files
    ///
    /// Can also use `MISE_NO_CONFIG=1`
    #[clap(long)]
    pub no_config: bool,
    /// Do not load environment variables from config files
    ///
    /// Can also use `MISE_NO_ENV=1`
    #[clap(long)]
    pub no_env: bool,
    /// Do not execute hooks from config files
    ///
    /// Can also use `MISE_NO_HOOKS=1`
    #[clap(long)]
    pub no_hooks: bool,
    /// Hides elapsed time after each task completes
    ///
    /// Default to always hide with `MISE_TASK_TIMINGS=0`
    #[clap(long, alias = "no-timing", hide = true, verbatim_doc_comment)]
    pub no_timings: bool,
    #[clap(long)]
    pub output: Option<TaskOutput>,
    /// Read/write directly to stdin/stdout/stderr instead of by line
    #[clap(long, global = true)]
    pub raw: bool,
    /// Require lockfile URLs to be present during installation
    ///
    /// Fails if tools don't have pre-resolved URLs in the lockfile for the current platform.
    /// This prevents API calls to GitHub, aqua registry, etc.
    /// Can also be enabled via MISE_LOCKED=1 or settings.locked=true
    #[clap(long, global = true, verbatim_doc_comment)]
    pub locked: bool,
    /// Suppress all task output and mise non-error messages
    #[clap(long, global = true, overrides_with_all = &["quiet", "trace", "verbose", "debug", "log_level"])]
    pub silent: bool,
    /// Shows elapsed time after each task completes
    ///
    /// Default to always show with `MISE_TASK_TIMINGS=1`
    #[clap(long, alias = "timing", verbatim_doc_comment, hide = true)]
    pub timings: bool,
    /// Sets log level to trace
    #[clap(long, global = true, hide = true, overrides_with_all = &["quiet", "silent", "verbose", "debug", "log_level"])]
    pub trace: bool,
}

#[derive(Subcommand, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Commands {
    Activate(activate::Activate),
    ToolAlias(Box<tool_alias::ToolAlias>),
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
    Edit(edit::Edit),
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
    Prepare(prepare::Prepare),
    Prune(prune::Prune),
    Registry(registry::Registry),
    #[cfg(debug_assertions)]
    RenderHelp(render_help::RenderHelp),
    Reshim(reshim::Reshim),
    Run(Box<run::Run>),
    Search(search::Search),
    #[cfg(feature = "self_update")]
    SelfUpdate(self_update::SelfUpdate),
    Set(set::Set),
    Settings(settings::Settings),
    Shell(shell::Shell),
    ShellAlias(shell_alias::ShellAlias),
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
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run(),
            Self::ToolAlias(cmd) => cmd.run().await,
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
            Self::Edit(cmd) => cmd.run().await,
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
            Self::Prepare(cmd) => cmd.run().await,
            Self::Prune(cmd) => cmd.run().await,
            Self::Registry(cmd) => cmd.run().await,
            #[cfg(debug_assertions)]
            Self::RenderHelp(cmd) => cmd.run(),
            Self::Reshim(cmd) => cmd.run().await,
            Self::Run(cmd) => (*cmd).run().await,
            Self::Search(cmd) => cmd.run().await,
            #[cfg(feature = "self_update")]
            Self::SelfUpdate(cmd) => cmd.run().await,
            Self::Set(cmd) => cmd.run().await,
            Self::Settings(cmd) => cmd.run().await,
            Self::Shell(cmd) => cmd.run().await,
            Self::ShellAlias(cmd) => cmd.run().await,
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

/// Get all flags (with values and boolean) from both global Cli and Run subcommand
fn get_all_run_flags(cmd: &clap::Command) -> (Vec<String>, Vec<String>) {
    // Get global flags from Cli
    let (mut flags_with_values, mut boolean_flags) = get_global_flags(cmd);

    // Get run-specific flags from Run subcommand
    if let Some(run_cmd) = cmd.get_subcommands().find(|s| s.get_name() == "run") {
        let (run_vals, run_bools) = get_global_flags(run_cmd);
        flags_with_values.extend(run_vals);
        boolean_flags.extend(run_bools);
    }

    (flags_with_values, boolean_flags)
}

/// Prefix used to escape flags that should be passed to tasks, not mise
const TASK_ARG_ESCAPE_PREFIX: &str = "\x00MISE_TASK_ARG\x00";

/// Escape flags after task names so clap doesn't parse them as mise flags.
/// This preserves ::: separators for multi-task handling while preventing
/// clap from consuming flags like --jobs that appear after task names.
fn escape_task_args(cmd: &clap::Command, args: &[String]) -> Vec<String> {
    // If there's already a '--' separator, let clap handle everything normally
    if args.contains(&"--".to_string()) {
        return args.to_vec();
    }

    // Find "run" position
    let run_pos = args.iter().position(|a| a == "run");
    let run_pos = match run_pos {
        Some(pos) => pos,
        None => return args.to_vec(), // Not a run command
    };

    let (flags_with_values, _) = get_all_run_flags(cmd);

    // Build result, escaping flags that appear after task names
    let mut result = args[..=run_pos].to_vec(); // Include up to and including "run"
    let mut in_task_args = false; // true after we've seen a task name

    let mut i = run_pos + 1;
    while i < args.len() {
        let arg = &args[i];

        // ::: starts a new task, so reset to looking for task name
        if arg == ":::" {
            result.push(arg.clone());
            in_task_args = false;
            i += 1;
            continue;
        }

        if !in_task_args {
            // Looking for task name - skip any mise flags
            if arg.starts_with('-') {
                // It's a flag - keep it as-is for mise to parse
                result.push(arg.clone());

                // Check if this flag takes a value (and needs to consume the next arg)
                let flag_takes_value = if arg.starts_with("--") {
                    if arg.contains('=') {
                        false // --flag=value, no separate value
                    } else {
                        flags_with_values.iter().any(|f| f == arg)
                    }
                } else if arg.len() > 2 {
                    // Short flag with embedded value (e.g., -j4), no separate value needed
                    false
                } else if arg.len() == 2 {
                    let flag_name = &arg[..2];
                    flags_with_values.iter().any(|f| f == flag_name)
                } else {
                    false
                };

                if flag_takes_value && i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    i += 1;
                    result.push(args[i].clone());
                }
            } else {
                // Found task name
                result.push(arg.clone());
                in_task_args = true;
            }
        } else {
            // In task args - escape flags so clap doesn't parse them
            if arg.starts_with('-') && arg != "-" {
                // Escape the flag
                result.push(format!("{}{}", TASK_ARG_ESCAPE_PREFIX, arg));
            } else {
                result.push(arg.clone());
            }
        }

        i += 1;
    }

    result
}

/// Unescape task args that were escaped by escape_task_args
pub fn unescape_task_args(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|arg| {
            if let Some(stripped) = arg.strip_prefix(TASK_ARG_ESCAPE_PREFIX) {
                stripped.to_string()
            } else {
                arg.clone()
            }
        })
        .collect()
}

fn preprocess_args_for_naked_run(cmd: &clap::Command, args: &[String]) -> Vec<String> {
    // Check if this might be a naked run (no subcommand)
    if args.len() < 2 {
        return args.to_vec();
    }

    // If there's already a '--' separator, let clap handle everything normally
    // (user explicitly separated task args)
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
    let known_subcommands: Vec<_> = cmd
        .get_subcommands()
        .flat_map(|s| std::iter::once(s.get_name()).chain(s.get_all_aliases()))
        .collect();

    // Check if the first non-flag argument is a known subcommand
    if known_subcommands.contains(&args[i].as_str()) {
        return args.to_vec();
    }

    // Special case: "help" should print help, not be treated as a task
    if args[i] == "help" || args[i] == "-h" || args[i] == "--help" {
        return args.to_vec();
    }

    // This is a naked run - inject "run" subcommand so clap routes it correctly
    // Format: ["mise", "-q", "task", "arg1"] becomes ["mise", "-q", "run", "task", "arg1"]
    // This preserves global flags while making it an explicit run command
    let mut result = args[..i].to_vec(); // Keep program name + global flags
    result.push("run".to_string()); // Insert "run" subcommand
    result.extend_from_slice(&args[i..]); // Add task name and args
    result
}

impl Cli {
    pub async fn run(args: &Vec<String>) -> Result<()> {
        crate::env::ARGS.write().unwrap().clone_from(args);
        // Load .miserc.toml early, before MISE_ENV and other early settings are accessed.
        // This allows setting MISE_ENV in a config file instead of only via env vars.
        if let Err(err) = crate::config::miserc::init() {
            warn!("Failed to load .miserc.toml: {err}");
        }
        if *crate::env::MISE_TOOL_STUB && args.len() >= 2 {
            tool_stub::short_circuit_stub(&args[2..]).await?;
        }
        // Fast-path for hook-env: exit early if nothing has changed
        // This avoids expensive backend::load_tools() and config loading
        if hook_env_module::should_exit_early_fast() {
            return Ok(());
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
        // Escape flags after task names so they go to tasks, not mise
        let processed_args = escape_task_args(&cmd, &processed_args);

        let cli = measure!("get_matches_from", {
            Cli::parse_from(processed_args.iter())
        });
        // Validate --cd path BEFORE Settings processes it and changes the directory
        validate_cd_path(&cli.cd)?;
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
                // Handle special case: "help", "-h", or "--help" as task should print help
                if task == "help" || task == "-h" || task == "--help" {
                    Cli::command().print_help()?;
                    exit(0);
                }

                let config = Config::get().await?;

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
                    return Ok(Commands::Run(Box::new(run::Run {
                        task,
                        args: self.task_args.unwrap_or_default(),
                        args_last: self.task_args_last,
                        cd: self.cd,
                        continue_on_error: self.continue_on_error,
                        dry_run: self.dry_run,
                        force: self.force,
                        interleave: self.interleave,
                        is_linear: false,
                        jobs: self.jobs,
                        no_timings: self.no_timings,
                        output: self.output,
                        prefix: self.prefix,
                        shell: self.shell,
                        quiet: self.quiet,
                        silent: self.silent,
                        raw: self.raw,
                        timings: self.timings,
                        tmpdir: Default::default(),
                        tool: Default::default(),
                        output_handler: None,
                        context_builder: Default::default(),
                        executor: None,
                        no_cache: Default::default(),
                        timeout: None,
                        skip_deps: false,
                        no_prepare: false,
                        fresh_env: false,
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

Shorthand for `mise tasks run <TASK>`."#;

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

/// Validate the --cd path if provided and return an error if it doesn't exist
fn validate_cd_path(cd: &Option<PathBuf>) -> Result<()> {
    if let Some(path) = cd {
        if !path.exists() {
            bail!(
                "Directory specified with --cd does not exist: {}\n\
                 Please check the path and try again.",
                ui::style::epath(path)
            );
        }
        if !path.is_dir() {
            bail!(
                "Path specified with --cd is not a directory: {}\n\
                 Please provide a valid directory path.",
                ui::style::epath(path)
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subcommands_are_sorted() {
        let cmd = Cli::command();
        // Check all subcommands except watch (which has many watchexec passthrough args)
        for subcmd in cmd.get_subcommands() {
            if subcmd.get_name() != "watch" {
                clap_sort::assert_sorted(subcmd);
            }
        }
    }
}
