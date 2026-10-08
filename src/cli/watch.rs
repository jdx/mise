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
use eyre::bail;
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
    #[usage(short, long)]
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
        if self.watchexec.workdir.is_some() {
            warn!("--workdir has no effect: mise runs each task from its own dir");
        }
        let config = Config::get().await?;
        let ts = ToolsetBuilder::new().build(&config).await?;
        if let Err(err) = which::which("watchexec") {
            let watchexec: BackendArg = "watchexec".into();
            if !ts.versions.contains_key(&watchexec) {
                eprintln!("{}: {}", style("Error").red().bold(), err);
                eprintln!("{}: Install watchexec with:", style("Hint").bold());
                eprintln!("  mise use -g watchexec");
                return Err(request_exit(1));
            }
        }
        // watchexec refuses `--manual` alongside a command, and showing its manual page needs no
        // tasks, so this resolves none and runs nothing but `watchexec --manual`.
        if self.watchexec.manual {
            debug!("$ watchexec --manual");
            let mut cmd = cmd::cmd("watchexec", ["--manual"]);
            for (k, v) in ts.env_with_path(&config).await? {
                cmd = cmd.env(k, v);
            }
            cmd.run()?;
            return Ok(());
        }
        let mut args = once(self.task.clone())
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
        let mut args = self
            .watchexec
            .watchexec_args(tasks_disable_vcs_ignores(&watched_tasks));
        // A --project-origin the user gave wins over the one mise derives
        // below. It is made absolute here, against the directory watchexec
        // will run in, so the task sources can be made relative to it.
        let project_origin = self.watchexec.project_origin.as_ref().map(|p| {
            env::current_dir()
                .map(|cwd| normalize_path(&cwd.join(p)))
                .unwrap_or_else(|_| p.clone())
        });
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
            let mut parsed: Vec<Vec<(SourceKind, PathBuf)>> = task_cwds
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
                let anchor: PathBuf = match (project_origin.clone(), configured, common) {
                    (Some(origin), _, _) => {
                        // watchexec drops a filter outside the origin, so a
                        // source there would never trigger the task.
                        if let Some((_, outside)) = parsed.iter().flatten().find(|(kind, p)| {
                            !matches!(kind, SourceKind::Negation) && !p.starts_with(&origin)
                        }) {
                            bail!(
                                "--project-origin {} does not contain the watched source {}; \
                                 use a directory that contains every task's sources",
                                origin.display(),
                                outside.display()
                            );
                        }
                        // An exclusion such as `!**/*.tmp` starts above the
                        // origin but still matches inside it, so it is
                        // rebased onto the origin; one that cannot match
                        // inside the origin excludes nothing there and is
                        // dropped.
                        for sources in &mut parsed {
                            sources.retain_mut(|(kind, p)| {
                                if !matches!(kind, SourceKind::Negation) {
                                    return true;
                                }
                                match negation_within_origin(p, &origin) {
                                    Some(rebased) => {
                                        *p = rebased;
                                        true
                                    }
                                    None => {
                                        debug!(
                                            "dropping exclusion {} outside --project-origin {}",
                                            p.display(),
                                            origin.display()
                                        );
                                        false
                                    }
                                }
                            });
                        }
                        origin
                    }
                    (None, Some(mut cfg), Some(common)) => {
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
                    (None, Some(cfg), None) => cfg,
                    (None, None, Some(common)) => common,
                    (None, None, None) => dirs::CWD.clone().unwrap_or_default(),
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
        if let Some(anchor) = filter_anchor.as_ref().or(project_origin.as_ref()) {
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
        args.extend(self.command_args(&tasks));
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

    /// The command watchexec runs on each change: `-- <mise> run [--skip-deps] <tasks>`.
    ///
    /// With `--only-emit-events` there is none, because watchexec refuses that flag alongside a
    /// command; it then only prints the events that the task sources would have triggered on.
    fn command_args(&self, tasks: &[Task]) -> Vec<String> {
        if self.watchexec.only_emit_events {
            return vec![];
        }
        let mut args = vec![
            "--".to_string(),
            env::MISE_BIN.to_string_lossy().to_string(),
            "run".to_string(),
        ];
        if self.skip_deps {
            args.push("--skip-deps".to_string());
        }
        args.extend(
            itertools::intersperse(
                tasks.iter().map(|t| {
                    let mut args = vec![t.name.to_string()];
                    args.extend(t.args.iter().map(|a| a.to_string()));
                    args
                }),
                vec![":::".to_string()],
            )
            .flatten(),
        );
        args
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

/// Express an absolute exclusion pattern as one under `origin` that excludes the same files
/// inside it, or `None` when it cannot match anything inside `origin`.
///
/// A pattern under `origin` is kept as is. One whose literal (glob-free) prefix is an ancestor of
/// `origin` and whose next component is `**`, such as `<task cwd>/**/*.tmp`, matches at any depth
/// below that prefix, so `<origin>/**/*.tmp` excludes the same files inside `origin`.
fn negation_within_origin(absolute: &Path, origin: &Path) -> Option<PathBuf> {
    use std::path::Component;
    if absolute.starts_with(origin) {
        return Some(absolute.to_path_buf());
    }
    let is_glob = |s: &std::ffi::OsStr| s.to_string_lossy().contains(['*', '?', '[', '{']);
    let mut prefix = PathBuf::new();
    let mut components = absolute.components();
    for c in components.by_ref() {
        match c {
            Component::Normal(part) if is_glob(part) => {
                if part != "**" || !origin.starts_with(&prefix) {
                    return None;
                }
                return Some(origin.join(part).join(components.as_path()));
            }
            _ => prefix.push(c.as_os_str()),
        }
    }
    None
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
    #[usage(long, help_heading = "Command", value_name = "SHELL")]
    pub shell: Option<String>,

    /// Shorthand for '--shell=none'
    #[usage(short = 'n', help_heading = "Command")]
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
    ///         "source": "filesystem"
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
    #[usage(
        long,
        help_heading = "Command",
        verbatim_doc_comment,
        default = "none",
        hide_default_value = true,
        value_name = "MODE",
        required_if_eq("only_emit_events", "true"),
        value_enum
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
    #[usage(
		long,
		help_heading = "Output",
		conflicts = ["manual"],
    )]
    pub only_emit_events: bool,

    /// Add env vars to the command
    ///
    /// This is a convenience option for setting environment variables for the command, without
    /// setting them for the Watchexec process itself.
    ///
    /// Use key=value syntax. Multiple variables can be set by repeating the option.
    ///
    /// This is watchexec's '--env'. In mise, '-E'/'--env' selects the mise environment
    /// (`mise.<ENV>.toml`), as it does for every other command.
    #[usage(
        long = "watchexec-env",
        help_heading = "Command",
        value_name = "KEY=VALUE"
    )]
    pub command_env: Vec<String>,

    /// How to wrap the task process: group, session, or none
    ///
    /// By default watchexec uses a session on macOS, a process group on other
    /// Unix systems, and a Job Object on Windows. Some programs need a session,
    /// and some do not work in a process group; `none` runs the command
    /// directly. On Windows, `group` and `session` both use a Job Object.
    #[usage(long, help_heading = "Command", value_name = "MODE", value_enum)]
    pub wrap_process: Option<WrapMode>,

    /// Alert when commands start and end
    ///
    /// With this, Watchexec will emit a desktop notification when a command starts and ends, on
    /// supported platforms. On unsupported platforms, it may silently do nothing, or log a warning.
    #[usage(short = 'N', long, help_heading = "Output")]
    pub notify: bool,

    /// When to use terminal colours
    ///
    /// Setting the environment variable `NO_COLOR` to any value is equivalent to `--color=never`.
    #[usage(
        long,
        help_heading = "Output",
        default = "auto",
        value_name = "MODE",
        alias = "colour",
        value_enum
    )]
    pub color: ColourMode,

    /// Print how long the command took to run
    ///
    /// This may not be exactly accurate, as it includes some overhead from Watchexec itself. Use
    /// the `time` utility, high-precision timers, or benchmarking tools for more accurate results.
    #[usage(long, help_heading = "Output")]
    pub timings: bool,

    /// Don't print starting and stopping messages
    ///
    /// By default Watchexec will print a message when the command starts and stops. This option
    /// disables this behaviour, so only the command's output, warnings, and errors will be printed.
    ///
    /// This is watchexec's '--quiet'. In mise, '-q'/'--quiet' quiets mise's own messages, as it
    /// does for every other command.
    #[usage(long = "watchexec-quiet", help_heading = "Output")]
    pub watchexec_quiet: bool,

    /// Ring the terminal bell on command completion
    #[usage(long, help_heading = "Output")]
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
    ///
    /// The directory must contain every watched task's sources, which mise makes relative to it.
    #[usage(
		long,
		value_hint = usage_rs::ValueHint::DirPath,
		value_name = "DIRECTORY",
    )]
    pub project_origin: Option<PathBuf>,

    /// Has no effect: mise runs each task from its own `dir`
    ///
    /// Accepted so scripts that pass it keep working. Forwarding it would make the nested
    /// `mise run` look for tasks in that directory instead of this project.
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

    /// Files to load filters from
    ///
    /// Provide a path to a file containing filters, one per line. Empty lines and lines starting
    /// with '#' are ignored. Uses the same pattern format as the '--filter' option.
    ///
    /// This can also be used via the $WATCHEXEC_FILTER_FILES environment variable.
    #[usage(
		long = "filter-file",
		help_heading = "Filtering",
		value_hint = usage_rs::ValueHint::FilePath,
		value_name = "PATH",
		env = "WATCHEXEC_FILTER_FILES",
		hide_env = true,
    )]
    #[cfg_attr(windows, usage(delimiter = ';'))]
    #[cfg_attr(not(windows), usage(delimiter = ':'))]
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
    ///   'any(.tags[] | select(.kind == "path" and .filetype == "file"); .absolute | file_meta | .executable)'
    ///
    /// Ignore files that start with shebangs:
    ///
    ///   'any(.tags[] | select(.kind == "path" and .filetype == "file"); .absolute | file_read(2) == "#!") | not'
    #[usage(
        long = "filter-prog",
        short = 'J',
        help_heading = "Filtering",
        value_name = "EXPRESSION",
        verbatim_doc_comment
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

    /// Filesystem events to filter to
    ///
    /// This is a quick filter to only emit events for the given types of filesystem changes. Choose
    /// from 'access', 'create', 'remove', 'rename', 'modify', 'metadata'. Multiple types can be
    /// given by repeating the option or by separating them with commas. By default, this is all
    /// types except for 'access'.
    ///
    /// This may apply filtering at the kernel level when possible, which can be more efficient, but
    /// may be more confusing when reading the logs.
    // The default must match DEFAULT_FS_EVENTS, which decides whether to forward this flag.
    #[usage(
        long = "fs-events",
        help_heading = "Filtering",
        default = "create,remove,rename,modify,metadata",
        delimiter = ',',
        hide_default_value = true,
        value_enum,
        value_name = "EVENTS"
    )]
    pub filter_fs_events: Vec<FsEvent>,

    /// Don't emit fs events for metadata changes
    ///
    /// This is a shorthand for '--fs-events create,remove,rename,modify'. Using it alongside the
    /// '--fs-events' option is non-sensical and not allowed.
    #[usage(
        long = "no-meta",
        help_heading = "Filtering",
        conflicts = "filter_fs_events"
    )]
    pub filter_fs_meta: bool,

    /// Print the events that trigger each run
    ///
    /// Use it to check which files `--watch`, `--filter`, and the tasks'
    /// `sources` are reacting to.
    #[usage(long, help_heading = "Debugging")]
    pub print_events: bool,

    /// Show the manual page
    ///
    /// This shows the manual page for Watchexec, if the output is a terminal and the 'man' program
    /// is available. If not, the manual page is printed to stdout in ROFF format (suitable for
    /// writing to a watchexec.1 file).
    #[usage(long, help_heading = "Debugging")]
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

impl WatchexecArgs {
    /// The watchexec flags these arguments ask for, spelled the way watchexec takes them.
    ///
    /// Every flag `mise watch` accepts on watchexec's behalf has to be re-emitted here: one
    /// that is parsed and not pushed is silently ignored (#7776, #10212). Two are handled in
    /// `run` instead: `--project-origin`, because mise also derives an origin from the task
    /// sources and has to pick one, and `--manual`, because watchexec refuses it alongside the
    /// command mise always runs.
    ///
    /// `no_vcs_ignore` is forced on when a watched task opted out of VCS ignores.
    fn watchexec_args(&self, no_vcs_ignore: bool) -> Vec<String> {
        let mut args = vec![];
        if let Some(delay_run) = &self.delay_run {
            args.push("--delay-run".to_string());
            args.push(delay_run.clone());
        }
        if let Some(poll) = &self.poll {
            args.push("--poll".to_string());
            args.push(poll.clone());
        }
        if let Some(signal) = &self.signal {
            args.push("--signal".to_string());
            args.push(signal.clone());
        }
        if let Some(stop_signal) = &self.stop_signal {
            args.push("--stop-signal".to_string());
            args.push(stop_signal.clone());
        }
        if self.stop_timeout != "10s" {
            args.push("--stop-timeout".to_string());
            args.push(self.stop_timeout.clone());
        }
        if self.debounce != "50ms" {
            args.push("--debounce".to_string());
            args.push(self.debounce.clone());
        }
        if self.stdin_quit {
            args.push("--stdin-quit".to_string());
        }
        if self.no_vcs_ignore || no_vcs_ignore {
            args.push("--no-vcs-ignore".to_string());
        }
        if self.no_project_ignore {
            args.push("--no-project-ignore".to_string());
        }
        if self.no_global_ignore {
            args.push("--no-global-ignore".to_string());
        }
        if self.no_default_ignore {
            args.push("--no-default-ignore".to_string());
        }
        if self.no_discover_ignore {
            args.push("--no-discover-ignore".to_string());
        }
        if self.ignore_nothing {
            args.push("--ignore-nothing".to_string());
        }
        if self.postpone {
            args.push("--postpone".to_string());
        }
        if let Some(screen_clear) = self.screen_clear {
            args.push("--clear".to_string());
            if let ClearMode::Reset = screen_clear {
                args.push("reset".to_string());
            }
        }
        if self.restart {
            args.push("--restart".to_string());
        }
        if self.on_busy_update != OnBusyUpdate::DoNothing {
            args.push("--on-busy-update".to_string());
            args.push(self.on_busy_update.to_string());
        }
        args.extend(wrap_process_args(self.wrap_process));
        for signal_map in &self.signal_map {
            args.push("--map-signal".to_string());
            args.push(signal_map.to_string());
        }
        for path in &self.recursive_paths {
            args.push("--watch".to_string());
            args.push(path.to_string_lossy().to_string());
        }
        for path in &self.non_recursive_paths {
            args.push("--watch-non-recursive".to_string());
            args.push(path.to_string_lossy().to_string());
        }
        for ext in &self.filter_extensions {
            args.push("--exts".to_string());
            args.push(ext.to_string());
        }
        for pattern in &self.filter_patterns {
            args.push("--filter".to_string());
            args.push(pattern.to_string());
        }
        if let Some(watch_file) = &self.watch_file {
            args.push("--watch-file".to_string());
            args.push(watch_file.to_string_lossy().to_string());
        }
        // watchexec unions repeated --ignore flags, so these combine with the
        // source-derived ignores `run` pushes rather than clobbering them (#7776).
        for pattern in &self.ignore_patterns {
            args.push("--ignore".to_string());
            args.push(pattern.to_string());
        }
        for path in &self.ignore_files {
            args.push("--ignore-file".to_string());
            args.push(path.to_string_lossy().to_string());
        }
        if self.print_events {
            args.push("--print-events".to_string());
        }
        if let Some(shell) = &self.shell {
            args.push("--shell".to_string());
            args.push(shell.clone());
        }
        if self.no_shell {
            args.push("-n".to_string());
        }
        if self.emit_events_to != EmitEvents::None {
            args.push("--emit-events-to".to_string());
            args.push(self.emit_events_to.to_string());
        }
        if self.only_emit_events {
            args.push("--only-emit-events".to_string());
        }
        for env in &self.command_env {
            args.push("--env".to_string());
            args.push(env.clone());
        }
        if self.notify {
            args.push("--notify".to_string());
        }
        if self.color != ColourMode::Auto {
            args.push("--color".to_string());
            args.push(self.color.to_string());
        }
        if self.timings {
            args.push("--timings".to_string());
        }
        if self.watchexec_quiet {
            args.push("--quiet".to_string());
        }
        if self.bell {
            args.push("--bell".to_string());
        }
        for path in &self.filter_files {
            args.push("--filter-file".to_string());
            args.push(path.to_string_lossy().to_string());
        }
        for program in &self.filter_programs {
            args.push("--filter-prog".to_string());
            args.push(program.clone());
        }
        if self.filter_fs_events != DEFAULT_FS_EVENTS {
            args.push("--fs-events".to_string());
            args.push(self.filter_fs_events.iter().join(","));
        }
        if self.filter_fs_meta {
            args.push("--no-meta".to_string());
        }
        args
    }
}

/// watchexec's own `--fs-events` default. It must match the `default` on `filter_fs_events`;
/// if the two drift apart, mise forwards `--fs-events` when the user did not pass it.
const DEFAULT_FS_EVENTS: &[FsEvent] = &[
    FsEvent::Create,
    FsEvent::Remove,
    FsEvent::Rename,
    FsEvent::Modify,
    FsEvent::Metadata,
];

#[derive(Clone, Copy, Debug, Default, usage_rs::ValueEnum, PartialEq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, usage_rs::ValueEnum, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum FsEvent {
    Access,
    Create,
    Remove,
    Rename,
    Modify,
    Metadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, usage_rs::ValueEnum, strum::Display)]
#[strum(serialize_all = "kebab-case")]
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
        WrapMode, common_ancestor, merge_watch_patterns, negation_within_origin, normalize_path,
        parse_source, relativize_source, source_watch_dir, tasks_disable_vcs_ignores,
        wrap_process_args,
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

    fn parse_watch(argv: &[&str]) -> (Cli, super::Watch) {
        let argv = argv.iter().map(std::ffi::OsStr::new).collect::<Vec<_>>();
        let mut cli = Cli::parse_from_argv(&argv).unwrap();
        let Some(Commands::Watch(watch)) = cli.command.take() else {
            panic!("expected the watch command");
        };
        (cli, *watch)
    }

    #[test]
    fn defaults_forward_no_watchexec_flags() {
        let (_, watch) = parse_watch(&["mise", "watch", "build"]);
        assert!(watch.watchexec.watchexec_args(false).is_empty());
    }

    /// These flags used to be parsed and then dropped, so watchexec never saw them.
    #[test]
    fn forwards_previously_dropped_watchexec_flags() {
        let (_, watch) = parse_watch(&[
            "mise",
            "watch",
            "--shell",
            "bash",
            "-n",
            "--emit-events-to",
            "json-stdio",
            "--watchexec-env",
            "FOO=bar",
            "--watchexec-env",
            "BAZ=qux",
            "--notify",
            "--color",
            "never",
            "--timings",
            "--watchexec-quiet",
            "--bell",
            "--workdir",
            "sub",
            "--filter-file",
            "filters.txt",
            "-J",
            "true",
            "--fs-events",
            "create,modify",
            "build",
        ]);
        assert_eq!(
            watch.watchexec.watchexec_args(false),
            s(&[
                "--shell",
                "bash",
                "-n",
                "--emit-events-to",
                "json-stdio",
                "--env",
                "FOO=bar",
                "--env",
                "BAZ=qux",
                "--notify",
                "--color",
                "never",
                "--timings",
                "--quiet",
                "--bell",
                "--filter-file",
                "filters.txt",
                "--filter-prog",
                "true",
                "--fs-events",
                "create,modify",
            ])
        );
    }

    /// `--no-meta` conflicts with `--fs-events` in the test above.
    #[test]
    fn forwards_no_meta() {
        let (_, watch) = parse_watch(&["mise", "watch", "--no-meta", "build"]);
        assert_eq!(watch.watchexec.watchexec_args(false), s(&["--no-meta"]));
    }

    /// `run` handles `--manual` on its own, as `watchexec --manual` with no command, because
    /// watchexec refuses `--manual` alongside one.
    #[test]
    fn manual_is_not_forwarded_with_the_other_flags() {
        let (_, watch) = parse_watch(&["mise", "watch", "--manual"]);
        assert!(watch.watchexec.manual);
        assert!(watch.watchexec.watchexec_args(false).is_empty());
    }

    fn named_task(name: &str) -> Task {
        let mut task = Task::default();
        task.name = name.to_string();
        task
    }

    #[test]
    fn command_runs_the_tasks() {
        let (_, watch) = parse_watch(&["mise", "watch", "--skip-deps", "build"]);
        let args = watch.command_args(&[named_task("build"), named_task("test")]);
        assert_eq!(args[0], "--");
        assert_eq!(
            args[2..],
            s(&["run", "--skip-deps", "build", ":::", "test"])
        );
    }

    /// watchexec refuses `--only-emit-events` alongside a command, so mise must not append
    /// `-- <mise> run ...` after it.
    #[test]
    fn only_emit_events_runs_no_command() {
        let (_, watch) = parse_watch(&[
            "mise",
            "watch",
            "--emit-events-to",
            "json-stdio",
            "--only-emit-events",
            "build",
        ]);
        assert_eq!(
            watch.watchexec.watchexec_args(false),
            s(&["--emit-events-to", "json-stdio", "--only-emit-events"])
        );
        assert!(watch.command_args(&[named_task("build")]).is_empty());
    }

    /// `-E`/`--env` and `-q`/`--quiet` are mise's global flags. watchexec's flags of the same
    /// name used to shadow them after `watch`, and are now spelled `--watchexec-env` and
    /// `--watchexec-quiet`, so `mise watch -E dev` must not turn into `watchexec --env dev`.
    #[test]
    fn env_and_quiet_after_watch_are_mises_own() {
        let (cli, watch) = parse_watch(&["mise", "watch", "-E", "dev", "-q", "build"]);
        assert_eq!(cli.env, Some(vec!["dev".to_string()]));
        assert!(cli.quiet);
        assert!(watch.watchexec.watchexec_args(false).is_empty());

        let (cli, watch) = parse_watch(&["mise", "watch", "--env", "dev", "--quiet", "build"]);
        assert_eq!(cli.env, Some(vec!["dev".to_string()]));
        assert!(cli.quiet);
        assert!(watch.watchexec.watchexec_args(false).is_empty());
    }

    #[test]
    fn glob_is_parsed() {
        let (_, watch) = parse_watch(&["mise", "watch", "--glob", "src/**/*.rs", "build"]);
        assert_eq!(watch.glob, s(&["src/**/*.rs"]));
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
    fn negation_within_origin_keeps_one_inside() {
        assert_eq!(
            negation_within_origin(Path::new("/repo/src/gen/**"), Path::new("/repo/src")),
            Some(pb("/repo/src/gen/**")),
        );
    }

    #[test]
    fn negation_within_origin_rebases_double_star_above() {
        assert_eq!(
            negation_within_origin(Path::new("/repo/**/*.tmp"), Path::new("/repo/src")),
            Some(pb("/repo/src/**/*.tmp")),
        );
        assert_eq!(
            negation_within_origin(Path::new("/repo/**"), Path::new("/repo/src/a")),
            Some(pb("/repo/src/a/**")),
        );
    }

    #[test]
    fn negation_within_origin_drops_one_that_cannot_match() {
        // A sibling of the origin.
        assert_eq!(
            negation_within_origin(Path::new("/repo/shared/**"), Path::new("/repo/src")),
            None,
        );
        // A single-level glob above the origin matches only files there.
        assert_eq!(
            negation_within_origin(Path::new("/repo/*.log"), Path::new("/repo/src")),
            None,
        );
        // A literal file above the origin.
        assert_eq!(
            negation_within_origin(Path::new("/repo/notes.txt"), Path::new("/repo/src")),
            None,
        );
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
