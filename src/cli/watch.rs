use crate::Result;
use crate::cli::args::BackendArg;
use crate::cli::{Cli, run};
use crate::cmd;
use crate::config::Config;
use crate::env;
use crate::exit::exit;
use crate::toolset::ToolsetBuilder;
use clap::{CommandFactory, ValueEnum, ValueHint};
use console::style;
use eyre::bail;
use itertools::Itertools;
use std::cmp::PartialEq;
use std::iter::once;
use std::path::PathBuf;

/// Run task(s) and watch for changes to rerun it
///
/// This command uses the `watchexec` tool to watch for changes to files and rerun the specified task(s).
/// It must be installed for this command to work, but you can install it with `mise use -g watchexec@latest`.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "w", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Watch {
    /// Tasks to run
    /// Can specify multiple tasks by separating with `:::`
    /// e.g.: `mise run task1 arg1 arg2 ::: task2 arg1 arg2`
    #[clap(allow_hyphen_values = true, verbatim_doc_comment)]
    task: Option<String>,

    /// Tasks to run
    #[clap(short, long, verbatim_doc_comment, hide = true)]
    task_flag: Vec<String>,

    /// Task and arguments to run
    #[clap(allow_hyphen_values = true, trailing_var_arg = true)]
    args: Vec<String>,

    /// Files to watch
    /// Defaults to sources from the tasks(s)
    #[clap(short, long, verbatim_doc_comment, hide = true)]
    glob: Vec<String>,

    #[clap(flatten)]
    watchexec: WatchexecArgs,
}

impl Watch {
    pub async fn run(self) -> Result<()> {
        if let Some(task) = &self.task {
            if task == "-h" {
                self.get_clap_command().print_help()?;
                return Ok(());
            }
            if task == "--help" {
                self.get_clap_command().print_long_help()?;
                return Ok(());
            }
        }
        let config = Config::get().await?;
        let ts = ToolsetBuilder::new().build(&config).await?;
        if let Err(err) = which::which("watchexec") {
            let watchexec: BackendArg = "watchexec".into();
            if !ts.versions.contains_key(&watchexec) {
                eprintln!("{}: {}", style("Error").red().bold(), err);
                eprintln!("{}: Install watchexec with:", style("Hint").bold());
                eprintln!("  mise use -g watchexec@latest");
                exit(1);
            }
        }
        let args = once(self.task)
            .flatten()
            .chain(self.task_flag.iter().cloned())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>();
        if args.is_empty() {
            bail!("No tasks specified");
        }
        let tasks = run::get_task_lists(&config, &args, false).await?;
        let mut args = vec![];
        if let Some(delay_run) = self.watchexec.delay_run {
            args.push("--delay-run".to_string());
            args.push(delay_run);
        }
        if let Some(poll) = self.watchexec.poll {
            args.push("--poll".to_string());
            args.push(poll);
        }
        if let Some(signal) = self.watchexec.signal {
            args.push("--signal".to_string());
            args.push(signal);
        }
        if let Some(stop_signal) = self.watchexec.stop_signal {
            args.push("--stop-signal".to_string());
            args.push(stop_signal);
        }
        if self.watchexec.stop_timeout != "10s" {
            args.push("--stop-timeout".to_string());
            args.push(self.watchexec.stop_timeout);
        }
        if self.watchexec.debounce != "50ms" {
            args.push("--debounce".to_string());
            args.push(self.watchexec.debounce);
        }
        if self.watchexec.stdin_quit {
            args.push("--stdin-quit".to_string());
        }
        if self.watchexec.no_vcs_ignore {
            args.push("--no-vcs-ignore".to_string());
        }
        if self.watchexec.no_project_ignore {
            args.push("--no-project-ignore".to_string());
        }
        if self.watchexec.no_global_ignore {
            args.push("--no-global-ignore".to_string());
        }
        if self.watchexec.no_default_ignore {
            args.push("--no-default-ignore".to_string());
        }
        if self.watchexec.no_discover_ignore {
            args.push("--no-discover-ignore".to_string());
        }
        if self.watchexec.ignore_nothing {
            args.push("--ignore-nothing".to_string());
        }
        if self.watchexec.postpone {
            args.push("--postpone".to_string());
        }
        if let Some(screen_clear) = self.watchexec.screen_clear {
            args.push("--clear".to_string());
            if let ClearMode::Reset = screen_clear {
                args.push("reset".to_string());
            }
        }
        if self.watchexec.restart {
            args.push("--restart".to_string());
        }
        if self.watchexec.on_busy_update != OnBusyUpdate::DoNothing {
            args.push("--on-busy-update".to_string());
            args.push(self.watchexec.on_busy_update.to_string());
        }
        if !self.watchexec.signal_map.is_empty() {
            for signal_map in &self.watchexec.signal_map {
                args.push("--map-signal".to_string());
                args.push(signal_map.to_string());
            }
        }
        if !self.watchexec.recursive_paths.is_empty() {
            for path in &self.watchexec.recursive_paths {
                args.push("--watch".to_string());
                args.push(path.to_string_lossy().to_string());
            }
        }
        if !self.watchexec.non_recursive_paths.is_empty() {
            for path in &self.watchexec.non_recursive_paths {
                args.push("--watch-non-recursive".to_string());
                args.push(path.to_string_lossy().to_string());
            }
        }
        if !self.watchexec.filter_extensions.is_empty() {
            for ext in &self.watchexec.filter_extensions {
                args.push("--exts".to_string());
                args.push(ext.to_string());
            }
        }
        if !self.watchexec.filter_patterns.is_empty() {
            for pattern in &self.watchexec.filter_patterns {
                args.push("--filter".to_string());
                args.push(pattern.to_string());
            }
        }
        if let Some(watch_file) = &self.watchexec.watch_file {
            args.push("--watch-file".to_string());
            args.push(watch_file.to_string_lossy().to_string());
        }
        let globs = if self.glob.is_empty() {
            tasks
                .iter()
                .flat_map(|t| t.sources.clone())
                .collect::<Vec<_>>()
        } else {
            self.glob.clone()
        };
        if !globs.is_empty() {
            args.push("-f".to_string());
            args.extend(itertools::intersperse(globs, "-f".to_string()).collect::<Vec<_>>());
        }
        args.extend([
            "--".to_string(),
            env::MISE_BIN.to_string_lossy().to_string(),
            "run".to_string(),
        ]);
        let task_args = itertools::intersperse(
            tasks.iter().map(|t| {
                let mut args = vec![t.name.to_string()];
                args.extend(t.args.iter().map(|a| a.to_string()));
                args
            }),
            vec![":::".to_string()],
        )
        .flatten()
        .collect_vec();
        for arg in task_args {
            args.push(arg);
        }
        debug!("$ watchexec {}", args.join(" "));
        let mut cmd = cmd::cmd("watchexec", &args);
        for (k, v) in ts.env_with_path(&config).await? {
            cmd = cmd.env(k, v);
        }
        cmd.run()?;
        Ok(())
    }

    fn get_clap_command(&self) -> clap::Command {
        Cli::command()
            .get_subcommands()
            .find(|s| s.get_name() == "watch")
            .unwrap()
            .clone()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise watch build</bold>
    Runs the "build" tasks. Will re-run the tasks when any of its sources change.
    Uses "sources" from the tasks definition to determine which files to watch.

    $ <bold>mise watch build --glob src/**/*.rs</bold>
    Runs the "build" tasks but specify the files to watch with a glob pattern.
    This overrides the "sources" from the tasks definition.

    $ <bold>mise watch build --clear</bold>
    Extra arguments are passed to watchexec. See `watchexec --help` for details.

    $ <bold>mise watch serve --watch src --exts rs --restart</bold>
    Starts an api server, watching for changes to "*.rs" files in "./src" and kills/restarts the server when they change.
"#
);

//region watchexec
const OPTSET_FILTERING: &str = "Filtering";
const OPTSET_COMMAND: &str = "Command";
const OPTSET_DEBUGGING: &str = "Debugging";
const OPTSET_OUTPUT: &str = "Output";

#[derive(Debug, clap::Args)]
pub struct WatchexecArgs {
    /// Watch a specific file or directory
    ///
    /// By default, Watchexec watches the current directory.
    ///
    /// When watching a single file, it's often better to watch the containing directory instead,
    /// and filter on the filename. Some editors may replace the file with a new one when saving,
    /// and some platforms may not detect that or further changes.
    ///
    /// Upon starting, Watchexec resolves a "project origin" from the watched paths. See the help
    /// for '--project-origin' for more information.
    ///
    /// This option can be specified multiple times to watch multiple files or directories.
    ///
    /// The special value '/dev/null', provided as the only path watched, will cause Watchexec to
    /// not watch any paths. Other event sources (like signals or key events) may still be used.
    #[arg(
		short = 'w',
		long = "watch",
		help_heading = OPTSET_FILTERING,
		value_hint = ValueHint::AnyPath,
		value_name = "PATH",
    )]
    pub recursive_paths: Vec<PathBuf>,

    /// Watch a specific directory, non-recursively
    ///
    /// Unlike '-w', folders watched with this option are not recursed into.
    ///
    /// This option can be specified multiple times to watch multiple directories non-recursively.
    #[arg(
		short = 'W',
		long = "watch-non-recursive",
		help_heading = OPTSET_FILTERING,
		value_hint = ValueHint::AnyPath,
		value_name = "PATH",
    )]
    pub non_recursive_paths: Vec<PathBuf>,

    /// Watch files and directories from a file
    ///
    /// Each line in the file will be interpreted as if given to '-w'.
    ///
    /// For more complex uses (like watching non-recursively), use the argfile capability: build a
    /// file containing command-line options and pass it to watchexec with `@path/to/argfile`.
    ///
    /// The special value '-' will read from STDIN; this in incompatible with '--stdin-quit'.
    #[arg(
		short = 'F',
		long,
		help_heading = OPTSET_FILTERING,
		value_hint = ValueHint::AnyPath,
		value_name = "PATH",
    )]
    pub watch_file: Option<PathBuf>,

    /// Clear screen before running command
    ///
    /// If this doesn't completely clear the screen, try '--clear=reset'.
    #[arg(
		short = 'c',
		long = "clear",
		help_heading = OPTSET_OUTPUT,
		num_args = 0..=1,
		default_missing_value = "clear",
		value_name = "MODE",
    )]
    pub screen_clear: Option<ClearMode>,

    /// What to do when receiving events while the command is running
    ///
    /// Default is to 'do-nothing', which ignores events while the command is running, so that
    /// changes that occur due to the command are ignored, like compilation outputs. You can also
    /// use 'queue' which will run the command once again when the current run has finished if any
    /// events occur while it's running, or 'restart', which terminates the running command and starts
    /// a new one. Finally, there's 'signal', which only sends a signal; this can be useful with
    /// programs that can reload their configuration without a full restart.
    ///
    /// The signal can be specified with the '--signal' option.
    #[arg(
        short,
        long,
        default_value = "do-nothing",
        hide_default_value = true,
        value_name = "MODE"
    )]
    pub on_busy_update: OnBusyUpdate,

    /// Restart the process if it's still running
    ///
    /// This is a shorthand for '--on-busy-update=restart'.
    #[arg(
		short,
		long,
		conflicts_with_all = ["on_busy_update"],
    )]
    pub restart: bool,

    /// Send a signal to the process when it's still running
    ///
    /// Specify a signal to send to the process when it's still running. This implies
    /// '--on-busy-update=signal'; otherwise the signal used when that mode is 'restart' is
    /// controlled by '--stop-signal'.
    ///
    /// See the long documentation for '--stop-signal' for syntax.
    ///
    /// Signals are not supported on Windows at the moment, and will always be overridden to 'kill'.
    /// See '--stop-signal' for more on Windows "signals".
    #[arg(
		short,
		long,
		conflicts_with_all = ["restart"],
		value_name = "SIGNAL"
    )]
    pub signal: Option<String>,

    /// Signal to send to stop the command
    ///
    /// This is used by 'restart' and 'signal' modes of '--on-busy-update' (unless '--signal' is
    /// provided). The restart behaviour is to send the signal, wait for the command to exit, and if
    /// it hasn't exited after some time (see '--timeout-stop'), forcefully terminate it.
    ///
    /// The default on unix is "SIGTERM".
    ///
    /// Input is parsed as a full signal name (like "SIGTERM"), a short signal name (like "TERM"),
    /// or a signal number (like "15"). All input is case-insensitive.
    ///
    /// On Windows this option is technically supported but only supports the "KILL" event, as
    /// Watchexec cannot yet deliver other events. Windows doesn't have signals as such; instead it
    /// has termination (here called "KILL" or "STOP") and "CTRL+C", "CTRL+BREAK", and "CTRL+CLOSE"
    /// events. For portability the unix signals "SIGKILL", "SIGINT", "SIGTERM", and "SIGHUP" are
    /// respectively mapped to these.
    #[arg(long, value_name = "SIGNAL")]
    pub stop_signal: Option<String>,

    /// Time to wait for the command to exit gracefully
    ///
    /// This is used by the 'restart' mode of '--on-busy-update'. After the graceful stop signal
    /// is sent, Watchexec will wait for the command to exit. If it hasn't exited after this time,
    /// it is forcefully terminated.
    ///
    /// Takes a unit-less value in seconds, or a time span value such as "5min 20s".
    /// Providing a unit-less value is deprecated and will warn; it will be an error in the future.
    ///
    /// The default is 10 seconds. Set to 0 to immediately force-kill the command.
    ///
    /// This has no practical effect on Windows as the command is always forcefully terminated; see
    /// '--stop-signal' for why.
    #[arg(
        long,
        default_value = "10s",
        hide_default_value = true,
        value_name = "TIMEOUT"
    )]
    pub stop_timeout: String,

    /// Translate signals from the OS to signals to send to the command
    ///
    /// Takes a pair of signal names, separated by a colon, such as "TERM:INT" to map SIGTERM to
    /// SIGINT. The first signal is the one received by watchexec, and the second is the one sent to
    /// the command. The second can be omitted to discard the first signal, such as "TERM:" to
    /// not do anything on SIGTERM.
    ///
    /// If SIGINT or SIGTERM are mapped, then they no longer quit Watchexec. Besides making it hard
    /// to quit Watchexec itself, this is useful to send pass a Ctrl-C to the command without also
    /// terminating Watchexec and the underlying program with it, e.g. with "INT:INT".
    ///
    /// This option can be specified multiple times to map multiple signals.
    ///
    /// Signal syntax is case-insensitive for short names (like "TERM", "USR2") and long names (like
    /// "SIGKILL", "SIGHUP"). Signal numbers are also supported (like "15", "31"). On Windows, the
    /// forms "STOP", "CTRL+C", and "CTRL+BREAK" are also supported to receive, but Watchexec cannot
    /// yet deliver other "signals" than a STOP.
    #[arg(long = "map-signal", value_name = "SIGNAL:SIGNAL")]
    pub signal_map: Vec<String>,

    /// Time to wait for new events before taking action
    ///
    /// When an event is received, Watchexec will wait for up to this amount of time before handling
    /// it (such as running the command). This is essential as what you might perceive as a single
    /// change may actually emit many events, and without this behaviour, Watchexec would run much
    /// too often. Additionally, it's not infrequent that file writes are not atomic, and each write
    /// may emit an event, so this is a good way to avoid running a command while a file is
    /// partially written.
    ///
    /// An alternative use is to set a high value (like "30min" or longer), to save power or
    /// bandwidth on intensive tasks, like an ad-hoc backup script. In those use cases, note that
    /// every accumulated event will build up in memory.
    ///
    /// Takes a unit-less value in milliseconds, or a time span value such as "5sec 20ms".
    /// Providing a unit-less value is deprecated and will warn; it will be an error in the future.
    ///
    /// The default is 50 milliseconds. Setting to 0 is highly discouraged.
    #[arg(
        long,
        short,
        default_value = "50ms",
        hide_default_value = true,
        value_name = "TIMEOUT"
    )]
    pub debounce: String,

    /// Exit when stdin closes
    ///
    /// This watches the stdin file descriptor for EOF, and exits Watchexec gracefully when it is
    /// closed. This is used by some process managers to avoid leaving zombie processes around.
    #[arg(long)]
    pub stdin_quit: bool,

    /// Don't load gitignores
    ///
    /// Among other VCS exclude files, like for Mercurial, Subversion, Bazaar, DARCS, Fossil. Note
    /// that Watchexec will detect which of these is in use, if any, and only load the relevant
    /// files. Both global (like '~/.gitignore') and local (like '.gitignore') files are considered.
    ///
    /// This option is useful if you want to watch files that are ignored by Git.
    #[arg(
		long,
		help_heading = OPTSET_FILTERING,
    )]
    pub no_vcs_ignore: bool,

    /// Don't load project-local ignores
    ///
    /// This disables loading of project-local ignore files, like '.gitignore' or '.ignore' in the
    /// watched project. This is contrasted with '--no-vcs-ignore', which disables loading of Git
    /// and other VCS ignore files, and with '--no-global-ignore', which disables loading of global
    /// or user ignore files, like '~/.gitignore' or '~/.config/watchexec/ignore'.
    ///
    /// Supported project ignore files:
    ///
    ///   - Git: .gitignore at project root and child directories, .git/info/exclude, and the file pointed to by `core.excludesFile` in .git/config.
    ///   - Mercurial: .hgignore at project root and child directories.
    ///   - Bazaar: .bzrignore at project root.
    ///   - Darcs: _darcs/prefs/boring
    ///   - Fossil: .fossil-settings/ignore-glob
    ///   - Ripgrep/Watchexec/generic: .ignore at project root and child directories.
    ///
    /// VCS ignore files (Git, Mercurial, Bazaar, Darcs, Fossil) are only used if the corresponding
    /// VCS is discovered to be in use for the project/origin. For example, a .bzrignore in a Git
    /// repository will be discarded.
    #[arg(
		long,
		help_heading = OPTSET_FILTERING,
		verbatim_doc_comment,
    )]
    pub no_project_ignore: bool,

    /// Don't load global ignores
    ///
    /// This disables loading of global or user ignore files, like '~/.gitignore',
    /// '~/.config/watchexec/ignore', or '%APPDATA%\Bazzar\2.0\ignore'. Contrast with
    /// '--no-vcs-ignore' and '--no-project-ignore'.
    ///
    /// Supported global ignore files
    ///
    ///   - Git (if core.excludesFile is set): the file at that path
    ///   - Git (otherwise): the first found of $XDG_CONFIG_HOME/git/ignore, %APPDATA%/.gitignore, %USERPROFILE%/.gitignore, $HOME/.config/git/ignore, $HOME/.gitignore.
    ///   - Bazaar: the first found of %APPDATA%/Bazzar/2.0/ignore, $HOME/.bazaar/ignore.
    ///   - Watchexec: the first found of $XDG_CONFIG_HOME/watchexec/ignore, %APPDATA%/watchexec/ignore, %USERPROFILE%/.watchexec/ignore, $HOME/.watchexec/ignore.
    ///
    /// Like for project files, Git and Bazaar global files will only be used for the corresponding
    /// VCS as used in the project.
    #[arg(
		long,
		help_heading = OPTSET_FILTERING,
		verbatim_doc_comment,
    )]
    pub no_global_ignore: bool,

    /// Don't use internal default ignores
    ///
    /// Watchexec has a set of default ignore patterns, such as editor swap files, `*.pyc`, `*.pyo`,
    /// `.DS_Store`, `.bzr`, `_darcs`, `.fossil-settings`, `.git`, `.hg`, `.pijul`, `.svn`, and
    /// Watchexec log files.
    #[arg(
		long,
		help_heading = OPTSET_FILTERING,
    )]
    pub no_default_ignore: bool,

    /// Don't discover ignore files at all
    ///
    /// This is a shorthand for '--no-global-ignore', '--no-vcs-ignore', '--no-project-ignore', but
    /// even more efficient as it will skip all the ignore discovery mechanisms from the get go.
    ///
    /// Note that default ignores are still loaded, see '--no-default-ignore'.
    #[arg(
		long,
		help_heading = OPTSET_FILTERING,
    )]
    pub no_discover_ignore: bool,

    /// Don't ignore anything at all
    ///
    /// This is a shorthand for '--no-discover-ignore', '--no-default-ignore'.
    ///
    /// Note that ignores explicitly loaded via other command line options, such as '--ignore' or
    /// '--ignore-file', will still be used.
    #[arg(
		long,
		help_heading = OPTSET_FILTERING,
    )]
    pub ignore_nothing: bool,

    /// Wait until first change before running command
    ///
    /// By default, Watchexec will run the command once immediately. With this option, it will
    /// instead wait until an event is detected before running the command as normal.
    #[arg(long, short)]
    pub postpone: bool,

    /// Sleep before running the command
    ///
    /// This option will cause Watchexec to sleep for the specified amount of time before running
    /// the command, after an event is detected. This is like using "sleep 5 && command" in a shell,
    /// but portable and slightly more efficient.
    ///
    /// Takes a unit-less value in seconds, or a time span value such as "2min 5s".
    /// Providing a unit-less value is deprecated and will warn; it will be an error in the future.
    #[arg(long, value_name = "DURATION")]
    pub delay_run: Option<String>,

    /// Poll for filesystem changes
    ///
    /// By default, and where available, Watchexec uses the operating system's native file system
    /// watching capabilities. This option disables that and instead uses a polling mechanism, which
    /// is less efficient but can work around issues with some file systems (like network shares) or
    /// edge cases.
    ///
    /// Optionally takes a unit-less value in milliseconds, or a time span value such as "2s 500ms",
    /// to use as the polling interval. If not specified, the default is 30 seconds.
    /// Providing a unit-less value is deprecated and will warn; it will be an error in the future.
    ///
    /// Aliased as '--force-poll'.
    #[arg(
		long,
		alias = "force-poll",
		num_args = 0..=1,
		default_missing_value = "30s",
		value_name = "INTERVAL",
    )]
    pub poll: Option<String>,

    /// Use a different shell
    ///
    /// By default, Watchexec will use '$SHELL' if it's defined or a default of 'sh' on Unix-likes,
    /// and either 'pwsh', 'powershell', or 'cmd' (CMD.EXE) on Windows, depending on what Watchexec
    /// detects is the running shell.
    ///
    /// With this option, you can override that and use a different shell, for example one with more
    /// features or one which has your custom aliases and functions.
    ///
    /// If the value has spaces, it is parsed as a command line, and the first word used as the
    /// shell program, with the rest as arguments to the shell.
    ///
    /// The command is run with the '-c' flag (except for 'cmd' on Windows, where it's '/C').
    ///
    /// The special value 'none' can be used to disable shell use entirely. In that case, the
    /// command provided to Watchexec will be parsed, with the first word being the executable and
    /// the rest being the arguments, and executed directly. Note that this parsing is rudimentary,
    /// and may not work as expected in all cases.
    ///
    /// Using 'none' is a little more efficient and can enable a stricter interpretation of the
    /// input, but it also means that you can't use shell features like globbing, redirection,
    /// control flow, logic, or pipes.
    ///
    /// Examples:
    ///
    /// Use without shell:
    ///
    ///   $ watchexec -n -- zsh -x -o shwordsplit scr
    ///
    /// Use with powershell core:
    ///
    ///   $ watchexec --shell=pwsh -- Test-Connection localhost
    ///
    /// Use with CMD.exe:
    ///
    ///   $ watchexec --shell=cmd -- dir
    ///
    /// Use with a different unix shell:
    ///
    ///   $ watchexec --shell=bash -- 'echo $BASH_VERSION'
    ///
    /// Use with a unix shell and options:
    ///
    ///   $ watchexec --shell='zsh -x -o shwordsplit' -- scr
    #[arg(
		long,
		help_heading = OPTSET_COMMAND,
		value_name = "SHELL",
    )]
    pub shell: Option<String>,

    /// Shorthand for '--shell=none'
    #[arg(
		short = 'n',
		help_heading = OPTSET_COMMAND,
    )]
    pub no_shell: bool,

    /// Configure event emission
    ///
    /// Watchexec can emit event information when running a command, which can be used by the child
    /// process to target specific changed files.
    ///
    /// One thing to take care with is assuming inherent behaviour where there is only chance.
    /// Notably, it could appear as if the `RENAMED` variable contains both the original and the new
    /// path being renamed. In previous versions, it would even appear on some platforms as if the
    /// original always came before the new. However, none of this was true. It's impossible to
    /// reliably and portably know which changed path is the old or new, "half" renames may appear
    /// (only the original, only the new), "unknown" renames may appear (change was a rename, but
    /// whether it was the old or new isn't known), rename events might split across two debouncing
    /// boundaries, and so on.
    ///
    /// This option controls where that information is emitted. It defaults to 'none', which doesn't
    /// emit event information at all. The other options are 'environment' (deprecated), 'stdio',
    /// 'file', 'json-stdio', and 'json-file'.
    ///
    /// The 'stdio' and 'file' modes are text-based: 'stdio' writes absolute paths to the stdin of
    /// the command, one per line, each prefixed with `create:`, `remove:`, `rename:`, `modify:`,
    /// or `other:`, then closes the handle; 'file' writes the same thing to a temporary file, and
    /// its path is given with the $WATCHEXEC_EVENTS_FILE environment variable.
    ///
    /// There are also two JSON modes, which are based on JSON objects and can represent the full
    /// set of events Watchexec handles. Here's an example of a folder being created on Linux:
    ///
    /// ```json
    ///   {
    ///     "tags": [
    ///       {
    ///         "kind": "path",
    ///         "absolute": "/home/user/your/new-folder",
    ///         "filetype": "dir"
    ///       },
    ///       {
    ///         "kind": "fs",
    ///         "simple": "create",
    ///         "full": "Create(Folder)"
    ///       },
    ///       {
    ///         "kind": "source",
    ///         "source": "filesystem",
    ///       }
    ///     ],
    ///     "metadata": {
    ///       "notify-backend": "inotify"
    ///     }
    ///   }
    /// ```
    ///
    /// The fields are as follows:
    ///
    ///   - `tags`, structured event data.
    ///   - `tags[].kind`, which can be:
    ///     * 'path', along with:
    ///       + `absolute`, an absolute path.
    ///       + `filetype`, a file type if known ('dir', 'file', 'symlink', 'other').
    ///     * 'fs':
    ///       + `simple`, the "simple" event type ('access', 'create', 'modify', 'remove', or 'other').
    ///       + `full`, the "full" event type, which is too complex to fully describe here, but looks like 'General(Precise(Specific))'.
    ///     * 'source', along with:
    ///       + `source`, the source of the event ('filesystem', 'keyboard', 'mouse', 'os', 'time', 'internal').
    ///     * 'keyboard', along with:
    ///       + `keycode`. Currently only the value 'eof' is supported.
    ///     * 'process', for events caused by processes:
    ///       + `pid`, the process ID.
    ///     * 'signal', for signals sent to Watchexec:
    ///       + `signal`, the normalised signal name ('hangup', 'interrupt', 'quit', 'terminate', 'user1', 'user2').
    ///     * 'completion', for when a command ends:
    ///       + `disposition`, the exit disposition ('success', 'error', 'signal', 'stop', 'exception', 'continued').
    ///       + `code`, the exit, signal, stop, or exception code.
    ///   - `metadata`, additional information about the event.
    ///
    /// The 'json-stdio' mode will emit JSON events to the standard input of the command, one per
    /// line, then close stdin. The 'json-file' mode will create a temporary file, write the
    /// events to it, and provide the path to the file with the $WATCHEXEC_EVENTS_FILE
    /// environment variable.
    ///
    /// Finally, the 'environment' mode was the default until 2.0. It sets environment variables
    /// with the paths of the affected files, for filesystem events:
    ///
    /// $WATCHEXEC_COMMON_PATH is set to the longest common path of all of the below variables,
    /// and so should be prepended to each path to obtain the full/real path. Then:
    ///
    ///   - $WATCHEXEC_CREATED_PATH is set when files/folders were created
    ///   - $WATCHEXEC_REMOVED_PATH is set when files/folders were removed
    ///   - $WATCHEXEC_RENAMED_PATH is set when files/folders were renamed
    ///   - $WATCHEXEC_WRITTEN_PATH is set when files/folders were modified
    ///   - $WATCHEXEC_META_CHANGED_PATH is set when files/folders' metadata were modified
    ///   - $WATCHEXEC_OTHERWISE_CHANGED_PATH is set for every other kind of pathed event
    ///
    /// Multiple paths are separated by the system path separator, ';' on Windows and ':' on unix.
    /// Within each variable, paths are deduplicated and sorted in binary order (i.e. neither
    /// Unicode nor locale aware).
    ///
    /// This is the legacy mode, is deprecated, and will be removed in the future. The environment
    /// is a very restricted space, while also limited in what it can usefully represent. Large
    /// numbers of files will either cause the environment to be truncated, or may error or crash
    /// the process entirely. The $WATCHEXEC_COMMON_PATH is also unintuitive, as demonstrated by the
    /// multiple confused queries that have landed in my inbox over the years.
    #[arg(
		long,
		help_heading = OPTSET_COMMAND,
		verbatim_doc_comment,
		default_value = "none",
		hide_default_value = true,
		value_name = "MODE",
		required_if_eq("only_emit_events", "true"),
    )]
    pub emit_events_to: EmitEvents,

    /// Only emit events to stdout, run no commands.
    ///
    /// This is a convenience option for using Watchexec as a file watcher, without running any
    /// commands. It is almost equivalent to using `cat` as the command, except that it will not
    /// spawn a new process for each event.
    ///
    /// This option requires `--emit-events-to` to be set, and restricts the available modes to
    /// `stdio` and `json-stdio`, modifying their behaviour to write to stdout instead of the stdin
    /// of the command.
    #[arg(
		long,
		help_heading = OPTSET_OUTPUT,
		conflicts_with_all = ["manual"],
    )]
    pub only_emit_events: bool,

    /// Add env vars to the command
    ///
    /// This is a convenience option for setting environment variables for the command, without
    /// setting them for the Watchexec process itself.
    ///
    /// Use key=value syntax. Multiple variables can be set by repeating the option.
    #[arg(
		long,
		short = 'E',
		help_heading = OPTSET_COMMAND,
		value_name = "KEY=VALUE",
    )]
    pub env: Vec<String>,

    /// Configure how the process is wrapped
    ///
    /// By default, Watchexec will run the command in a process group in Unix, and in a Job Object
    /// in Windows.
    ///
    /// Some Unix programs prefer running in a session, while others do not work in a process group.
    ///
    /// Use 'group' to use a process group, 'session' to use a process session, and 'none' to run
    /// the command directly. On Windows, either of 'group' or 'session' will use a Job Object.
    #[arg(
		long,
		help_heading = OPTSET_COMMAND,
		value_name = "MODE",
		default_value = "group",
    )]
    pub wrap_process: WrapMode,

    /// Alert when commands start and end
    ///
    /// With this, Watchexec will emit a desktop notification when a command starts and ends, on
    /// supported platforms. On unsupported platforms, it may silently do nothing, or log a warning.
    #[arg(
		short = 'N',
		long,
		help_heading = OPTSET_OUTPUT,
    )]
    pub notify: bool,

    /// When to use terminal colours
    ///
    /// Setting the environment variable `NO_COLOR` to any value is equivalent to `--color=never`.
    #[arg(
		long,
		help_heading = OPTSET_OUTPUT,
		default_value = "auto",
		value_name = "MODE",
		alias = "colour",
    )]
    pub color: ColourMode,

    /// Print how long the command took to run
    ///
    /// This may not be exactly accurate, as it includes some overhead from Watchexec itself. Use
    /// the `time` utility, high-precision timers, or benchmarking tools for more accurate results.
    #[arg(
		long,
		help_heading = OPTSET_OUTPUT,
    )]
    pub timings: bool,

    /// Don't print starting and stopping messages
    ///
    /// By default Watchexec will print a message when the command starts and stops. This option
    /// disables this behaviour, so only the command's output, warnings, and errors will be printed.
    #[arg(
		short,
		long,
		help_heading = OPTSET_OUTPUT,
    )]
    pub quiet: bool,

    /// Ring the terminal bell on command completion
    #[arg(
		long,
		help_heading = OPTSET_OUTPUT,
    )]
    pub bell: bool,

    /// Set the project origin
    ///
    /// Watchexec will attempt to discover the project's "origin" (or "root") by searching for a
    /// variety of markers, like files or directory patterns. It does its best but sometimes gets it
    /// it wrong, and you can override that with this option.
    ///
    /// The project origin is used to determine the path of certain ignore files, which VCS is being
    /// used, the meaning of a leading '/' in filtering patterns, and maybe more in the future.
    ///
    /// When set, Watchexec will also not bother searching, which can be significantly faster.
    #[arg(
		long,
		value_hint = ValueHint::DirPath,
		value_name = "DIRECTORY",
    )]
    pub project_origin: Option<PathBuf>,

    /// Set the working directory
    ///
    /// By default, the working directory of the command is the working directory of Watchexec. You
    /// can change that with this option. Note that paths may be less intuitive to use with this.
    #[arg(
		long,
		value_hint = ValueHint::DirPath,
		value_name = "DIRECTORY",
    )]
    pub workdir: Option<PathBuf>,

    /// Filename extensions to filter to
    ///
    /// This is a quick filter to only emit events for files with the given extensions. Extensions
    /// can be given with or without the leading dot (e.g. 'js' or '.js'). Multiple extensions can
    /// be given by repeating the option or by separating them with commas.
    #[arg(
		long = "exts",
		short = 'e',
		help_heading = OPTSET_FILTERING,
		value_delimiter = ',',
		value_name = "EXTENSIONS",
    )]
    pub filter_extensions: Vec<String>,

    /// Filename patterns to filter to
    ///
    /// Provide a glob-like filter pattern, and only events for files matching the pattern will be
    /// emitted. Multiple patterns can be given by repeating the option. Events that are not from
    /// files (e.g. signals, keyboard events) will pass through untouched.
    #[arg(
		long = "filter",
		short = 'f',
		help_heading = OPTSET_FILTERING,
		value_name = "PATTERN",
    )]
    pub filter_patterns: Vec<String>,

    /// Files to load filters from
    ///
    /// Provide a path to a file containing filters, one per line. Empty lines and lines starting
    /// with '#' are ignored. Uses the same pattern format as the '--filter' option.
    ///
    /// This can also be used via the $WATCHEXEC_FILTER_FILES environment variable.
    #[arg(
		long = "filter-file",
		help_heading = OPTSET_FILTERING,
		value_delimiter = env::PATH_ENV_SEP,
		value_hint = ValueHint::FilePath,
		value_name = "PATH",
		env = "WATCHEXEC_FILTER_FILES",
		hide_env = true,
    )]
    pub filter_files: Vec<PathBuf>,

    /// [experimental] Filter programs.
    ///
    /// /!\ This option is EXPERIMENTAL and may change and/or vanish without notice.
    ///
    /// Provide your own custom filter programs in jaq (similar to jq) syntax. Programs are given
    /// an event in the same format as described in '--emit-events-to' and must return a boolean.
    /// Invalid programs will make watchexec fail to start; use '-v' to see program runtime errors.
    ///
    /// In addition to the jaq stdlib, watchexec adds some custom filter definitions:
    ///
    ///   - 'path | file_meta' returns file metadata or null if the file does not exist.
    ///
    ///   - 'path | file_size' returns the size of the file at path, or null if it does not exist.
    ///
    ///   - 'path | file_read(bytes)' returns a string with the first n bytes of the file at path.
    ///     If the file is smaller than n bytes, the whole file is returned. There is no filter to
    ///     read the whole file at once to encourage limiting the amount of data read and processed.
    ///
    ///   - 'string | hash', and 'path | file_hash' return the hash of the string or file at path.
    ///     No guarantee is made about the algorithm used: treat it as an opaque value.
    ///
    ///   - 'any | kv_store(key)', 'kv_fetch(key)', and 'kv_clear' provide a simple key-value store.
    ///     Data is kept in memory only, there is no persistence. Consistency is not guaranteed.
    ///
    ///   - 'any | printout', 'any | printerr', and 'any | log(level)' will print or log any given
    ///     value to stdout, stderr, or the log (levels = error, warn, info, debug, trace), and
    ///     pass the value through (so '[1] | log("debug") | .[]' will produce a '1' and log '[1]').
    ///
    /// All filtering done with such programs, and especially those using kv or filesystem access,
    /// is much slower than the other filtering methods. If filtering is too slow, events will back
    /// up and stall watchexec. Take care when designing your filters.
    ///
    /// If the argument to this option starts with an '@', the rest of the argument is taken to be
    /// the path to a file containing a jaq program.
    ///
    /// Jaq programs are run in order, after all other filters, and short-circuit: if a filter (jaq
    /// or not) rejects an event, execution stops there, and no other filters are run. Additionally,
    /// they stop after outputting the first value, so you'll want to use 'any' or 'all' when
    /// iterating, otherwise only the first item will be processed, which can be quite confusing!
    ///
    /// Find user-contributed programs or submit your own useful ones at
    /// <https://github.com/watchexec/watchexec/discussions/592>.
    ///
    /// ## Examples:
    ///
    /// Regexp ignore filter on paths:
    ///
    ///   'all(.tags[] | select(.kind == "path"); .absolute | test("[.]test[.]js$")) | not'
    ///
    /// Pass any event that creates a file:
    ///
    ///   'any(.tags[] | select(.kind == "fs"); .simple == "create")'
    ///
    /// Pass events that touch executable files:
    ///
    ///   'any(.tags[] | select(.kind == "path" && .filetype == "file"); .absolute | metadata | .executable)'
    ///
    /// Ignore files that start with shebangs:
    ///
    ///   'any(.tags[] | select(.kind == "path" && .filetype == "file"); .absolute | read(2) == "#!") | not'
    #[arg(
		long = "filter-prog",
		short = 'J',
		help_heading = OPTSET_FILTERING,
		value_name = "EXPRESSION",
    )]
    pub filter_programs: Vec<String>,

    /// Filename patterns to filter out
    ///
    /// Provide a glob-like filter pattern, and events for files matching the pattern will be
    /// excluded. Multiple patterns can be given by repeating the option. Events that are not from
    /// files (e.g. signals, keyboard events) will pass through untouched.
    #[arg(
		long = "ignore",
		short = 'i',
		help_heading = OPTSET_FILTERING,
		value_name = "PATTERN",
    )]
    pub ignore_patterns: Vec<String>,

    /// Files to load ignores from
    ///
    /// Provide a path to a file containing ignores, one per line. Empty lines and lines starting
    /// with '#' are ignored. Uses the same pattern format as the '--ignore' option.
    ///
    /// This can also be used via the $WATCHEXEC_IGNORE_FILES environment variable.
    #[arg(
		long = "ignore-file",
		help_heading = OPTSET_FILTERING,
		value_delimiter = env::PATH_ENV_SEP,
		value_hint = ValueHint::FilePath,
		value_name = "PATH",
		env = "WATCHEXEC_IGNORE_FILES",
		hide_env = true,
    )]
    pub ignore_files: Vec<PathBuf>,

    /// Filesystem events to filter to
    ///
    /// This is a quick filter to only emit events for the given types of filesystem changes. Choose
    /// from 'access', 'create', 'remove', 'rename', 'modify', 'metadata'. Multiple types can be
    /// given by repeating the option or by separating them with commas. By default, this is all
    /// types except for 'access'.
    ///
    /// This may apply filtering at the kernel level when possible, which can be more efficient, but
    /// may be more confusing when reading the logs.
    #[arg(
		long = "fs-events",
		help_heading = OPTSET_FILTERING,
		default_value = "create,remove,rename,modify,metadata",
		value_delimiter = ',',
		hide_default_value = true,
		value_name = "EVENTS",
    )]
    pub filter_fs_events: Vec<FsEvent>,

    /// Don't emit fs events for metadata changes
    ///
    /// This is a shorthand for '--fs-events create,remove,rename,modify'. Using it alongside the
    /// '--fs-events' option is non-sensical and not allowed.
    #[arg(
		long = "no-meta",
		help_heading = OPTSET_FILTERING,
		conflicts_with = "filter_fs_events",
    )]
    pub filter_fs_meta: bool,

    /// Print events that trigger actions
    ///
    /// This prints the events that triggered the action when handling it (after debouncing), in a
    /// human readable form. This is useful for debugging filters.
    ///
    /// Use '-vvv' instead when you need more diagnostic information.
    #[arg(
		long,
		help_heading = OPTSET_DEBUGGING,
    )]
    pub print_events: bool,

    /// Show the manual page
    ///
    /// This shows the manual page for Watchexec, if the output is a terminal and the 'man' program
    /// is available. If not, the manual page is printed to stdout in ROFF format (suitable for
    /// writing to a watchexec.1 file).
    #[arg(
		long,
		help_heading = OPTSET_DEBUGGING,
    )]
    pub manual: bool,
    // /// Change to this directory before executing the command
    // #[clap(short = 'C', long, value_hint = ValueHint::DirPath, long)]
    // pub cd: Option<PathBuf>,
    //
    // /// Don't actually run the tasks(s), just print them in order of execution
    // #[clap(long, short = 'n', verbatim_doc_comment)]
    // pub dry_run: bool,
    //
    // /// Force the tasks to run even if outputs are up to date
    // #[clap(long, short, verbatim_doc_comment)]
    // pub force: bool,
    //
    // /// Print stdout/stderr by line, prefixed with the tasks's label
    // /// Defaults to true if --jobs > 1
    // /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    // #[clap(long, short, verbatim_doc_comment, overrides_with = "interleave")]
    // pub prefix: bool,
    //
    // /// Print directly to stdout/stderr instead of by line
    // /// Defaults to true if --jobs == 1
    // /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    // #[clap(long, short, verbatim_doc_comment, overrides_with = "prefix")]
    // pub interleave: bool,
    //
    // /// Tool(s) to also add
    // /// e.g.: node@20 python@3.10
    // #[clap(short, long, value_name = "TOOL@VERSION")]
    // pub tool: Vec<ToolArg>,
    //
    // /// Number of tasks to run in parallel
    // /// [default: 4]
    // /// Configure with `jobs` config or `MISE_JOBS` env var
    // #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    // pub jobs: Option<usize>,
    //
    // /// Read/write directly to stdin/stdout/stderr instead of by line
    // /// Configure with `raw` config or `MISE_RAW` env var
    // #[clap(long, short, verbatim_doc_comment)]
    // pub raw: bool,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum EmitEvents {
    #[default]
    Environment,
    Stdio,
    File,
    JsonStdio,
    JsonFile,
    None,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum, PartialEq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum OnBusyUpdate {
    #[default]
    Queue,
    DoNothing,
    Restart,
    Signal,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum WrapMode {
    #[default]
    Group,
    Session,
    None,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ClearMode {
    #[default]
    Clear,
    Reset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum FsEvent {
    Access,
    Create,
    Remove,
    Rename,
    Modify,
    Metadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ShellCompletion {
    Bash,
    Elvish,
    Fish,
    Nu,
    Powershell,
    Zsh,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ColourMode {
    Auto,
    Always,
    Never,
}
//endregion
