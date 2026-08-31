use crate::config::{Config, Settings, config_file};
use crate::task::TaskOutput;
use crate::ui::{self, ctrlc};
use crate::{Result, backend, request_exit};
use crate::{cli::args::ToolArg, path::PathExt};
use crate::{hook_env as hook_env_module, logger, migrate, shims};
use eyre::{Report, bail};
use std::path::PathBuf;

mod activate;
pub(crate) mod args;
mod asdf;
pub(crate) mod backends;
mod bin_paths;
mod bootstrap;
mod cache;
mod completion;
mod config;
mod current;
mod deactivate;
mod direnv;
mod doctor;
mod dotfiles;
mod en;
mod env;
pub(crate) mod exec;
mod external;
mod fmt;
mod generate;
mod github;
mod global;
mod hook_env;
mod hook_not_found;
mod tool_alias;

pub(crate) use hook_env::HookReason;
mod command_effects;
mod deps;
pub(crate) mod edit;
mod editor;
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
mod oci;
mod outdated;
mod patrons;
mod plugins;
pub(crate) mod prune;
mod registry;
#[cfg(debug_assertions)]
mod render_help;
mod reshim;
pub(crate) mod run;
mod search;
#[cfg_attr(not(feature = "self_update"), path = "self_update_stub.rs")]
pub(crate) mod self_update;
mod set;
mod settings;
mod shell;
mod shell_alias;
mod sponsors;
mod sync;
pub(crate) mod system;
mod tasks;
mod test_tool;
mod token;
mod tool;
pub(crate) mod tool_stub;
mod trust;
mod uninstall;
mod unset;
mod untrust;
mod unuse;
mod upgrade;
mod usage;
mod r#use;
pub(crate) mod version;
mod watch;
mod r#where;
mod r#which;

#[derive(usage_rs::ValueEnum, Debug, Clone, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum LevelFilter {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(usage_rs::Cli)]
#[usage(name = "mise", about, long_about = LONG_ABOUT, after_long_help = AFTER_LONG_HELP, author = "Jeff Dickey <@jdx>", arg_required_else_help = true, completion = true, unknown_flags = "error")]
pub(crate) struct Cli {
    #[usage(subcommand)]
    pub command: Option<Commands>,
    /// Task to run
    #[usage(
        name = "TASK",
        double_dash = "automatic",
        long_help = r#"Task to run.

Shorthand for `mise tasks run <TASK>`."#
    )]
    pub task: Option<String>,
    /// Task arguments
    #[usage(hide = true)]
    pub task_args: Option<Vec<String>>,
    #[usage(double_dash = "required", hide = true)]
    pub task_args_last: Vec<String>,
    /// Continue running tasks even if one fails
    #[usage(long, short = 'c', hide = true, verbatim_doc_comment)]
    pub continue_on_error: bool,
    /// Change directory before running command
    #[usage(short='C', long, global=true, value_name="DIR", value_hint=usage_rs::ValueHint::DirPath)]
    pub cd: Option<PathBuf>,
    /// Set the environment for loading `mise.<ENV>.toml`
    #[usage(short = 'E', long, global = true)]
    pub env: Option<Vec<String>>,
    /// Force the operation
    #[usage(long, short, hide = true)]
    pub force: bool,
    /// How many jobs to run in parallel; values below 1 are treated as 1 [default: 8]
    #[usage(long, short, global = true, env = "MISE_JOBS")]
    pub jobs: Option<usize>,
    /// Dry run, don't actually do anything
    #[usage(short = 'n', long, hide = true)]
    pub dry_run: bool,
    /// Set the profile (environment)
    #[usage(short = 'P', long, global = true, hide = true, conflicts = "env")]
    pub profile: Option<Vec<String>>,
    /// Suppress non-error messages
    #[usage(short = 'q', long, global = true, env = "MISE_QUIET", overrides = &["silent", "trace", "verbose", "debug", "log_level"])]
    pub quiet: bool,
    #[usage(long, short, hide = true)]
    pub shell: Option<String>,
    /// Tool(s) to run in addition to what is in mise.toml files
    /// e.g.: node@20 python@3.10
    #[usage(short, long, hide = true, value_name = "TOOL@VERSION")]
    pub tool: Vec<ToolArg>,
    /// Show extra output (use -vv for even more)
    #[usage(short='v', long, global=true, count, overrides = &["quiet", "silent", "trace", "debug"])]
    pub verbose: u8,
    #[usage(long, short = 'V', hide = true)]
    pub version: bool,
    /// Answer yes to all confirmation prompts
    #[usage(short = 'y', long, global = true)]
    pub yes: bool,
    /// Sets log level to debug
    #[usage(long, global = true, hide = true, overrides = &["quiet", "trace", "verbose", "silent", "log_level"])]
    pub debug: bool,
    #[usage(long, global = true, hide = true, value_name = "LEVEL", value_enum, overrides = &["quiet", "trace", "verbose", "silent", "debug"])]
    pub log_level: Option<LevelFilter>,
    /// Do not load any config files
    ///
    /// Can also use `MISE_NO_CONFIG=1`
    #[usage(long)]
    pub no_config: bool,
    /// Do not load environment variables from config files
    ///
    /// Can also use `MISE_NO_ENV=1`
    #[usage(long)]
    pub no_env: bool,
    /// Do not execute hooks from config files
    ///
    /// Can also use `MISE_NO_HOOKS=1`
    #[usage(long)]
    pub no_hooks: bool,
    /// Hides elapsed time after each task completes
    ///
    /// Default to always hide with `MISE_TASK_TIMINGS=0`
    #[usage(long, alias = "no-timing", hide = true, verbatim_doc_comment)]
    pub no_timings: bool,
    #[usage(long)]
    pub output: Option<TaskOutput>,
    /// Read/write directly to stdin/stdout/stderr instead of by line
    #[usage(long, global = true)]
    pub raw: bool,
    /// Require lockfile URLs to be present during installation
    ///
    /// Fails if tools don't have pre-resolved URLs in the lockfile for the current platform.
    /// This prevents API calls to GitHub, aqua registry, etc.
    /// Can also be enabled via MISE_LOCKED=1 or settings.locked=true
    #[usage(long, global = true, verbatim_doc_comment)]
    pub locked: bool,
    /// Suppress all task output and mise non-error messages
    #[usage(long, global = true, overrides = &["quiet", "trace", "verbose", "debug", "log_level"])]
    pub silent: bool,
    /// Shows elapsed time after each task completes
    ///
    /// Default to always show with `MISE_TASK_TIMINGS=1`
    #[usage(long, alias = "timing", verbatim_doc_comment, hide = true)]
    pub timings: bool,
    /// Sets log level to trace
    #[usage(long, global = true, hide = true, overrides = &["quiet", "silent", "verbose", "debug", "log_level"])]
    pub trace: bool,
}

fn render_subcommand_help(name: &str, long: bool) -> String {
    let spec = Cli::spec();
    let command = spec
        .root
        .subcommands
        .iter()
        .find(|command| command.cmd.name == name)
        .unwrap_or_else(|| panic!("missing generated {name} command"));
    usage_rs::help::render(spec, command.cmd, long)
        .unwrap_or_else(|| panic!("generated {name} command is outside the usage spec"))
}

#[derive(usage_rs::Subcommands, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum Commands {
    Activate(activate::Activate),
    ToolAlias(Box<tool_alias::ToolAlias>),
    Asdf(asdf::Asdf),
    Backends(backends::Backends),
    BinPaths(bin_paths::BinPaths),
    #[usage(visible_alias = "bs")]
    Bootstrap(bootstrap::Bootstrap),
    Cache(cache::Cache),
    Completion(completion::Completion),
    Config(config::Config),
    Current(current::Current),
    Deactivate(deactivate::Deactivate),
    Direnv(direnv::Direnv),
    Dotfiles(dotfiles::Dotfiles),
    Doctor(doctor::Doctor),
    En(en::En),
    Env(env::Env),
    Exec(exec::Exec),
    Fmt(fmt::Fmt),
    Generate(generate::Generate),
    Github(github::Github),
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
    Oci(oci::Oci),
    Outdated(outdated::Outdated),
    Patrons(patrons::Patrons),
    Plugins(plugins::Plugins),
    Deps(deps::Deps),
    Prune(prune::Prune),
    Registry(registry::Registry),
    #[cfg(debug_assertions)]
    RenderHelp(render_help::RenderHelp),
    Reshim(reshim::Reshim),
    Run(Box<run::Run>),
    Search(search::Search),
    SelfUpdate(self_update::SelfUpdate),
    Set(set::Set),
    Settings(settings::Settings),
    Shell(shell::Shell),
    ShellAlias(shell_alias::ShellAlias),
    Sponsors(sponsors::Sponsors),
    Sync(sync::Sync),
    Tasks(tasks::Tasks),
    TestTool(test_tool::TestTool),
    Token(token::Token),
    Tool(tool::Tool),
    ToolStub(tool_stub::ToolStub),
    Trust(trust::Trust),
    Uninstall(uninstall::Uninstall),
    Unset(unset::Unset),
    Untrust(untrust::Untrust),
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
    fn is_dry_run(&self) -> bool {
        match self {
            Self::Bootstrap(cmd) => cmd.is_dry_run(),
            Self::Edit(cmd) => cmd.is_dry_run(),
            Self::Implode(cmd) => cmd.is_dry_run(),
            Self::Install(cmd) => cmd.is_dry_run(),
            Self::Lock(cmd) => cmd.dry_run,
            Self::Prune(cmd) => cmd.is_dry_run(),
            Self::Run(cmd) => cmd.dry_run,
            Self::Uninstall(cmd) => cmd.is_dry_run(),
            Self::Upgrade(cmd) => cmd.is_dry_run(),
            Self::Use(cmd) => cmd.is_dry_run(),
            _ => false,
        }
    }

    /// Whether this parsed command may trigger a pre-command automatic update.
    ///
    /// This operates on clap's canonical command variant so aliases such as
    /// `dr` inherit the same policy as `doctor`.
    fn allows_auto_update(&self) -> bool {
        !matches!(
            self,
            Self::Activate(_)
                | Self::Completion(_)
                | Self::Deactivate(_)
                | Self::Doctor(_)
                | Self::HookEnv(_)
                | Self::HookNotFound(_)
                | Self::Implode(_)
                | Self::SelfUpdate(_)
                | Self::Settings(_)
                | Self::Shell(_)
                | Self::Usage(_)
                | Self::Version(_)
        )
    }

    fn implicitly_trusts_active_config(&self) -> bool {
        matches!(
            self,
            Self::Exec(_) | Self::Install(_) | Self::Run(_) | Self::Watch(_)
        )
    }

    pub(crate) async fn run(self) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run(),
            Self::ToolAlias(cmd) => cmd.run().await,
            Self::Asdf(cmd) => cmd.run().await,
            Self::Backends(cmd) => cmd.run().await,
            Self::BinPaths(cmd) => cmd.run().await,
            Self::Bootstrap(cmd) => cmd.run().await,
            Self::Cache(cmd) => cmd.run().await,
            Self::Completion(cmd) => cmd.run().await,
            Self::Config(cmd) => cmd.run().await,
            Self::Current(cmd) => cmd.run().await,
            Self::Deactivate(cmd) => cmd.run(),
            Self::Direnv(cmd) => cmd.run().await,
            Self::Dotfiles(cmd) => cmd.run().await,
            Self::Doctor(cmd) => cmd.run().await,
            Self::En(cmd) => cmd.run().await,
            Self::Env(cmd) => cmd.run().await,
            Self::Exec(cmd) => cmd.run().await,
            Self::Fmt(cmd) => cmd.run(),
            Self::Generate(cmd) => cmd.run().await,
            Self::Github(cmd) => cmd.run().await,
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
            Self::Oci(cmd) => cmd.run().await,
            Self::Outdated(cmd) => cmd.run().await,
            Self::Patrons(cmd) => cmd.run().await,
            Self::Plugins(cmd) => cmd.run().await,
            Self::Deps(cmd) => cmd.run().await,
            Self::Prune(cmd) => cmd.run().await,
            Self::Registry(cmd) => cmd.run().await,
            #[cfg(debug_assertions)]
            Self::RenderHelp(cmd) => cmd.run(),
            Self::Reshim(cmd) => cmd.run().await,
            Self::Run(cmd) => (*cmd).run().await,
            Self::Search(cmd) => cmd.run().await,
            Self::SelfUpdate(cmd) => cmd.run().await,
            Self::Set(cmd) => cmd.run().await,
            Self::Settings(cmd) => cmd.run().await,
            Self::Shell(cmd) => cmd.run().await,
            Self::ShellAlias(cmd) => cmd.run().await,
            Self::Sponsors(cmd) => cmd.run(),
            Self::Sync(cmd) => cmd.run().await,
            Self::Tasks(cmd) => cmd.run().await,
            Self::TestTool(cmd) => cmd.run().await,
            Self::Token(cmd) => cmd.run().await,
            Self::Tool(cmd) => cmd.run().await,
            Self::ToolStub(cmd) => cmd.run().await,
            Self::Trust(cmd) => cmd.run().await,
            Self::Uninstall(cmd) => cmd.run().await,
            Self::Unset(cmd) => cmd.run().await,
            Self::Untrust(cmd) => cmd.run(),
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

fn has_dry_run_flag(args: &[String], allow_short: bool) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| {
            matches!(arg.as_str(), "--dry-run" | "--dry-run-code")
                || allow_short
                    && arg
                        .strip_prefix('-')
                        .is_some_and(|flags| !flags.starts_with('-') && flags.contains('n'))
        })
}

fn get_global_flags(cmd: &usage_rs::Command<'_>) -> (Vec<String>, Vec<String>) {
    let mut flags_with_values = Vec::new();
    let mut boolean_flags = Vec::new();

    for arg in cmd.flags {
        let takes_value = arg.takes_value;
        let is_bool = !takes_value;

        if takes_value {
            if let Some(long) = arg.longs.first() {
                flags_with_values.push(format!("--{}", long));
            }
            if let Some(short) = arg.shorts.first() {
                flags_with_values.push(format!("-{}", *short as char));
            }
        } else if is_bool {
            if let Some(long) = arg.longs.first() {
                boolean_flags.push(format!("--{}", long));
            }
            if let Some(short) = arg.shorts.first() {
                boolean_flags.push(format!("-{}", *short as char));
            }
        }
    }

    (flags_with_values, boolean_flags)
}

/// Get all flags (with values and boolean) from both global Cli and Run subcommand
fn get_all_run_flags(cmd: &usage_rs::Command<'_>) -> (Vec<String>, Vec<String>) {
    // Get global flags from Cli
    let (mut flags_with_values, mut boolean_flags) = get_global_flags(cmd);

    // Get run-specific flags from Run subcommand
    if let Some(run_cmd) = cmd.subcommands.iter().find(|s| s.name == "run") {
        let (run_vals, run_bools) = get_global_flags(run_cmd);
        flags_with_values.extend(run_vals);
        boolean_flags.extend(run_bools);
    }

    (flags_with_values, boolean_flags)
}

fn get_value_taking_short_flags(cmd: &usage_rs::Command<'_>) -> Vec<(String, String)> {
    cmd.flags
        .iter()
        .filter(|arg| arg.takes_value)
        .filter_map(|arg| Some((*arg.shorts.first()? as char, *arg.longs.first()?)))
        .map(|(short, long)| (format!("-{short}"), format!("--{long}")))
        .collect()
}

fn get_all_run_value_taking_short_flags(cmd: &usage_rs::Command<'_>) -> Vec<(String, String)> {
    let mut flags = get_value_taking_short_flags(cmd);
    if let Some(run_cmd) = cmd
        .subcommands
        .iter()
        .find(|subcommand| subcommand.name == "run")
    {
        flags.extend(get_value_taking_short_flags(run_cmd));
    }
    flags
}

/// Prefix used to escape flags that should be passed to tasks, not mise
const TASK_ARG_ESCAPE_PREFIX: &str = "\x00MISE_TASK_ARG\x00";

/// One task-side argument, with a leading flag hidden from the parser.
///
/// A lone `-` is the conventional stdin placeholder rather than a flag, so it passes through. That
/// exception was written out at each of the three places that needed this rule; this is the only
/// copy of it now.
fn escape_flag_arg(arg: &str) -> String {
    if arg.starts_with('-') && arg != "-" {
        format!("{TASK_ARG_ESCAPE_PREFIX}{arg}")
    } else {
        arg.to_string()
    }
}

fn escape_args_after_separator(args: &[String], separator_idx: usize) -> Vec<String> {
    let mut result = args[..=separator_idx].to_vec();
    result.extend(args[separator_idx + 1..].iter().map(|a| escape_flag_arg(a)));
    result
}

/// Long and short forms of the top-level flags that consume a following argument.
///
/// Hardcoded rather than derived because `env.rs` needs it from `Lazy` statics
/// during startup — before anything has parsed arguments — and deriving it means
/// building the entire clap tree, which costs ~3.1M instructions. Doing that
/// there is what made every mise command ~6.3M instructions more expensive.
///
/// `test_global_flags_with_values_matches_clap` asserts this equals what clap
/// reports, so adding a value-taking flag to [`Cli`] without updating this list
/// fails CI rather than silently mis-parsing arguments.
pub(crate) const GLOBAL_FLAGS_WITH_VALUES: &[&str] = &[
    "--cd",
    "-C",
    "--env",
    "-E",
    "--jobs",
    "-j",
    "--profile",
    "-P",
    "--shell",
    "-s",
    "--tool",
    "-t",
    "--log-level",
    "--output",
];

/// Index of the first argument that is not a global flag or one of its values.
///
/// Takes a `&Command` so callers that have already built one (argument parsing)
/// use its real flag set. Callers without one want
/// [`first_non_global_arg_idx_cached`].
pub(crate) fn first_non_global_arg_idx(
    cmd: &usage_rs::Command<'_>,
    args: &[String],
) -> Option<usize> {
    let flags = get_global_flags(cmd).0;
    first_non_global_arg_idx_with(|f| flags.iter().any(|x| x == f), args)
}

/// As [`first_non_global_arg_idx`], against [`GLOBAL_FLAGS_WITH_VALUES`].
///
/// For callers with no `Command` to hand, which would otherwise build the whole
/// tree just to read its top-level arguments.
pub(crate) fn first_non_global_arg_idx_cached(args: &[String]) -> Option<usize> {
    first_non_global_arg_idx_with(|f| GLOBAL_FLAGS_WITH_VALUES.contains(&f), args)
}

fn first_non_global_arg_idx_with(
    takes_value: impl Fn(&str) -> bool,
    args: &[String],
) -> Option<usize> {
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--" {
            return None;
        }

        if !arg.starts_with('-') {
            return Some(i);
        }

        let flag_takes_separate_value = if arg.starts_with("--") {
            if arg.contains('=') {
                false
            } else {
                let flag_name = arg.split('=').next().unwrap();
                takes_value(flag_name)
            }
        } else if let Some(flag_name) = arg.get(..2) {
            // `arg.get(..2)` (not `&arg[..2]`) avoids panicking when the arg is
            // not valid UTF-8 in the first place: args are read lossily, so a
            // malformed byte becomes a multi-byte U+FFFD and byte index 2 may not
            // be a char boundary. A short flag is always ASCII, so a non-ASCII
            // prefix simply matches no value-taking flag.
            arg.len() == 2 && takes_value(flag_name)
        } else {
            false
        };

        if flag_takes_separate_value && i + 1 < args.len() {
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

fn is_known_subcommand(cmd: &usage_rs::Command<'_>, arg: &str) -> bool {
    cmd.subcommands
        .iter()
        .flat_map(|s| std::iter::once(s.name).chain(s.aliases.iter().copied()))
        .any(|name| name == arg)
}

fn uses_deprecated_backends_alias(cmd: &usage_rs::Command<'_>, args: &[String]) -> bool {
    matches!(
        first_non_global_arg_idx(cmd, args).and_then(|idx| args.get(idx)),
        Some(arg) if arg == "b"
    )
}

fn warn_deprecated_backends_alias(uses_alias: bool) {
    if uses_alias {
        deprecated_at!(
            "2026.4.0",
            "2027.4.0",
            "cli.backends.b",
            "`mise b` is deprecated. Use `mise backends` instead."
        );
    }
}

/// Escape flags after task names so clap doesn't parse them as mise flags.
/// This preserves ::: separators for multi-task handling while preventing
/// clap from consuming flags like --jobs that appear after task names.
fn escape_task_args(cmd: &usage_rs::Command<'_>, args: &[String]) -> Vec<String> {
    // Find the mise `run` subcommand position. Do not scan past `--`; values
    // after that boundary belong to another command or a task.
    let first_idx = first_non_global_arg_idx(cmd, args);
    let run_pos = first_idx.filter(|&pos| args[pos] == "run");
    let run_pos = match run_pos {
        Some(pos) => pos,
        None => {
            if let (Some(task_idx), Some(separator_idx)) =
                (first_idx, args.iter().position(|a| a == "--"))
            {
                if is_known_subcommand(cmd, &args[task_idx]) || separator_idx <= task_idx {
                    return args.to_vec();
                }
                return escape_args_after_separator(args, separator_idx);
            }
            return args.to_vec();
        }
    };

    if let Some(separator_idx) = args[run_pos + 1..].iter().position(|a| a == "--") {
        let separator_idx = run_pos + 1 + separator_idx;
        // First protect task-side flags before the separator (`run TASK -q -- ...`),
        // then preserve the existing escaping for its tail.
        let mut result = escape_task_args(cmd, &args[..separator_idx]);
        // `separator_idx` was found by matching `"--"`, so the slice starting there begins with it:
        // `escape_args_after_separator(.., 0)` emits that element and then escapes the tail, which
        // is what this branch used to build by hand.
        result.extend(escape_args_after_separator(&args[separator_idx..], 0));
        return result;
    }

    let (flags_with_values, _) = get_all_run_flags(cmd);
    let short_flags_with_values = get_all_run_value_taking_short_flags(cmd);

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
                // clap treats attached values for short options as positional
                // values when the positional allows hyphens. Normalize them to
                // unambiguous long options before parsing.
                let attached_short_value =
                    short_flags_with_values.iter().find_map(|(short, long)| {
                        arg.strip_prefix(short)
                            .filter(|value| !value.is_empty())
                            .map(|value| (long, value))
                    });

                if let Some((long, value)) = attached_short_value {
                    let value = value.strip_prefix('=').unwrap_or(value);
                    result.push(format!("{long}={value}"));
                    i += 1;
                    continue;
                }

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
            // In task args - escape flags so the parser doesn't take them
            result.push(escape_flag_arg(arg));
        }

        i += 1;
    }

    result
}

/// Unescape task args that were escaped by escape_task_args
pub(crate) fn unescape_task_args(args: &[String]) -> Vec<String> {
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

fn preprocess_args_for_naked_run(cmd: &usage_rs::Command<'_>, args: &[String]) -> Vec<String> {
    // Check if this might be a naked run (no subcommand)
    if args.len() < 2 {
        return args.to_vec();
    }

    // If there's already a '--' separator, let clap handle the naked task path.
    // escape_task_args will still protect task-side flags after the separator.
    if args.contains(&"--".to_string()) {
        return args.to_vec();
    }

    // Skip global flags to find the first non-flag argument (subcommand or task)
    let Some(i) = first_non_global_arg_idx(cmd, args) else {
        return args.to_vec();
    };

    // Check if the first non-flag argument is a known subcommand
    if is_known_subcommand(cmd, &args[i]) {
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
    pub(crate) async fn run(args: &Vec<String>) -> Result<()> {
        run_with_exit_signal(Self::run_inner(args), ctrlc::exit_signal()).await
    }

    async fn run_inner(args: &Vec<String>) -> Result<()> {
        // usage-rs's generated `parse()` intercepts this, but mise never calls
        // `parse()` — it uses `parse_from_argv` after shim/naked-run rewriting.
        // Handle the hidden completion protocol here, before config or tools load.
        let completion_argv: Vec<std::ffi::OsString> =
            args.iter().skip(1).map(std::ffi::OsString::from).collect();
        if let Some(answer) = completion::completion_request(&completion_argv) {
            print!("{answer}");
            return Ok(());
        }
        crate::env::ARGS.write().unwrap().clone_from(args);
        let original_cwd = std::env::current_dir().ok();
        // Load .miserc.toml early, before MISE_ENV and other early settings are accessed.
        // This allows setting MISE_ENV in a config file instead of only via env vars.
        crate::config::miserc::init()?;
        if *crate::env::MISE_TOOL_STUB && args.len() >= 2 {
            tool_stub::short_circuit_stub(&args[2..]).await?;
        }
        // Fast-path for hook-env: exit early if nothing has changed
        // This avoids expensive backend::load_tools() and config loading
        if hook_env_module::should_exit_early_fast() {
            measure!("logger", { logger::init() });
            Settings::flush_pending_warnings_before_exit();
            return Ok(());
        }
        measure!("logger", { logger::init() });
        check_working_directory();
        measure!("handle_shim", { shims::handle_shim().await })?;
        let print_version = version::print_version_if_requested(args)?;
        // Clap's tool argument parsers consult installed plugin/tool metadata while
        // resolving registry options. Initialize that filesystem-only state before
        // parsing, while leaving full backend loading until after registry refresh.
        if !print_version {
            measure!("install_state::init", {
                crate::toolset::install_state::init().await?
            });
        }
        // Pre-process args to handle naked runs before parsing
        let cmd = measure!("build_cli_command", { Cli::command() });
        let processed_args = preprocess_args_for_naked_run(cmd, args);
        // Escape flags after task names so they go to tasks, not mise
        let processed_args = escape_task_args(cmd, &processed_args);
        let deprecated_backends_alias = uses_deprecated_backends_alias(cmd, args);

        let parsed_argv: Vec<&std::ffi::OsStr> =
            processed_args.iter().map(std::ffi::OsStr::new).collect();
        let mut cli = measure!("parse_args", {
            Cli::parse_from_argv(&parsed_argv).map_err(|err| usage_error(&parsed_argv[1..], err))
        })?;
        if let Some(Commands::Bootstrap(bootstrap)) = &mut cli.command {
            bootstrap.inherit_root_flags(cli.dry_run, cli.yes);
        }
        config_file::set_implicitly_trust_active_config(
            cli.command
                .as_ref()
                .is_some_and(Commands::implicitly_trusts_active_config)
                || cli.task.is_some(),
        );
        // Validate --cd path BEFORE Settings processes it and changes the directory
        validate_cd_path(&cli.cd)?;
        measure!("add_cli_matches", { Settings::add_cli_matches(&cli) });
        // Propagated, not discarded: this is where `--cd` is actually applied, and a directory
        // that passed the checks above can still refuse the `chdir` — no execute permission, or a
        // path past the length `SetCurrentDirectory` accepts. Dropping the error here does not
        // avoid it, it only defers it: `BASE_SETTINGS` stays empty, so the next `Settings::get()`
        // repeats the same failure and unwraps it.
        measure!("settings", { Settings::try_get() })?;
        let auto_update_command_eligible = !print_version
            && cli
                .command
                .as_ref()
                .is_some_and(Commands::allows_auto_update);
        measure!("auto_update", {
            self_update::maybe_auto_update(
                args,
                original_cwd.as_deref(),
                auto_update_command_eligible,
            )
            .await?
        });
        measure!("trust_active_config", {
            config_file::trust_active_config()?
        });
        measure!("logger", { logger::init() });
        if !print_version {
            measure!("registry::refresh", { crate::registry::refresh().await });
            let _ = measure!("backend::load_tools", { backend::load_tools().await });
        }
        warn_deprecated_backends_alias(deprecated_backends_alias);
        measure!("migrate", { migrate::run().await });
        if let Err(err) = crate::cache::auto_prune() {
            warn!("auto_prune failed: {err:?}");
        }
        let dry_run_requested = cli.dry_run
            || cli.command.as_ref().is_some_and(Commands::is_dry_run)
            // Nested command structs are private to their modules, so inspect
            // their parsed argument span as a fallback. Task/exec arguments
            // after `--` cannot affect this policy. `watch -n` means
            // `--no-shell`, unlike every other mise `-n` flag.
            || has_dry_run_flag(
                &processed_args,
                !matches!(cli.command.as_ref(), Some(Commands::Watch(_))),
            );
        if !print_version
            && !dry_run_requested
            && let Err(err) = crate::tool_purgatory::auto_prune().await
        {
            warn!("tool purgatory cleanup failed: {err:#}");
        }

        debug!("ARGS: {}", &args.join(" "));
        trace!("MISE_BIN: {}", crate::env::MISE_BIN.display_user());
        if print_version {
            version::show_latest().await;
            version::show_version_hint();
            return Err(request_exit(0));
        }
        let _remote_task_artifacts = crate::task::task_fetcher::RemoteTaskArtifactsGuard::new();
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
                    if let Some(page) = usage_rs::help::render(Cli::spec(), Cli::command(), false) {
                        print!("{page}");
                    }
                    return Err(request_exit(0));
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
                        task: Some(task),
                        args: self.task_args.unwrap_or_default(),
                        args_last: self.task_args_last,
                        all: false,
                        affected: false,
                        affected_base: None,
                        affected_head: None,
                        affected_explain: false,
                        affected_json: false,
                        cd: self.cd,
                        continue_on_error: self.continue_on_error,
                        dry_run: self.dry_run,
                        force: self.force,
                        is_linear: false,
                        jobs: self.jobs,
                        no_timings: self.no_timings,
                        output: self.output,
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
                        task_cache: crate::task::TaskCacheMode::from_env()?,
                        task_cache_explain: false,
                        task_cache_explain_json: false,
                        task_cache_stats: false,
                        timeout: None,
                        skip_deps: false,
                        skip_tools: false,
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
                    return Err(request_exit(0));
                }
            }
            if let Some(page) = usage_rs::help::render(Cli::spec(), Cli::command(), false) {
                print!("{page}");
            }
            Err(request_exit(1))
        }
    }
}

async fn run_with_exit_signal<T>(
    command: impl std::future::Future<Output = Result<T>>,
    exit_signal: impl std::future::Future<Output = i32>,
) -> Result<T> {
    tokio::select! {
        result = command => result,
        code = exit_signal => Err(request_exit(code)),
    }
}

fn usage_error(argv: &[&std::ffi::OsStr], err: usage_rs::Error<'_, '_>) -> Report {
    let spec = Cli::spec();
    match err {
        usage_rs::Error::Help { cmd, long } => {
            if let Some(page) = usage_rs::help::render(spec, cmd, long) {
                print!("{page}");
            }
            request_exit(0)
        }
        usage_rs::Error::HelpAll { cmd } => {
            if let Some(page) = usage_rs::help::render_all(spec, cmd) {
                print!("{page}");
            }
            request_exit(0)
        }
        usage_rs::Error::MissingArgsHelp { cmd } => {
            if let Some(page) = usage_rs::help::render(spec, cmd, false) {
                eprint!("{page}");
            }
            request_exit(2)
        }
        usage_rs::Error::Version { long } => {
            let version = if long {
                spec.long_version.or(spec.version)
            } else {
                spec.version
            }
            .unwrap_or_default();
            println!("{} {version}", spec.name);
            request_exit(0)
        }
        err => {
            eprint!("{}", usage_rs::render_failure(spec, argv, &err));
            request_exit(2)
        }
    }
}

const LONG_ABOUT: &str = "mise prepares your development environment before each command runs. https://github.com/jdx/mise";

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise install node@20.0.0</bold>       Install a specific node version
    $ <bold>mise install node@20</bold>           Install a version matching a prefix
    $ <bold>mise install node</bold>              Install the node version defined in config
    $ <bold>mise install</bold>                   Install all plugins/tools defined in config

    $ <bold>mise install cargo:ripgrep</bold>     Install something via cargo
    $ <bold>mise install npm:prettier</bold>      Install something via npm

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

    fn parse_cli<'a>(args: &'a [&'a str]) -> std::result::Result<Cli, usage_rs::Error<'a, 'a>> {
        let argv: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
        Cli::parse_from_argv(&argv)
    }

    #[test]
    fn dry_run_flag_scan_handles_nested_clusters_and_separator() {
        let args = |args: &[&str]| args.iter().map(ToString::to_string).collect::<Vec<_>>();

        assert!(has_dry_run_flag(
            &args(&["mise", "cache", "prune", "--dry-run"]),
            true
        ));
        assert!(has_dry_run_flag(
            &args(&["mise", "bootstrap", "files", "apply", "-nq"]),
            true
        ));
        assert!(!has_dry_run_flag(
            &args(&["mise", "exec", "--", "tool", "--dry-run"]),
            true
        ));
        assert!(!has_dry_run_flag(&args(&["mise", "watch", "-n"]), false));
    }

    #[test]
    fn run_keeps_its_custom_help_route() {
        let cli = parse_cli(&["mise", "run", "--help"]).unwrap();
        let Some(Commands::Run(run)) = cli.command else {
            panic!("expected the run command");
        };
        assert_eq!(run.task.as_deref(), Some("--help"));
    }

    #[test]
    fn bootstrap_inherits_root_yes_and_dry_run_flags() {
        for args in [
            ["mise", "--yes", "bootstrap"].as_slice(),
            ["mise", "--dry-run", "bootstrap"].as_slice(),
            ["mise", "--yes", "--dry-run", "bootstrap"].as_slice(),
        ] {
            let mut cli = parse_cli(args).unwrap();
            let root_flags = (cli.dry_run, cli.yes);
            let Some(Commands::Bootstrap(bootstrap)) = &mut cli.command else {
                panic!("expected bootstrap for {args:?}")
            };
            bootstrap.inherit_root_flags(root_flags.0, root_flags.1);
            assert_eq!(bootstrap.inherited_root_flags(), root_flags, "{args:?}");
        }
    }

    #[test]
    fn tool_stub_forwards_flag_like_arguments() {
        let cli = parse_cli(&["mise", "tool-stub", "jqstub", "--version"]).unwrap();
        let Some(Commands::ToolStub(tool_stub)) = cli.command else {
            panic!("expected tool-stub command")
        };
        assert_eq!(tool_stub.file, PathBuf::from("jqstub"));
        assert_eq!(tool_stub.args, ["--version"]);
    }

    #[test]
    fn test_commands_that_implicitly_trust_active_config() {
        let trusting = [
            vec!["mise", "run", "task"],
            vec!["mise", "install"],
            vec!["mise", "exec", "--", "true"],
            vec!["mise", "watch", "task"],
        ];
        for args in trusting {
            let cli = parse_cli(&args).unwrap();
            assert!(
                cli.command
                    .as_ref()
                    .is_some_and(Commands::implicitly_trusts_active_config),
                "expected {args:?} to imply config trust"
            );
        }

        let non_trusting = [
            vec!["mise", "hook-env", "-s", "bash"],
            vec!["mise", "env"],
            vec!["mise", "ls"],
        ];
        for args in non_trusting {
            let cli = parse_cli(&args).unwrap();
            assert!(
                !cli.command
                    .as_ref()
                    .is_some_and(Commands::implicitly_trusts_active_config),
                "expected {args:?} not to imply config trust"
            );
        }
    }

    #[test]
    fn shell_commands_and_aliases_do_not_allow_auto_update() {
        for args in [
            vec!["mise", "doctor"],
            vec!["mise", "dr"],
            vec!["mise", "hook-env"],
            vec!["mise", "hook-not-found", "missing-bin"],
            vec!["mise", "version"],
            vec!["mise", "v"],
        ] {
            let cli = parse_cli(&args).unwrap();
            assert!(
                !cli.command
                    .as_ref()
                    .is_some_and(Commands::allows_auto_update),
                "expected {args:?} to skip automatic updates"
            );
        }

        let cli = parse_cli(&["mise", "install"]).unwrap();
        assert!(
            cli.command
                .as_ref()
                .is_some_and(Commands::allows_auto_update)
        );
    }

    /// Guards [`GLOBAL_FLAGS_WITH_VALUES`]. It is hardcoded so that startup does
    /// not have to walk the generated tables; this keeps it honest. If you added a
    /// value-taking flag to `Cli`, add it to that list too.
    #[test]
    fn test_global_flags_with_values_matches_generated_tables() {
        let derived = get_global_flags(Cli::command()).0;
        let mut derived_sorted = derived.clone();
        derived_sorted.sort();
        let mut hardcoded: Vec<String> = GLOBAL_FLAGS_WITH_VALUES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        hardcoded.sort();
        assert_eq!(
            hardcoded, derived_sorted,
            "GLOBAL_FLAGS_WITH_VALUES is stale; usage-rs reports {derived:?}"
        );
    }
    #[tokio::test]
    async fn exit_signal_drops_command_future_before_returning() {
        struct DropGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);

        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let guard = DropGuard(dropped.clone());
        let command = async move {
            let _guard = guard;
            std::future::pending::<Result<()>>().await
        };

        let err = run_with_exit_signal(command, std::future::ready(42))
            .await
            .unwrap_err();

        assert_eq!(crate::exit::requested_exit_code(&err), Some(42));
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_bootstrap_command_tree_is_static_and_parses_aliases() {
        let cmd = Cli::command();
        let bootstrap = cmd
            .subcommands
            .iter()
            .find(|subcommand| subcommand.name == "bootstrap")
            .unwrap();
        assert!(!bootstrap.subcommands.is_empty());

        let alias_args = vec![
            "mise".to_string(),
            "bs".to_string(),
            "status".to_string(),
            "--json".to_string(),
        ];
        assert_eq!(preprocess_args_for_naked_run(cmd, &alias_args), alias_args);
        assert!(parse_cli(&["mise", "bs", "status", "--json"]).is_ok());
        assert!(parse_cli(&["mise", "bootstrap", "status", "--json"]).is_ok());
    }

    /// Commands that name a config file to write to accept both spellings, so it
    /// does not matter which one you remember. See
    /// <https://github.com/jdx/mise/discussions/4881>.
    ///
    /// Asserted through the generated tables rather than end-to-end because several of these
    /// commands install or apply something when actually run.
    #[test]
    fn test_config_target_options_accept_both_names() {
        // (command path, argument id, expected alias, available on this platform)
        // `use` and `dotfiles add` are deliberately absent: `-f` is `--force` on both, so
        // they carry `--path` only. See the shadowing test below.
        let cases: &[(&[&str], &str, &str, bool)] = &[
            (&["unuse"], "path", "file", true),
            (&["set"], "file", "path", true),
            (&["unset"], "file", "path", true),
            (&["config", "get"], "file", "path", true),
            (&["config", "set"], "file", "path", true),
            (&["bootstrap", "packages", "use"], "path", "file", true),
            (&["bootstrap", "packages", "import"], "path", "file", true),
            // the brew manager is not registered on Windows
            (
                &["bootstrap", "packages", "brew", "tap"],
                "path",
                "file",
                cfg!(not(windows)),
            ),
            (
                &["bootstrap", "packages", "brew", "untap"],
                "path",
                "file",
                cfg!(not(windows)),
            ),
        ];
        let root = Cli::command();

        for (path, arg_name, alias, available) in cases {
            if !available {
                continue;
            }
            let command = path.iter().fold(root, |command, name| {
                command
                    .subcommands
                    .iter()
                    .find(|subcommand| subcommand.name == *name)
                    .unwrap_or_else(|| panic!("missing command path {}", path.join(" ")))
            });
            let arg = command
                .flags
                .iter()
                .find(|arg| arg.name == *arg_name)
                .unwrap_or_else(|| panic!("missing --{arg_name} on {}", path.join(" ")));

            assert!(
                arg.longs.contains(alias),
                "missing visible --{alias} alias for --{arg_name} on {}",
                path.join(" ")
            );
        }
    }

    /// A `--file`/`--path` alias whose natural short form belongs to a *different* argument
    /// on the same command teaches the wrong flag. `mise dotfiles add` carried `--file` as an
    /// alias of `--path` while `-f` was `--force`, and because its targets accept any string,
    /// `mise dotfiles add -f <path>` silently adopted that config file as a dotfile instead of
    /// writing to it — no error, and `--force` meant no prompt either.
    ///
    /// Walks the whole CLI rather than a fixed list, so re-adding the alias anywhere fails
    /// even if the case table above is left alone.
    #[test]
    fn config_target_aliases_do_not_shadow_another_short_flag() {
        fn check(command: &usage_rs::Command<'_>, path: &mut Vec<String>) {
            for arg in command.flags {
                for alias in arg.longs.iter().skip(1) {
                    // Only this vocabulary — an unrelated alias sharing a letter with some
                    // other flag is ordinary and not what this is about.
                    if *alias != "file" && *alias != "path" {
                        continue;
                    }
                    let short = alias.chars().next().unwrap();
                    if let Some(owner) = command
                        .flags
                        .iter()
                        .find(|other| other.key != arg.key && other.shorts.contains(&(short as u8)))
                    {
                        panic!(
                            "mise {}: --{alias} aliases --{}, but -{short} is --{} — \
                             drop the alias or the collision",
                            path.join(" "),
                            arg.name,
                            owner.name
                        );
                    }
                }
            }
            for subcommand in command.subcommands {
                path.push(subcommand.name.to_string());
                check(subcommand, path);
                path.pop();
            }
        }

        check(Cli::command(), &mut Vec::new());
    }

    #[test]
    fn test_escape_flag_arg_leaves_a_lone_hyphen_alone() {
        // The exception the three copies of this rule each carried: `-` on its own is the
        // conventional stdin placeholder, not a flag, and a task that reads stdin needs it to
        // arrive unchanged. Now that the rule has one home, it is asserted there.
        assert_eq!(escape_flag_arg("-"), "-");
        assert_eq!(escape_flag_arg("plain"), "plain");
        assert!(escape_flag_arg("--help").starts_with(TASK_ARG_ESCAPE_PREFIX));
        assert!(escape_flag_arg("-q").starts_with(TASK_ARG_ESCAPE_PREFIX));
        assert_eq!(
            unescape_task_args(&[escape_flag_arg("--help")]),
            vec!["--help".to_string()]
        );
    }

    #[test]
    fn test_escape_task_args_preserves_task_separator_tail() {
        let cmd = Cli::command();
        let args = vec![
            "mise".to_string(),
            "run".to_string(),
            "atask".to_string(),
            "-q".to_string(),
            "--".to_string(),
            "--".to_string(),
            "--help".to_string(),
        ];

        let escaped = escape_task_args(cmd, &args);
        let separator_idx = escaped.iter().position(|arg| arg == "--").unwrap();
        assert_eq!(escaped[..3], args[..3]);
        assert!(escaped[3].starts_with(TASK_ARG_ESCAPE_PREFIX));
        assert_eq!(separator_idx, 4);
        assert!(escaped[separator_idx + 1].starts_with(TASK_ARG_ESCAPE_PREFIX));
        assert!(escaped[separator_idx + 2].starts_with(TASK_ARG_ESCAPE_PREFIX));
        assert_eq!(
            unescape_task_args(&escaped[separator_idx + 1..]),
            vec!["--".to_string(), "--help".to_string()]
        );
    }

    #[test]
    fn quiet_owns_the_mise_quiet_environment_binding() {
        let quiet = Cli::spec()
            .root
            .flags
            .iter()
            .find(|flag| flag.flag.name == "quiet")
            .expect("--quiet");
        let tool = Cli::spec()
            .root
            .flags
            .iter()
            .find(|flag| flag.flag.name == "tool")
            .expect("--tool");
        assert_eq!(quiet.env, Some("MISE_QUIET"));
        assert_eq!(tool.env, None);
    }

    #[test]
    fn test_run_parser_consumes_the_task_separator() {
        let cmd = Cli::command();
        let args = [
            "mise",
            "run",
            "show-output-on-failure",
            "--",
            "mise",
            "x",
            "node@latest",
            "--",
            "npx",
            "--version",
        ]
        .map(str::to_string);
        let escaped = escape_task_args(cmd, &args);
        let refs = escaped.iter().map(String::as_str).collect::<Vec<_>>();
        let cli = parse_cli(&refs).expect("nested task invocation should parse");
        let Some(Commands::Run(run)) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(run.task.as_deref(), Some("show-output-on-failure"));
        assert!(run.args.is_empty(), "ordinary args: {:?}", run.args);
        assert_eq!(
            unescape_task_args(&run.args_last),
            ["mise", "x", "node@latest", "--", "npx", "--version"]
        );
    }

    #[test]
    fn test_escape_task_args_splits_attached_short_option_values_before_task() {
        let cmd = Cli::command();
        let args = vec![
            "mise".to_string(),
            "run".to_string(),
            "-j1".to_string(),
            "-Ctmp".to_string(),
            "-oprefix".to_string(),
            "atask".to_string(),
        ];

        assert_eq!(
            escape_task_args(cmd, &args),
            [
                "mise",
                "run",
                "--jobs=1",
                "--cd=tmp",
                "--output=prefix",
                "atask",
            ]
            .map(str::to_string)
        );
    }

    #[test]
    fn test_escape_task_args_preserves_equals_attached_option_values() {
        let cmd = Cli::command();
        let args = ["mise", "run", "-C=/tmp", "-o=prefix", "atask"].map(str::to_string);
        let processed = escape_task_args(cmd, &args);

        assert_eq!(
            processed,
            ["mise", "run", "--cd=/tmp", "--output=prefix", "atask"].map(str::to_string)
        );
        let refs: Vec<&str> = processed.iter().map(String::as_str).collect();
        let cli = parse_cli(&refs).unwrap();
        assert_eq!(cli.cd, Some(PathBuf::from("/tmp")));
        let Some(Commands::Run(run)) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(run.task.as_deref(), Some("atask"));
    }

    #[test]
    fn test_escape_task_args_keeps_hyphen_prefixed_attached_value_bound_to_option() {
        let cmd = Cli::command();
        let args = ["mise", "run", "-C-dir", "atask"].map(str::to_string);
        let processed = escape_task_args(cmd, &args);

        assert_eq!(
            processed,
            ["mise", "run", "--cd=-dir", "atask"].map(str::to_string)
        );

        let refs: Vec<&str> = processed.iter().map(String::as_str).collect();
        let cli = parse_cli(&refs).unwrap();
        assert_eq!(cli.cd, Some(PathBuf::from("-dir")));
        let Some(Commands::Run(run)) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(run.task.as_deref(), Some("atask"));
    }

    #[test]
    fn test_aube_node_gyp_bootstrap_would_become_naked_run_without_early_intercept() {
        // Embedded aube re-execs the host as `__node-gyp-bootstrap`. That name is
        // not a mise subcommand, so the naked-run rewrite injects `run` — which is
        // exactly the gemini-cli / node-pty failure mode. `main` must intercept
        // this argv *before* preprocess_args_for_naked_run runs.
        let cmd = Cli::command();
        let args = [
            "mise".to_string(),
            "__node-gyp-bootstrap".to_string(),
            "/tmp/project".to_string(),
        ];
        let processed = preprocess_args_for_naked_run(cmd, &args);
        assert_eq!(
            processed,
            [
                "mise".to_string(),
                "run".to_string(),
                "__node-gyp-bootstrap".to_string(),
                "/tmp/project".to_string(),
            ],
            "if this no longer rewrites to `run`, update main's early aube dispatch"
        );
    }

    #[test]
    fn test_escape_task_args_preserves_naked_task_separator_tail() {
        let cmd = Cli::command();
        let args = vec![
            "mise".to_string(),
            "atask".to_string(),
            "-q".to_string(),
            "--".to_string(),
            "--help".to_string(),
        ];

        assert_eq!(preprocess_args_for_naked_run(cmd, &args), args);
        let escaped = escape_task_args(cmd, &args);
        let separator_idx = escaped.iter().position(|arg| arg == "--").unwrap();
        assert_eq!(escaped[..=separator_idx], args[..=separator_idx]);
        assert!(escaped[separator_idx + 1].starts_with(TASK_ARG_ESCAPE_PREFIX));
        assert_eq!(
            unescape_task_args(&escaped[separator_idx + 1..]),
            vec!["--help".to_string()]
        );
    }

    #[test]
    fn test_escape_task_args_leaves_subcommand_separator_tail_alone() {
        let cmd = Cli::command();
        let args = vec![
            "mise".to_string(),
            "exec".to_string(),
            "--".to_string(),
            "sh".to_string(),
            "--flag".to_string(),
        ];

        assert_eq!(escape_task_args(cmd, &args), args);
    }

    #[test]
    fn test_uses_deprecated_backends_alias() {
        let cmd = Cli::command();
        let args = vec!["mise".to_string(), "b".to_string()];

        assert!(uses_deprecated_backends_alias(cmd, &args));
    }

    #[test]
    fn test_uses_deprecated_backends_alias_after_global_flag() {
        let cmd = Cli::command();
        let args = vec![
            "mise".to_string(),
            "--cd".to_string(),
            "project".to_string(),
            "b".to_string(),
        ];

        assert!(uses_deprecated_backends_alias(cmd, &args));
    }

    #[test]
    fn test_uses_deprecated_backends_alias_ignores_global_flag_value() {
        let cmd = Cli::command();
        let args = vec![
            "mise".to_string(),
            "--cd".to_string(),
            "b".to_string(),
            "backends".to_string(),
        ];

        assert!(!uses_deprecated_backends_alias(cmd, &args));
    }

    #[test]
    fn test_first_non_global_arg_idx_handles_attached_short_flag_values() {
        let cmd = Cli::command();
        let args = |args: &[&str]| {
            args.iter()
                .map(|arg| (*arg).to_string())
                .collect::<Vec<_>>()
        };

        for args in [
            args(&["mise", "-C", "/tmp", "lock"]),
            args(&["mise", "-C/tmp", "lock"]),
            args(&["mise", "-C=/tmp", "lock"]),
            args(&["mise", "-j8", "lock"]),
        ] {
            let idx = first_non_global_arg_idx(cmd, &args).unwrap();
            assert_eq!(args[idx], "lock");
        }
    }

    #[test]
    fn test_uses_deprecated_backends_alias_ignores_task_arg() {
        let cmd = Cli::command();
        let args = vec!["mise".to_string(), "run".to_string(), "b".to_string()];

        assert!(!uses_deprecated_backends_alias(cmd, &args));
    }

    #[test]
    fn test_escape_task_args_ignores_run_after_subcommand_separator() {
        let cmd = Cli::command();
        let args = vec![
            "mise".to_string(),
            "exec".to_string(),
            "--".to_string(),
            "npm".to_string(),
            "run".to_string(),
            "test".to_string(),
            "--help".to_string(),
        ];

        assert_eq!(escape_task_args(cmd, &args), args);
    }
}
