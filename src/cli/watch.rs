use crate::Result;
use crate::args::BackendArg;
use crate::cli::render_subcommand_help;
use crate::cmd;
use crate::config::Config;
use crate::dirs;
use crate::env;
use crate::request_exit;
use crate::task::task_source_checker::task_cwd;
use crate::task::{Deps, Task};
use crate::toolset::ToolsetBuilder;
use console::style;
use itertools::Itertools;
use std::cmp::PartialEq;
use std::iter::once;
use std::path::{Path, PathBuf};

/// Run tasks and rerun them when files change
///
/// Runs the tasks with `mise run` under watchexec, then reruns them whenever
/// one of their `sources` changes, including the sources of their dependencies
/// unless you pass `--skip-deps`. When no selected task declares `sources`,
/// watchexec watches the current directory. With no task, runs the `default`
/// task.
///
/// Choose what to watch with `--watch`, `--exts`, and `--filter`, and use
/// `--print-events` to see what triggered a run. Arguments for the task go
/// after `--`. Requires watchexec, which `mise use -g watchexec` installs.
///
/// To keep a server or database running alongside your tasks, with readiness
/// checks and automatic restarts, see `mise daemons` (experimental):
/// https://mise.jdx.dev/daemons.html. For cron scheduling and standalone
/// process management, see pitchfork: https://pitchfork.jdx.dev
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "w",
    example(
        "mise watch build",
        help = "Run build and rerun it whenever its sources change"
    ),
    example("mise watch build --clear", help = "Clear the screen before each run"),
    example(
        "mise watch serve --watch src --exts rs --restart",
        help = "Start an API server and restart it when Rust files in ./src change"
    ),
    example(
        "mise watch test -- --verbose",
        help = "Pass arguments after -- to the task"
    ),
    unknown_flags = "value"
)]
pub(crate) struct Watch {
    /// Task to watch (default: `default`); separate several tasks with `:::`
    task: Option<String>,

    /// Tasks to run
    #[usage(short, long, hide = true)]
    task_flag: Vec<String>,

    /// Arguments for the task; `:::` starts the next task
    #[usage(allow_hyphen_values = true, trailing_var_arg = true)]
    args: Vec<String>,

    /// Globs to watch instead of the tasks' sources
    #[usage(short, long, hide = true)]
    glob: Vec<String>,

    /// Run and watch only the named tasks, not the tasks they depend on
    #[usage(long)]
    pub skip_deps: bool,

    #[usage(flatten)]
    watchexec: WatchexecArgs,
}

impl Watch {
    pub(crate) async fn run(self) -> Result<()> {
        if let Some(task) = &self.task {
            if task == "-h" {
                miseprint!("{}", render_subcommand_help("watch", false))?;
                return Ok(());
            }
            if task == "--help" {
                miseprint!("{}", render_subcommand_help("watch", true))?;
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
                return Err(request_exit(1));
            }
        }
        let mut args = once(self.task)
            .flatten()
            .chain(self.task_flag.iter().cloned())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>();
        if args.is_empty() {
            args.push("default".to_string());
        }
        let tasks =
            crate::task::task_list::get_task_lists(&config, &args, false, false, false).await?;
        let watched_tasks = if self.skip_deps {
            tasks.to_vec()
        } else {
            let deps = Deps::new(&config, tasks.clone()).await?;
            deps.all().cloned().collect()
        };
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
        if self.watchexec.no_vcs_ignore || tasks_disable_vcs_ignores(&watched_tasks) {
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
        args.extend(wrap_process_args(self.watchexec.wrap_process));
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
        // Forward user-supplied filtering/debug flags that are parsed into
        // WatchexecArgs but were never re-emitted onto the watchexec command
        // line (#7776). watchexec unions repeated --ignore flags, so these
        // combine with the source-derived ignores pushed below rather than
        // clobbering them.
        if !self.watchexec.ignore_patterns.is_empty() {
            for pattern in &self.watchexec.ignore_patterns {
                args.push("--ignore".to_string());
                args.push(pattern.to_string());
            }
        }
        if !self.watchexec.ignore_files.is_empty() {
            for path in &self.watchexec.ignore_files {
                args.push("--ignore-file".to_string());
                args.push(path.to_string_lossy().to_string());
            }
        }
        if self.watchexec.print_events {
            args.push("--print-events".to_string());
        }
        // Filter anchor: the path that --project-origin is set to and that
        // every glob filter is made relative to. watchexec interprets -f
        // patterns relative to the origin and silently rejects absolute
        // paths, so the anchor must be an ancestor of every task cwd.
        let (globs, ignores, extra_watch_dirs, filter_anchor) = if !self.glob.is_empty() {
            (self.glob.clone(), Vec::new(), Vec::new(), None)
        } else {
            let mut task_cwds: Vec<(&_, PathBuf)> = Vec::with_capacity(watched_tasks.len());
            for t in &watched_tasks {
                let cwd = task_cwd(t, &config).await?;
                task_cwds.push((t, cwd));
            }
            // Pre-resolve sources to absolute paths so the anchor can be
            // widened to cover any source that escapes its task's cwd via
            // `..` or an absolute path.
            let parsed: Vec<Vec<(SourceKind, PathBuf)>> = task_cwds
                .iter()
                .map(|(t, cwd)| t.sources.iter().map(|s| parse_source(s, cwd)).collect())
                .collect();
            // If no task declared any sources, opt out of source-based
            // watching entirely and let watchexec apply its defaults.
            if parsed.iter().all(|v| v.is_empty()) {
                (Vec::new(), Vec::new(), Vec::new(), None)
            } else {
                let configured = config
                    .monorepo_root()
                    .or_else(|| config.project_root.clone());
                let common = common_ancestor(
                    task_cwds
                        .iter()
                        .map(|(_, c)| c.as_path())
                        .chain(parsed.iter().flatten().map(|(_, p)| p.as_path())),
                );
                let anchor: PathBuf = match (configured, common) {
                    (Some(mut cfg), Some(common)) => {
                        while !common.starts_with(&cfg) {
                            if !cfg.pop() {
                                break;
                            }
                        }
                        if cfg.as_os_str().is_empty() {
                            common
                        } else {
                            cfg
                        }
                    }
                    (Some(cfg), None) => cfg,
                    (None, Some(common)) => common,
                    (None, None) => dirs::CWD.clone().unwrap_or_default(),
                };
                let resolved: Vec<Vec<String>> = parsed
                    .iter()
                    .map(|sources| {
                        sources
                            .iter()
                            .map(|(k, abs)| relativize_source(*k, abs, &anchor))
                            .collect()
                    })
                    .collect();
                let cwds: Vec<PathBuf> =
                    task_cwds.iter().map(|(_, c)| c.clone()).unique().collect();
                let mut watch_dirs = cwds.clone();
                for (kind, abs) in parsed.iter().flatten() {
                    if matches!(kind, SourceKind::Negation) {
                        continue;
                    }
                    let dir = source_watch_dir(abs);
                    // Already covered by a recursively-watched cwd.
                    if cwds.iter().any(|c| dir.starts_with(c)) {
                        continue;
                    }
                    watch_dirs.push(dir);
                }
                let watch_dirs: Vec<PathBuf> = watch_dirs.into_iter().unique().collect();
                let (i, e) = merge_watch_patterns(resolved.iter().map(|v| v.as_slice()));
                (i, e, watch_dirs, Some(anchor))
            }
        };
        if let Some(anchor) = &filter_anchor {
            args.push("--project-origin".to_string());
            args.push(anchor.to_string_lossy().to_string());
        }
        // Always include each task's cwd as a watch path
        for path in &extra_watch_dirs {
            if self.watchexec.recursive_paths.contains(path) {
                continue;
            }
            args.push("--watch".to_string());
            args.push(path.to_string_lossy().to_string());
        }
        if !globs.is_empty() {
            args.push("-f".to_string());
            args.extend(itertools::intersperse(globs, "-f".to_string()).collect::<Vec<_>>());
        }
        if !ignores.is_empty() {
            args.push("--ignore".to_string());
            args.extend(
                itertools::intersperse(ignores, "--ignore".to_string()).collect::<Vec<_>>(),
            );
        }
        args.extend([
            "--".to_string(),
            env::MISE_BIN.to_string_lossy().to_string(),
            "run".to_string(),
        ]);
        if self.skip_deps {
            args.push("--skip-deps".to_string());
        }
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
        // Propagate profiles selected with -E/--env to the nested `mise run` command.
        if !env::MISE_ENV.is_empty() {
            cmd = cmd.env("MISE_ENV", env::MISE_ENV.join(","));
        }

        // watchexec's --clear=reset resets the controlling terminal, which
        // also clears termios flags such as ECHO, and it does not put them back
        // when it is interrupted. Capture them so the guard can restore them
        // however this scope ends: normal return, `?`, or the future being
        // dropped by the ctrl-c branch of `run_with_exit_signal` (#8269).
        #[cfg(unix)]
        let _terminal = TerminalState::capture();
        // ...and however the *process* ends, including the exits that never
        // reach a `Drop`: a `mise watch` nested under a `mise run` is killed by
        // that run's `exit::kill_all()`, which sends SIGTERM.
        #[cfg(unix)]
        let _signal_restore = _terminal.arm();

        cmd.run()?;
        Ok(())
    }
}

/// Longest path that is a prefix of every input path. Returns `None` for
/// an empty iterator.
fn common_ancestor<I, P>(paths: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut iter = paths.into_iter();
    let first = iter.next()?;
    let mut acc: Vec<std::ffi::OsString> = first
        .as_ref()
        .components()
        .map(|c| c.as_os_str().to_os_string())
        .collect();
    for p in iter {
        let n = acc
            .iter()
            .zip(p.as_ref().components())
            .take_while(|(a, b)| a.as_os_str() == b.as_os_str())
            .count();
        acc.truncate(n);
    }
    Some(acc.iter().collect())
}

#[derive(Clone, Copy, Debug)]
enum SourceKind {
    Negation,
    LiteralBang,
    Plain,
}

/// Parse a source pattern into its kind and an absolute path
fn parse_source(s: &str, cwd: &Path) -> (SourceKind, PathBuf) {
    let (kind, rest) = if let Some(r) = s.strip_prefix('!') {
        (SourceKind::Negation, r)
    } else if s.starts_with("\\!") {
        (SourceKind::LiteralBang, &s[1..])
    } else {
        (SourceKind::Plain, s)
    };
    let p = Path::new(rest);
    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(rest)
    };
    (kind, normalize_path(&absolute))
}

/// Resolve `.` and `..` components without touching the FS.
/// Used so a source like `../shared/src/*.ts` produces a path
/// we can contain in the anchor.
fn normalize_path(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            Component::CurDir => {}
            _ => out.push(c.as_os_str()),
        }
    }
    out
}

/// Extracts the directory that should be watched for a source pattern.
/// Basically slices up to the first glob-like character.
fn source_watch_dir(absolute: &Path) -> PathBuf {
    use std::path::Component;
    let is_glob = |s: &std::ffi::OsStr| s.to_string_lossy().contains(['*', '?', '[', '{']);
    let mut dir = PathBuf::new();
    let mut found_glob = false;
    for c in absolute.components() {
        if let Component::Normal(part) = c
            && is_glob(part)
        {
            found_glob = true;
            break;
        }
        dir.push(c.as_os_str());
    }
    if found_glob {
        dir
    } else {
        dir.parent().map(|p| p.to_path_buf()).unwrap_or(dir)
    }
}

/// Express an already-absolute source path relative to the filter anchor,
/// re-applying the original negation/literal-bang prefix.
fn relativize_source(kind: SourceKind, absolute: &Path, anchor: &Path) -> String {
    let relative = match absolute.strip_prefix(anchor) {
        Ok(p) => p.to_path_buf(),
        Err(_) => {
            warn!(
                "watch source {} is outside filter anchor {}; watchexec will silently drop it",
                absolute.display(),
                anchor.display()
            );
            absolute.to_path_buf()
        }
    };
    let relative = relative.to_string_lossy();
    let relative = if std::path::MAIN_SEPARATOR == '/' {
        relative.into_owned()
    } else {
        relative.replace(std::path::MAIN_SEPARATOR, "/")
    };
    match kind {
        SourceKind::Negation => format!("!{relative}"),
        SourceKind::LiteralBang | SourceKind::Plain if relative.starts_with('!') => {
            format!("\\{relative}")
        }
        SourceKind::LiteralBang | SourceKind::Plain => relative,
    }
}

fn tasks_disable_vcs_ignores(tasks: &[Task]) -> bool {
    tasks
        .iter()
        .any(|task| task.watch.as_ref().is_some_and(|watch| watch.no_vcs_ignore))
}

/// Merge each task's `sources` into the (filter, ignore) pair watchexec
/// expects.
///
/// Watchexec doesn't model gitignore-style re-inclusion, so the per-task
/// semantics from `task_source_checker` collapse here into a flat union.
/// To avoid one task's `!pat` silently suppressing another task's literal
/// positive `pat`, an exclude is dropped from the global ignore list when
/// the same pattern appears as a positive include in any watched task.
fn merge_watch_patterns<'a, I>(task_sources: I) -> (Vec<String>, Vec<String>)
where
    I: IntoIterator<Item = &'a [String]>,
{
    let mut inc = Vec::new();
    let mut exc_candidates: Vec<String> = Vec::new();
    for sources in task_sources {
        for s in sources {
            if let Some(rest) = s.strip_prefix('!') {
                exc_candidates.push(rest.to_string());
            } else if let Some(rest) = s.strip_prefix("\\!") {
                inc.push(format!("!{rest}"));
            } else {
                inc.push(s.clone());
            }
        }
    }
    let inc: Vec<String> = inc.into_iter().unique().collect();
    let exc: Vec<String> = exc_candidates
        .into_iter()
        .unique()
        .filter(|pat| !inc.contains(pat))
        .collect();
    (inc, exc)
}

//region watchexec
// The watchexec flags `mise watch` accepts. The ones it forwards are documented;
// the ones it parses but does not forward are hidden.
#[derive(Debug, usage_rs::Args)]
pub(crate) struct WatchexecArgs {
    /// Watch a file or directory
    ///
    /// By default, mise watches each task's directory and any source directory
    /// outside it, and filters events to the tasks' `sources`. When no task
    /// declares sources, watchexec watches the current directory. Paths given
    /// here are added to the task directories, or replace the current-directory
    /// default when there are no sources. Events are still filtered to the
    /// sources, so use `--filter` to widen what triggers a rerun.
    ///
    /// To watch a single file, watch its directory and filter on the file name
    /// instead: some editors replace a file when they save it, and some
    /// platforms do not report changes to the replacement. Repeat the flag to
    /// watch several paths.
    #[usage(
        short = 'w',
        long = "watch",
        help_heading = "Filtering",
        value_hint = usage_rs::ValueHint::AnyPath,
        value_name = "PATH",
    )]
    pub recursive_paths: Vec<PathBuf>,

    /// Watch a directory without recursing into it
    ///
    /// Repeat the flag to watch several directories.
    #[usage(
        short = 'W',
        long = "watch-non-recursive",
        help_heading = "Filtering",
        value_hint = usage_rs::ValueHint::AnyPath,
        value_name = "PATH",
    )]
    pub non_recursive_paths: Vec<PathBuf>,

    /// Read paths to watch from a file, one per line
    ///
    /// Each line is treated like a `--watch` value. The value `-` reads from
    /// stdin and cannot be combined with `--stdin-quit`.
    #[usage(
        short = 'F',
        long,
        help_heading = "Filtering",
        value_hint = usage_rs::ValueHint::AnyPath,
        value_name = "PATH",
    )]
    pub watch_file: Option<PathBuf>,

    /// Clear the screen before each run
    ///
    /// If the default `clear` mode leaves output behind, use `--clear=reset`.
    /// Because the mode is optional, `mise watch --clear build` reads `build`
    /// as the mode: put `--clear` after the task name, as in
    /// `mise watch build --clear`, or write the mode with `=`.
    #[usage(
        short = 'c',
        long = "clear",
        help_heading = "Output",
        num_args = 0..=1,
        default_missing = "clear",
        value_enum,
        value_name = "MODE",
    )]
    pub screen_clear: Option<ClearMode>,

    /// What to do when files change while the tasks are running
    ///
    /// `do-nothing` (the default) ignores the change, so files the tasks
    /// write do not trigger another run. `queue` runs the tasks again when the
    /// current run finishes. `restart` stops the running tasks and starts them
    /// again. `signal` sends the `--signal` signal and keeps the tasks running,
    /// for programs that reload their configuration on a signal.
    #[usage(
        short,
        long,
        default = "do-nothing",
        hide_default_value = true,
        value_enum,
        value_name = "MODE"
    )]
    pub on_busy_update: OnBusyUpdate,

    /// Restart the tasks if they are still running when files change
    ///
    /// Same as `--on-busy-update=restart`.
    #[usage(
        short,
        long,
        conflicts = ["on_busy_update"],
    )]
    pub restart: bool,

    /// Signal to send to the running tasks when files change
    ///
    /// Implies `--on-busy-update=signal`. Accepts the same forms as
    /// `--stop-signal`. Signals are not supported on Windows, where the tasks
    /// are always killed.
    #[usage(
        short,
        long,
        conflicts = ["restart"],
        value_name = "SIGNAL"
    )]
    pub signal: Option<String>,

    /// Signal that stops the tasks before a restart (default on Unix: SIGTERM)
    ///
    /// Used by the `restart` and `signal` modes of `--on-busy-update` unless
    /// `--signal` is given. Accepts a full name (`SIGTERM`), a short name
    /// (`TERM`), or a number (`15`), in any case. If the tasks have not exited
    /// after `--stop-timeout`, they are killed. On Windows the tasks are always
    /// killed.
    #[usage(long, value_name = "SIGNAL")]
    pub stop_signal: Option<String>,

    /// How long to wait for the tasks to stop before killing them (default: 10s)
    ///
    /// Takes a duration such as `5s` or `1min 20s`. Set it to 0 to kill the
    /// tasks right away. Has no effect on Windows, where the tasks are always
    /// killed.
    #[usage(
        long,
        default = "10s",
        hide_default_value = true,
        value_name = "TIMEOUT"
    )]
    pub stop_timeout: String,

    /// Translate a signal that watchexec receives into one it sends to the tasks
    ///
    /// Takes two signal names separated by a colon, such as `TERM:INT`. Leave
    /// out the second name to discard the first signal (`TERM:`). A mapped
    /// SIGINT or SIGTERM no longer stops watching; `INT:INT` passes Ctrl-C to
    /// the tasks without stopping watchexec. Repeat the flag to map several
    /// signals.
    #[usage(long = "map-signal", value_name = "SIGNAL:SIGNAL")]
    pub signal_map: Vec<String>,

    /// How long to wait for more changes before running (default: 50ms)
    ///
    /// One save can produce several events, and a file can be written in
    /// pieces, so watchexec waits this long after an event before running the
    /// tasks. Takes a duration such as `500ms` or `2s`.
    #[usage(
        long,
        short,
        default = "50ms",
        hide_default_value = true,
        value_name = "TIMEOUT"
    )]
    pub debounce: String,

    /// Exit when stdin closes
    #[usage(long)]
    pub stdin_quit: bool,

    /// Do not apply Git and other version control ignore files
    ///
    /// Use it to watch files that Git ignores. A task can turn this on for
    /// itself with `watch = { no_vcs_ignore = true }`.
    #[usage(long, help_heading = "Filtering")]
    pub no_vcs_ignore: bool,

    /// Do not apply ignore files in the project, such as `.gitignore` and
    /// `.ignore`
    #[usage(long, help_heading = "Filtering")]
    pub no_project_ignore: bool,

    /// Do not apply global ignore files, such as `~/.gitignore` and
    /// `~/.config/watchexec/ignore`
    #[usage(long, help_heading = "Filtering")]
    pub no_global_ignore: bool,

    /// Do not apply watchexec's built-in ignores
    ///
    /// They cover editor swap files, `*.pyc`, `.DS_Store`, and version control
    /// directories such as `.git`.
    #[usage(long, help_heading = "Filtering")]
    pub no_default_ignore: bool,

    /// Do not look for ignore files at all
    ///
    /// Same as `--no-global-ignore --no-vcs-ignore --no-project-ignore`, but
    /// faster. Built-in ignores still apply.
    #[usage(long, help_heading = "Filtering")]
    pub no_discover_ignore: bool,

    /// Ignore nothing
    ///
    /// Same as `--no-discover-ignore --no-default-ignore`. Patterns from
    /// `--ignore` and `--ignore-file` still apply.
    #[usage(long, help_heading = "Filtering")]
    pub ignore_nothing: bool,

    /// Wait for the first change before running the tasks
    #[usage(long, short)]
    pub postpone: bool,

    /// Wait this long after a change before running the tasks, e.g. 5s
    #[usage(long, value_name = "DURATION")]
    pub delay_run: Option<String>,

    /// Poll for changes instead of using native file watching
    ///
    /// Use it on file systems where native events do not work, such as network
    /// shares. Takes an optional interval such as `2s` (default: 30s). Also
    /// accepted as `--force-poll`.
    #[usage(
        long,
        alias = "force-poll",
        num_args = 0..=1,
        default_missing = "30s",
        value_name = "INTERVAL",
    )]
    pub poll: Option<String>,

    // Hidden because it is not forwarded: mise runs tasks with `mise run`.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(long, help_heading = "Command", value_name = "SHELL", hide = true)]
    pub shell: Option<String>,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(short = 'n', help_heading = "Command", hide = true)]
    pub no_shell: bool,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(
        long,
        help_heading = "Command",
        default = "none",
        hide_default_value = true,
        value_name = "MODE",
        required_if_eq("only_emit_events", "true"),
        value_enum,
        hide = true
    )]
    pub emit_events_to: EmitEvents,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(
        long,
        help_heading = "Output",
        conflicts = ["manual"],
        hide = true,
    )]
    pub only_emit_events: bool,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    ///
    /// Before `--`, mise also reads `-E` and `--env` as the global `--env` flag,
    /// which selects the mise config environment.
    #[usage(
        long,
        short = 'E',
        help_heading = "Command",
        value_name = "KEY=VALUE",
        hide = true
    )]
    pub env: Vec<String>,

    /// How to wrap the task process: group, session, or none
    ///
    /// By default watchexec uses a session on macOS, a process group on other
    /// Unix systems, and a Job Object on Windows. Some programs need a session,
    /// and some do not work in a process group; `none` runs the command
    /// directly. On Windows, `group` and `session` both use a Job Object.
    #[usage(long, help_heading = "Command", value_name = "MODE", value_enum)]
    pub wrap_process: Option<WrapMode>,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(short = 'N', long, help_heading = "Output", hide = true)]
    pub notify: bool,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(
        long,
        help_heading = "Output",
        default = "auto",
        value_name = "MODE",
        alias = "colour",
        value_enum,
        hide = true
    )]
    pub color: ColourMode,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(long, help_heading = "Output", hide = true)]
    pub timings: bool,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(short, long, help_heading = "Output", hide = true)]
    pub quiet: bool,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(long, help_heading = "Output", hide = true)]
    pub bell: bool,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    ///
    /// mise sets the project origin itself when the tasks declare sources.
    #[usage(
        long,
        value_hint = usage_rs::ValueHint::DirPath,
        value_name = "DIRECTORY",
        hide = true,
    )]
    pub project_origin: Option<PathBuf>,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(
        long,
        value_hint = usage_rs::ValueHint::DirPath,
        value_name = "DIRECTORY",
        hide = true,
    )]
    pub workdir: Option<PathBuf>,

    /// Only react to files with these extensions, e.g. js,ts
    ///
    /// Give extensions with or without the leading dot. Repeat the flag or
    /// separate extensions with commas.
    #[usage(
        long = "exts",
        short = 'e',
        help_heading = "Filtering",
        delimiter = ',',
        value_name = "EXTENSIONS"
    )]
    pub filter_extensions: Vec<String>,

    /// Only react to files that match this glob
    ///
    /// Combined with the patterns mise derives from the tasks' `sources`.
    /// Repeat the flag for more patterns. Events that do not come from files,
    /// such as signals, pass through.
    #[usage(
        long = "filter",
        short = 'f',
        help_heading = "Filtering",
        value_name = "PATTERN"
    )]
    pub filter_patterns: Vec<String>,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(
        long = "filter-file",
        help_heading = "Filtering",
        value_hint = usage_rs::ValueHint::FilePath,
        value_name = "PATH",
        env = "WATCHEXEC_FILTER_FILES",
        hide_env = true,
        hide = true,
    )]
    #[cfg_attr(windows, usage(delimiter = ';'))]
    #[cfg_attr(not(windows), usage(delimiter = ':'))]
    pub filter_files: Vec<PathBuf>,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(
        long = "filter-prog",
        short = 'J',
        help_heading = "Filtering",
        value_name = "EXPRESSION",
        hide = true
    )]
    pub filter_programs: Vec<String>,

    /// Ignore files that match this glob
    ///
    /// Repeat the flag for more patterns. Events that do not come from files,
    /// such as signals, pass through.
    #[usage(
        long = "ignore",
        short = 'i',
        help_heading = "Filtering",
        value_name = "PATTERN"
    )]
    pub ignore_patterns: Vec<String>,

    /// Read ignore patterns from a file, one per line
    ///
    /// Empty lines and lines that start with `#` are skipped. The patterns use
    /// the same format as `--ignore`. Also read from `$WATCHEXEC_IGNORE_FILES`.
    #[usage(
        long = "ignore-file",
        help_heading = "Filtering",
        value_hint = usage_rs::ValueHint::FilePath,
        value_name = "PATH",
        env = "WATCHEXEC_IGNORE_FILES",
        hide_env = true,
    )]
    #[cfg_attr(windows, usage(delimiter = ';'))]
    #[cfg_attr(not(windows), usage(delimiter = ':'))]
    pub ignore_files: Vec<PathBuf>,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(
        long = "fs-events",
        help_heading = "Filtering",
        default = "create,remove,rename,modify,metadata",
        delimiter = ',',
        hide_default_value = true,
        value_enum,
        value_name = "EVENTS",
        hide = true
    )]
    pub filter_fs_events: Vec<FsEvent>,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(
        long = "no-meta",
        help_heading = "Filtering",
        conflicts = "filter_fs_events",
        hide = true
    )]
    pub filter_fs_meta: bool,

    /// Print the events that trigger each run
    ///
    /// Use it to check which files `--watch`, `--filter`, and the tasks'
    /// `sources` are reacting to.
    #[usage(long, help_heading = "Debugging")]
    pub print_events: bool,

    // Hidden because it is not forwarded to watchexec.
    /// Ignored: mise watch accepts this watchexec flag but does not pass it on
    #[usage(long, help_heading = "Debugging", hide = true)]
    pub manual: bool,
    // /// Change to this directory before executing the command
    // #[arg(short = 'C', long, value_hint = ValueHint::DirPath, long)]
    // pub cd: Option<PathBuf>,
    //
    // /// Don't actually run the task(s), just print them in order of execution
    // #[arg(long, short = 'n', verbatim_doc_comment)]
    // pub dry_run: bool,
    //
    // /// Force the tasks to run even if outputs are up to date
    // #[arg(long, short, verbatim_doc_comment)]
    // pub force: bool,
    //
    // /// Print stdout/stderr by line, prefixed with the tasks's label
    // /// Defaults to true if --jobs > 1
    // /// Configure with `task.output` config or `MISE_TASK_OUTPUT` env var
    // #[arg(long, short, verbatim_doc_comment, overrides_with = "interleave")]
    // pub prefix: bool,
    //
    // /// Print directly to stdout/stderr instead of by line
    // /// Defaults to true if --jobs == 1
    // /// Configure with `task.output` config or `MISE_TASK_OUTPUT` env var
    // #[arg(long, short, verbatim_doc_comment, overrides_with = "prefix")]
    // pub interleave: bool,
    //
    // /// Tool(s) to also add
    // /// e.g.: node@20 python@3.10
    // #[arg(short, long, value_name = "TOOL@VERSION")]
    // pub tool: Vec<ToolArg>,
    //
    // /// Number of tasks to run in parallel
    // /// [default: 4]
    // /// Configure with `jobs` config or `MISE_JOBS` env var
    // #[arg(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    // pub jobs: Option<usize>,
    //
    // /// Read/write directly to stdin/stdout/stderr instead of by line
    // /// Configure with `raw` config or `MISE_RAW` env var
    // #[arg(long, short, verbatim_doc_comment)]
    // pub raw: bool,
}

#[derive(Clone, Copy, Debug, Default, usage_rs::ValueEnum)]
pub(crate) enum EmitEvents {
    #[default]
    Environment,
    Stdio,
    File,
    JsonStdio,
    JsonFile,
    None,
}

#[derive(Clone, Copy, Debug, Default, usage_rs::ValueEnum, PartialEq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum OnBusyUpdate {
    #[default]
    Queue,
    DoNothing,
    Restart,
    Signal,
}

/// The `--wrap-process` arguments to pass on to watchexec, i.e. the user's choice or nothing.
///
/// The flag used to be parsed and then dropped entirely, so `--wrap-process none` had no effect
/// and a TUI task stayed stuck in a process group waiting on terminal I/O (#10212).
///
/// mise deliberately has no default of its own here. watchexec's `WRAP_DEFAULT` is
/// platform-dependent — `"session"` on macOS, `"group"` elsewhere — so any fixed default mise
/// picked would be wrong on some platform, both in `--help` and when deciding that a value is
/// "the same as the default" and can be dropped. Forwarding exactly what was asked for, and
/// nothing when nothing was asked for, leaves that choice where it belongs.
fn wrap_process_args(mode: Option<WrapMode>) -> Vec<String> {
    mode.map(|m| vec!["--wrap-process".to_string(), m.to_string()])
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, Default, usage_rs::ValueEnum, PartialEq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum WrapMode {
    #[default]
    Group,
    Session,
    None,
}

#[derive(Clone, Copy, Debug, Default, usage_rs::ValueEnum)]
pub(crate) enum ClearMode {
    #[default]
    Clear,
    Reset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, usage_rs::ValueEnum)]
pub(crate) enum FsEvent {
    Access,
    Create,
    Remove,
    Rename,
    Modify,
    Metadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, usage_rs::ValueEnum)]
pub(crate) enum ColourMode {
    Auto,
    Always,
    Never,
}
//endregion

/// Terminal attributes captured before watchexec runs, put back when this is
/// dropped — or when a signal kills the process before anything can be dropped.
///
/// `--clear=reset` leaves the terminal without echo when watchexec is
/// interrupted, and restoring on the straight-line path after the child exits
/// is not enough: mise exited the process from the signal path until 2026.7.16,
/// so the restore never ran, and `run_with_exit_signal` still drops the command
/// future when ctrl-c wins the race.
///
/// `Drop` covers the exits that unwind, which is every one of those. It does
/// not cover being killed. An earlier version of this comment claimed it
/// covered "every one of those exits" and that was wrong: a `mise watch` nested
/// under a `mise run` is killed by that run's `exit::kill_all()`, which sends
/// SIGTERM, and mise handles only SIGINT — so the process dies where it stands
/// and the terminal keeps whatever watchexec left in it. [`Self::arm`] closes
/// that by restoring from a signal thread before letting the signal finish the
/// job.
#[cfg(unix)]
struct TerminalState {
    saved: Vec<(std::os::fd::OwnedFd, nix::sys::termios::Termios)>,
}

#[cfg(unix)]
impl TerminalState {
    /// Capture whichever terminal watchexec is going to reset.
    ///
    /// That is the controlling terminal, not stdin: with stdin on `/dev/null`
    /// and stdout on a second terminal, the flags that change are still the
    /// controlling terminal's. So prefer `/dev/tty`, and fall back to the
    /// standard streams only for a session that has no controlling terminal to
    /// open.
    fn capture() -> Self {
        Self::controlling_terminal().unwrap_or_else(|| {
            use std::os::fd::AsFd;
            Self::capture_from([
                std::io::stdin().as_fd(),
                std::io::stdout().as_fd(),
                std::io::stderr().as_fd(),
            ])
        })
    }

    fn controlling_terminal() -> Option<Self> {
        use std::os::fd::AsFd;
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .ok()?;
        let state = Self::capture_from([tty.as_fd()]);
        (!state.saved.is_empty()).then_some(state)
    }

    /// Capture every descriptor that is a terminal, not just the first.
    ///
    /// Restoring one terminal twice is idempotent, so the common case where the
    /// standard streams share a terminal needs no deduplication, and streams
    /// pointing at different terminals are all put back.
    fn capture_from<'a>(fds: impl IntoIterator<Item = std::os::fd::BorrowedFd<'a>>) -> Self {
        let saved = fds
            .into_iter()
            .filter_map(|fd| {
                let attrs = nix::sys::termios::tcgetattr(fd).ok()?;
                // Duplicate the descriptor: the borrow ends with this call, but
                // the restore happens later, whenever the guard is dropped.
                Some((fd.try_clone_to_owned().ok()?, attrs))
            })
            .collect();
        Self { saved }
    }
}

#[cfg(unix)]
impl TerminalState {
    /// Also restore if a signal kills the process, where `Drop` never runs.
    ///
    /// Only the signals whose default action is to terminate are taken. SIGINT
    /// is deliberately left alone: `tokio::signal::ctrl_c` already owns it and
    /// that path unwinds, so `Drop` restores and a second registration would
    /// only make the ordering harder to reason about.
    ///
    /// This uses `signal_hook`'s thread rather than a raw handler, matching
    /// `CmdLineRunner`. Registering replaces the default action, so the process
    /// no longer dies where it stands and the thread has time to put the
    /// terminal back. It then hands the signal its original meaning with
    /// `emulate_default_handler`, so the exit status still says the process was
    /// killed by that signal rather than reporting some invented code.
    #[must_use = "the guard disarms the signal thread when dropped"]
    fn arm(&self) -> SignalRestore {
        use signal_hook::consts::{SIGHUP, SIGQUIT, SIGTERM};

        let saved = self
            .saved
            .iter()
            .filter_map(|(fd, attrs)| Some((fd.try_clone().ok()?, attrs.clone())))
            .collect::<Vec<_>>();
        if saved.is_empty() {
            return SignalRestore { handle: None };
        }
        let mut signals = match signal_hook::iterator::Signals::new([SIGTERM, SIGHUP, SIGQUIT]) {
            Ok(signals) => signals,
            Err(err) => {
                // Worth saying out loud: the terminal is now only as safe as the
                // unwinding paths, which is the state this exists to improve on.
                debug!("could not watch for signals to restore the terminal: {err}");
                return SignalRestore { handle: None };
            }
        };
        let handle = signals.handle();
        std::thread::spawn(move || {
            let Some(signal) = signals.forever().next() else {
                // `handle.close()` ended the iterator: the command finished and
                // `Drop` is doing the restore instead.
                return;
            };
            for (fd, attrs) in &saved {
                let _ = nix::sys::termios::tcsetattr(fd, nix::sys::termios::SetArg::TCSANOW, attrs);
            }
            let _ = signal_hook::low_level::emulate_default_handler(signal);
        });
        SignalRestore {
            handle: Some(handle),
        }
    }
}

/// Stops the signal thread armed by [`TerminalState::arm`].
#[cfg(unix)]
struct SignalRestore {
    handle: Option<signal_hook::iterator::Handle>,
}

#[cfg(unix)]
impl Drop for SignalRestore {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.close();
        }
    }
}

#[cfg(unix)]
impl Drop for TerminalState {
    fn drop(&mut self) {
        for (fd, attrs) in &self.saved {
            let _ = nix::sys::termios::tcsetattr(fd, nix::sys::termios::SetArg::TCSANOW, attrs);
        }
    }
}

#[cfg(all(test, unix))]
mod terminal_state_tests {
    use super::TerminalState;
    use nix::sys::termios::{LocalFlags, SetArg, tcgetattr, tcsetattr};
    use std::os::fd::{AsFd, BorrowedFd};

    fn echo_is_on(fd: BorrowedFd<'_>) -> bool {
        tcgetattr(fd)
            .unwrap()
            .local_flags
            .contains(LocalFlags::ECHO)
    }

    fn set_echo(fd: BorrowedFd<'_>, on: bool) {
        let mut attrs = tcgetattr(fd).unwrap();
        attrs.local_flags.set(LocalFlags::ECHO, on);
        tcsetattr(fd, SetArg::TCSANOW, &attrs).unwrap();
    }

    #[test]
    fn capture_from_saves_nothing_when_no_descriptor_is_a_terminal() {
        let (r, w) = nix::unistd::pipe().unwrap();
        assert!(
            TerminalState::capture_from([r.as_fd(), w.as_fd()])
                .saved
                .is_empty()
        );
    }

    /// With nothing captured there is nothing to put back, so the signals are
    /// left with their default meaning rather than being taken over for a
    /// restore that would do nothing.
    #[test]
    fn arming_an_empty_capture_registers_nothing() {
        let (r, w) = nix::unistd::pipe().unwrap();
        let state = TerminalState::capture_from([r.as_fd(), w.as_fd()]);
        assert!(state.arm().handle.is_none());
    }

    /// Arming duplicates the descriptors rather than borrowing them, so the
    /// guard is still able to restore after the signal thread has its own copy.
    #[test]
    fn arming_leaves_the_guard_able_to_restore() {
        let pty = nix::pty::openpty(None, None).unwrap();
        let state = TerminalState::capture_from([pty.master.as_fd()]);
        assert_eq!(state.saved.len(), 1);
        let armed = state.arm();
        assert!(armed.handle.is_some());
        set_echo(pty.master.as_fd(), false);
        assert!(!echo_is_on(pty.master.as_fd()));
        drop(armed);
        drop(state);
        assert!(echo_is_on(pty.master.as_fd()));
    }

    #[test]
    fn capture_from_looks_past_a_non_terminal_descriptor() {
        // A redirected stdin must not stop the search: the process still shares
        // a terminal that watchexec can reset (#8269).
        let pty = nix::pty::openpty(None, None).unwrap();
        let (r, _w) = nix::unistd::pipe().unwrap();
        let state = TerminalState::capture_from([r.as_fd(), pty.master.as_fd()]);
        assert_eq!(state.saved.len(), 1);
    }

    #[test]
    fn dropping_restores_flags_cleared_while_it_was_held() {
        let pty = nix::pty::openpty(None, None).unwrap();
        let fd = pty.master.as_fd();
        set_echo(fd, true);

        let guard = TerminalState::capture_from([fd]);

        // Stand in for watchexec's --clear=reset, which clears ECHO and does
        // not put it back when it is interrupted.
        set_echo(fd, false);
        assert!(!echo_is_on(fd));

        drop(guard);
        assert!(echo_is_on(fd));
    }

    #[test]
    fn dropping_restores_every_captured_terminal() {
        let first = nix::pty::openpty(None, None).unwrap();
        let second = nix::pty::openpty(None, None).unwrap();
        set_echo(first.master.as_fd(), true);
        set_echo(second.master.as_fd(), true);

        let guard = TerminalState::capture_from([first.master.as_fd(), second.master.as_fd()]);

        set_echo(first.master.as_fd(), false);
        set_echo(second.master.as_fd(), false);

        drop(guard);
        assert!(echo_is_on(first.master.as_fd()));
        assert!(echo_is_on(second.master.as_fd()));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WrapMode, common_ancestor, merge_watch_patterns, normalize_path, parse_source,
        relativize_source, source_watch_dir, tasks_disable_vcs_ignores, wrap_process_args,
    };
    use crate::cli::{Cli, Commands};
    use crate::task::{Task, TaskWatchOptions};
    use std::path::{Path, PathBuf};

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn resolve_source(s: &str, cwd: &Path, anchor: &Path) -> String {
        let (k, abs) = parse_source(s, cwd);
        relativize_source(k, &abs, anchor)
    }

    #[test]
    fn wrap_mode_serializes_to_watchexec_values() {
        // These strings are forwarded verbatim to `watchexec --wrap-process`, so
        // they must match the values watchexec accepts (#10212).
        assert_eq!(WrapMode::Group.to_string(), "group");
        assert_eq!(WrapMode::Session.to_string(), "session");
        assert_eq!(WrapMode::None.to_string(), "none");
    }

    #[test]
    fn watch_flags_after_the_task_still_belong_to_watch() {
        let argv =
            ["mise", "watch", "default", "--postpone", "--poll=100ms"].map(std::ffi::OsStr::new);
        let cli = Cli::parse_from_argv(&argv).unwrap();
        let Some(Commands::Watch(watch)) = cli.command else {
            panic!("expected the watch command");
        };

        assert_eq!(watch.task.as_deref(), Some("default"));
        assert!(watch.args.is_empty());
        assert!(watch.watchexec.postpone);
        assert_eq!(watch.watchexec.poll.as_deref(), Some("100ms"));
    }

    #[test]
    fn wrap_process_is_forwarded_verbatim_when_given() {
        // Every mode reaches watchexec, including `group`. The previous `!= WrapMode::Group`
        // check dropped an explicit `group`, which on macOS left the command running under
        // watchexec's own default of `session` instead.
        assert_eq!(
            wrap_process_args(Some(WrapMode::Group)),
            s(&["--wrap-process", "group"])
        );
        assert_eq!(
            wrap_process_args(Some(WrapMode::Session)),
            s(&["--wrap-process", "session"])
        );
        assert_eq!(
            wrap_process_args(Some(WrapMode::None)),
            s(&["--wrap-process", "none"])
        );
    }

    #[test]
    fn wrap_process_is_omitted_when_not_given() {
        // Nothing is forwarded, so watchexec applies its own platform-dependent default
        // (`session` on macOS, `group` elsewhere) rather than one mise guessed.
        assert!(wrap_process_args(None).is_empty());
    }

    #[test]
    fn task_watch_options_disable_vcs_ignores_for_combined_watch() {
        let default_task = Task::default();
        let mut opted_in_task = Task::default();
        opted_in_task.watch = Some(TaskWatchOptions {
            no_vcs_ignore: true,
        });

        assert!(!tasks_disable_vcs_ignores(std::slice::from_ref(
            &default_task,
        )));
        assert!(tasks_disable_vcs_ignores(&[default_task, opted_in_task]));
    }

    #[test]
    fn merge_single_task_splits_pos_and_neg() {
        let task = s(&["src/**/*.ts", "!src/**/*.test.ts"]);
        let (inc, exc) = merge_watch_patterns(std::iter::once(task.as_slice()));
        assert_eq!(inc, vec!["src/**/*.ts"]);
        assert_eq!(exc, vec!["src/**/*.test.ts"]);
    }

    #[test]
    fn merge_unescapes_literal_bang() {
        let task = s(&["\\!keep.txt"]);
        let (inc, exc) = merge_watch_patterns(std::iter::once(task.as_slice()));
        assert_eq!(inc, vec!["!keep.txt"]);
        assert!(exc.is_empty());
    }

    #[test]
    fn merge_dedupes_across_tasks() {
        let a = s(&["src/**/*.ts", "!src/**/*.test.ts"]);
        let b = s(&["src/**/*.ts", "!src/**/*.test.ts"]);
        let (inc, exc) = merge_watch_patterns([a.as_slice(), b.as_slice()]);
        assert_eq!(inc, vec!["src/**/*.ts"]);
        assert_eq!(exc, vec!["src/**/*.test.ts"]);
    }

    /// Regression: one task's `!pat` must not suppress another task's
    /// positive `pat`. Watchexec's `--ignore` runs after `--filter`, so a
    /// pattern that is positively wanted by any task is removed from the
    /// global ignore list.
    #[test]
    fn merge_does_not_let_one_task_exclude_anothers_include() {
        let a = s(&["src/**/*.ts", "!src/**/*.test.ts"]);
        let b = s(&["src/**/*.test.ts"]);
        let (inc, exc) = merge_watch_patterns([a.as_slice(), b.as_slice()]);
        assert!(inc.contains(&"src/**/*.ts".to_string()));
        assert!(inc.contains(&"src/**/*.test.ts".to_string()));
        // `src/**/*.test.ts` is positively included by task B, so it must not
        // appear in the ignore list — even though task A asks to exclude it.
        assert!(
            !exc.contains(&"src/**/*.test.ts".to_string()),
            "exc should not contain a pattern that any task positively includes; got {exc:?}",
        );
    }

    #[test]
    fn resolve_preserves_literal_bang_escape_at_anchor() {
        let anchor = Path::new("/repo");
        assert_eq!(resolve_source("\\!keep.txt", anchor, anchor), "\\!keep.txt");
    }

    #[test]
    fn resolve_drops_literal_bang_escape_when_no_longer_ambiguous() {
        let anchor = Path::new("/repo");
        let cwd = Path::new("/repo/packages/foo");
        assert_eq!(
            resolve_source("\\!keep.txt", cwd, anchor),
            "packages/foo/!keep.txt",
        );
    }

    #[test]
    fn resolve_escapes_plain_source_relativized_to_leading_bang() {
        let cwd = Path::new("/repo");
        let anchor = Path::new("/repo/sub");
        assert_eq!(resolve_source("sub/!gen/*.ts", cwd, anchor), "\\!gen/*.ts");
    }

    fn pb(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn common_ancestor_of_siblings_is_parent() {
        let got = common_ancestor([pb("/repo/packages/foo"), pb("/repo/packages/bar")]);
        assert_eq!(got, Some(pb("/repo/packages")));
    }

    #[test]
    fn common_ancestor_of_nested_is_shorter() {
        let got = common_ancestor([pb("/repo/a"), pb("/repo/a/b/c")]);
        assert_eq!(got, Some(pb("/repo/a")));
    }

    #[test]
    fn common_ancestor_of_disjoint_is_root() {
        let got = common_ancestor([pb("/x/a"), pb("/y/b")]);
        assert_eq!(got, Some(pb("/")));
    }

    #[test]
    fn normalize_resolves_parent_components() {
        assert_eq!(
            normalize_path(Path::new("/repo/pkg/foo/../../shared/src")),
            pb("/repo/shared/src"),
        );
    }

    #[test]
    fn parse_source_absolutizes_relative_with_parent_escape() {
        let cwd = Path::new("/repo/packages/foo");
        let (_, abs) = parse_source("../../shared/src/*.ts", cwd);
        assert_eq!(abs, pb("/repo/shared/src/*.ts"));
    }

    #[test]
    fn anchor_widens_to_cover_source_escaping_cwd() {
        let cwd = pb("/repo/packages/foo");
        let (_, abs) = parse_source("../../shared/src/*.ts", &cwd);
        let common = common_ancestor([cwd.as_path(), abs.as_path()]);
        assert_eq!(common, Some(pb("/repo")));
        let rel = relativize_source(super::SourceKind::Plain, &abs, &common.unwrap());
        assert_eq!(rel, "shared/src/*.ts");
    }

    #[test]
    fn source_watch_dir_stops_at_first_glob() {
        assert_eq!(
            source_watch_dir(Path::new("/root/shared/src/*.ts")),
            pb("/root/shared/src"),
        );
    }

    #[test]
    fn source_watch_dir_stops_at_double_star() {
        assert_eq!(
            source_watch_dir(Path::new("/root/shared/**/*.ts")),
            pb("/root/shared"),
        );
    }

    #[test]
    fn source_watch_dir_of_literal_file_is_parent() {
        assert_eq!(
            source_watch_dir(Path::new("/root/shared/src/index.ts")),
            pb("/root/shared/src"),
        );
    }

    #[test]
    fn common_ancestor_empty_is_none() {
        let got = common_ancestor(std::iter::empty::<PathBuf>());
        assert_eq!(got, None);
    }
}
