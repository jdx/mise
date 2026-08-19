use crate::errors::Error;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::iter::once;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::args::ToolArg;
use crate::cli::{Cli, unescape_task_args};
use crate::config::{Config, Settings};
use crate::deps::{DepsEngine, DepsOptions, DepsStepResult};
use crate::duration;
use crate::env;
use crate::file::display_path;
use crate::task::has_any_usage_spec;
use crate::task::task_executor::TaskRunContext;
use crate::task::task_helpers::task_needs_permit;
use crate::task::task_list::{get_task_lists, resolve_depends};
use crate::task::task_output::TaskOutput;
use crate::task::task_output_handler::OutputHandler;
use crate::task::{Deps, Task, TaskCacheMode, usage_command_for_args};
use crate::toolset::{InstallOptions, ResolveOptions, ToolVersion, ToolsetBuilder};
use crate::ui::{ctrlc, info, style};
use bytesize::ByteSize;
use clap::{CommandFactory, ValueHint};
use eyre::{Context, Result, bail, eyre};
use futures_util::FutureExt;
use itertools::Itertools;
use serde::Serialize;
use std::panic::AssertUnwindSafe;
use tokio::sync::Mutex;

/// Run task(s)
///
/// This command will run a task, or multiple tasks in parallel.
/// Tasks may have dependencies on other tasks or on source files.
/// If source is configured on a task, it will only run if the source
/// files have changed.
///
/// Tasks can be defined in mise.toml or as standalone scripts.
/// In mise.toml, tasks take this form:
///
///     [tasks.build]
///     run = "npm run build"
///     sources = ["src/**/*.ts"]
///     outputs = ["dist/**/*.js"]
///
/// Alternatively, tasks can be defined as standalone scripts.
/// These must be located in `mise-tasks`, `.mise-tasks`, `.mise/tasks`, `mise/tasks` or
/// `.config/mise/tasks`.
/// The name of the script will be the name of the tasks.
///
///     $ cat .mise/tasks/build<<EOF
///     #!/usr/bin/env bash
///     npm run build
///     EOF
///     $ mise run build
#[derive(clap::Args)]
#[clap(visible_alias = "r", verbatim_doc_comment, disable_help_flag = true, after_long_help = AFTER_LONG_HELP)]
pub struct Run {
    /// Tasks to run
    /// Can specify multiple tasks by separating with `:::`
    /// e.g.: mise run task1 arg1 arg2 ::: task2 arg1 arg2
    /// Defaults to `default` when omitted
    #[clap(allow_hyphen_values = true, verbatim_doc_comment)]
    pub task: Option<String>,

    /// Arguments to pass to the tasks. Use ":::" to separate tasks.
    #[clap(allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Arguments to pass to the tasks. Use ":::" to separate tasks.
    #[clap(allow_hyphen_values = true, hide = true, last = true)]
    pub args_last: Vec<String>,

    /// Run matching tasks only for projects affected by Git changes
    #[clap(long, verbatim_doc_comment)]
    pub affected: bool,

    /// Git base revision for --affected
    /// Defaults to MISE_AFFECTED_BASE, CI metadata, or HEAD~1
    #[clap(long, requires = "affected", value_name = "REV", verbatim_doc_comment)]
    pub affected_base: Option<String>,

    /// Explain why projects and tasks were selected by --affected
    #[clap(
        long,
        requires = "affected",
        conflicts_with = "affected_json",
        verbatim_doc_comment
    )]
    pub affected_explain: bool,

    /// Git head revision for --affected
    /// Defaults to MISE_AFFECTED_HEAD, CI metadata, or HEAD
    #[clap(long, requires = "affected", value_name = "REV", verbatim_doc_comment)]
    pub affected_head: Option<String>,

    /// Output affected projects and tasks as JSON without running tasks
    #[clap(
        long,
        requires = "affected",
        conflicts_with = "affected_explain",
        verbatim_doc_comment
    )]
    pub affected_json: bool,

    /// Open the interactive selector with all tasks from the entire monorepo
    #[clap(long, conflicts_with_all = ["task", "affected"], verbatim_doc_comment)]
    pub all: bool,

    /// Continue running tasks even if one fails
    #[clap(long, short = 'c', verbatim_doc_comment)]
    pub continue_on_error: bool,

    /// Change to this directory before executing the command
    #[clap(short = 'C', long, value_hint = ValueHint::DirPath, long)]
    pub cd: Option<PathBuf>,

    /// Force the tasks to run even if outputs are up to date
    #[clap(long, short, verbatim_doc_comment)]
    pub force: bool,

    /// Number of tasks to run in parallel
    /// Values below 1 are treated as 1
    /// [default: 4]
    /// Configure with `jobs` config or `MISE_JOBS` env var
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    pub jobs: Option<usize>,

    /// Don't actually run the task(s), just print them in order of execution
    #[clap(long, short = 'n', verbatim_doc_comment)]
    pub dry_run: bool,

    /// Change how tasks information is output when running tasks
    ///
    /// - `prefix` - Print stdout/stderr by line, prefixed with the task's label
    /// - `interleave` - Print directly to stdout/stderr instead of by line
    /// - `replacing` - Stdout is replaced each time, stderr is printed as is
    /// - `timed` - Only show stdout lines if they are displayed for more than 1 second
    /// - `keep-order` - Print stdout/stderr by line, prefixed with the task's label, but keep the order of the output
    /// - `quiet` - Don't show extra output
    /// - `silent` - Don't show any output including stdout and stderr from the task except for errors
    #[clap(short, long, verbatim_doc_comment, env = "MISE_TASK_OUTPUT")]
    pub output: Option<TaskOutput>,

    /// Don't show extra output
    #[clap(long, short, verbatim_doc_comment, env = "MISE_QUIET")]
    pub quiet: bool,

    /// Read/write directly to stdin/stdout/stderr instead of by line
    /// Redactions are not applied with this option
    /// Configure with `raw` config or `MISE_RAW` env var
    #[clap(long, short, verbatim_doc_comment)]
    pub raw: bool,

    /// Shell to use to run toml tasks
    ///
    /// Defaults to `sh -c -o errexit -o pipefail` on unix, and `cmd /c` on Windows
    /// Can also be set with the setting `MISE_UNIX_DEFAULT_INLINE_SHELL_ARGS` or `MISE_WINDOWS_DEFAULT_INLINE_SHELL_ARGS`
    /// Or it can be overridden with the `shell` property on a task.
    #[clap(long, short, verbatim_doc_comment)]
    pub shell: Option<String>,

    /// Don't show any output except for errors
    #[clap(long, short = 'S', verbatim_doc_comment, env = "MISE_SILENT")]
    pub silent: bool,

    /// Tool(s) to run in addition to what is in mise.toml files
    /// e.g.: node@20 python@3.10
    #[clap(short, long, value_name = "TOOL@VERSION")]
    pub tool: Vec<ToolArg>,

    #[clap(skip)]
    pub is_linear: bool,

    /// Allow specific env var through (implies --deny-env for everything else)
    /// Supports wildcards, e.g. --allow-env='MYAPP_*'
    #[clap(long, value_name = "VAR", verbatim_doc_comment)]
    pub allow_env: Vec<String>,

    /// Allow network to specific host (implies --deny-net for everything else)
    #[clap(long, value_name = "HOST", verbatim_doc_comment)]
    pub allow_net: Vec<String>,

    /// Allow reads from specific path (implies --deny-read for everything else)
    #[clap(long, value_name = "PATH", verbatim_doc_comment)]
    pub allow_read: Vec<std::path::PathBuf>,

    /// Allow writes to specific path (implies --deny-write for everything else)
    #[clap(long, value_name = "PATH", verbatim_doc_comment)]
    pub allow_write: Vec<std::path::PathBuf>,

    /// Block reads, writes, network, and env vars
    #[clap(long, verbatim_doc_comment)]
    pub deny_all: bool,

    /// Block env var inheritance (only PATH, HOME, USER, SHELL, TERM, LANG pass through)
    #[clap(long, verbatim_doc_comment)]
    pub deny_env: bool,

    /// Block all network access
    #[clap(long, verbatim_doc_comment)]
    pub deny_net: bool,

    /// Block filesystem reads (system libs and tool dirs still accessible)
    #[clap(long, verbatim_doc_comment)]
    pub deny_read: bool,

    /// Block all filesystem writes
    #[clap(long, verbatim_doc_comment)]
    pub deny_write: bool,

    /// Bypass the environment cache and recompute the environment
    #[clap(long)]
    pub fresh_env: bool,

    /// Do not use cache on remote tasks
    #[clap(long, verbatim_doc_comment, env = "MISE_TASK_REMOTE_NO_CACHE")]
    pub no_cache: bool,

    /// Skip automatic dependency preparation
    #[clap(long)]
    pub no_deps: bool,

    /// Hides elapsed time after each task completes
    ///
    /// Default to always hide with `MISE_TASK_TIMINGS=0`
    #[clap(long, alias = "no-timing", verbatim_doc_comment)]
    pub no_timings: bool,

    /// Run only the specified tasks skipping all dependencies
    #[clap(long, verbatim_doc_comment, env = "MISE_TASK_SKIP_DEPENDS")]
    pub skip_deps: bool,

    /// Skip installing tools before running tasks
    ///
    /// Can also be set persistently with the `task.run_auto_install` setting
    /// or `MISE_TASK_RUN_AUTO_INSTALL=false` env var
    #[clap(long, verbatim_doc_comment)]
    pub skip_tools: bool,

    /// Set task output cache access for this run
    ///
    /// - `read-write` - Read cached results and write new results
    /// - `read-only` - Read cached results without writing new results
    /// - `write-only` - Write new results without reading cached results
    /// - `off` - Disable task output caching
    /// - `local-only` - Read and write only the local cache; currently equivalent to `read-write`
    #[clap(
        long,
        value_enum,
        default_value = "read-write",
        env = "MISE_TASK_CACHE",
        verbatim_doc_comment
    )]
    pub task_cache: TaskCacheMode,

    /// Explain the inputs that produced each task's output cache key
    #[clap(long, verbatim_doc_comment)]
    pub task_cache_explain: bool,

    /// Output cache-key input details as JSON Lines without running tasks
    #[clap(
        long,
        requires = "dry_run",
        conflicts_with = "task_cache_explain",
        verbatim_doc_comment
    )]
    pub task_cache_explain_json: bool,

    /// Report task output cache hits, restored bytes, and time saved
    #[clap(long, conflicts_with = "dry_run", verbatim_doc_comment)]
    pub task_cache_stats: bool,

    /// Timeout for the task to complete
    /// e.g.: 30s, 5m
    #[clap(long, verbatim_doc_comment)]
    pub timeout: Option<String>,

    /// Shows elapsed time after each task completes
    ///
    /// Default to always show with `MISE_TASK_TIMINGS=1`
    #[clap(long, alias = "timing", verbatim_doc_comment, hide = true)]
    pub timings: bool,

    #[clap(skip)]
    pub tmpdir: PathBuf,

    #[clap(skip)]
    pub output_handler: Option<OutputHandler>,

    #[clap(skip)]
    pub context_builder: crate::task::task_context_builder::TaskContextBuilder,

    #[clap(skip)]
    pub executor: Option<crate::task::task_executor::TaskExecutor>,

    #[clap(skip)]
    pub cache_session: Option<crate::cache::session::CacheSession>,
}

fn affected_task_args(args: &[String]) -> Vec<String> {
    let mut task = true;
    args.iter()
        .map(|arg| {
            if arg == ":::" {
                task = true;
                return arg.clone();
            }
            if !task {
                return arg.clone();
            }
            task = false;
            if arg.starts_with("//")
                || arg.starts_with(':')
                || crate::task::is_workspace_project_task(arg)
            {
                arg.clone()
            } else {
                format!("//...:{arg}")
            }
        })
        .collect()
}

async fn get_affected_task_list(
    config: &Arc<Config>,
    args: &[String],
    only: bool,
    base: Option<&str>,
    head: Option<&str>,
    explain: bool,
    json: bool,
) -> Result<Vec<Task>> {
    Settings::get().ensure_experimental("affected tasks")?;
    let workspace_root = config
        .monorepo_root()
        .ok_or_else(|| eyre!("--affected requires a monorepo root configuration"))?;
    let graph = config.workspace_project_graph()?;
    let revisions = crate::task::workspace::git::WorkspaceGitRevisions::resolve(base, head);
    let changed_paths = revisions.changed_paths(&workspace_root)?;
    let global_inputs = config.monorepo_global_task_inputs().await?;
    let git = crate::git::Git::new(&workspace_root);
    let cargo = crate::task::workspace::cargo::CargoWorkspaceProvider;
    let go = crate::task::workspace::go::GoWorkspaceProvider;
    let node = crate::task::workspace::node::NodeWorkspaceProvider;
    let uv = crate::task::workspace::uv::UvWorkspaceProvider;
    let providers: [&dyn crate::task::workspace::WorkspaceProvider; 4] = [&cargo, &go, &node, &uv];
    let mut regular_paths = BTreeSet::new();
    let mut lockfile_projects = BTreeMap::<PathBuf, BTreeSet<_>>::new();
    let mut comparison_base: Option<String> = None;

    for path in changed_paths {
        let Some(lockfile_candidates) =
            graph.affected_projects_for_lockfile(&providers, &path, None, None)?
        else {
            regular_paths.insert(path);
            continue;
        };
        if lockfile_candidates.is_empty() {
            regular_paths.insert(path);
            continue;
        }
        let comparison_base = match &comparison_base {
            Some(base) => base.clone(),
            None => {
                let base = git.merge_base(&revisions.base, &revisions.head)?;
                comparison_base = Some(base.clone());
                base
            }
        };
        let before = git.file_at_revision(&comparison_base, &path)?;
        let after = git.file_at_revision(&revisions.head, &path)?;
        if let Some(projects) = graph.affected_projects_for_lockfile(
            &providers,
            &path,
            before.as_deref(),
            after.as_deref(),
        )? {
            lockfile_projects.entry(path).or_default().extend(projects);
        }
    }

    let affected = graph.affected_projects_for_changes(
        &workspace_root,
        regular_paths,
        &global_inputs,
        &lockfile_projects,
    )?;
    let affected_roots = affected
        .projects()
        .map(|(id, _)| id)
        .filter_map(|id| graph.get(id))
        .map(|project| crate::file::desymlink_path(&workspace_root.join(&project.root)))
        .collect::<BTreeSet<_>>();

    let args = affected_task_args(args);
    let mut tasks = get_task_lists(config, &args, true, only, false).await?;
    // Restrict only the task-pattern matches. `Run::run` calls `resolve_depends`
    // after this returns, so prerequisites from unaffected projects remain intact.
    tasks.retain(|task| {
        !task.global
            && task
                .config_root
                .as_deref()
                .map(crate::file::desymlink_path)
                .is_some_and(|root| affected_roots.contains(&root))
    });
    if json {
        display_affected_json(&revisions, &workspace_root, &graph, &affected, &tasks)?;
    } else if explain {
        display_affected_explanation(&revisions, &workspace_root, &graph, &affected, &tasks)?;
    }
    Ok(tasks)
}

#[derive(Serialize)]
struct AffectedSelectionOutput<'a> {
    base: &'a str,
    head: &'a str,
    projects: Vec<AffectedProjectOutput<'a>>,
    tasks: Vec<AffectedTaskOutput<'a>>,
}

#[derive(Serialize)]
struct AffectedProjectOutput<'a> {
    id: &'a crate::task::workspace::ProjectId,
    root: &'a std::path::Path,
    reasons: &'a BTreeSet<crate::task::workspace::AffectedProjectReason>,
}

#[derive(Serialize)]
struct AffectedTaskOutput<'a> {
    name: &'a str,
    projects: Vec<&'a crate::task::workspace::ProjectId>,
}

fn display_affected_json(
    revisions: &crate::task::workspace::git::WorkspaceGitRevisions,
    workspace_root: &std::path::Path,
    graph: &crate::task::workspace::WorkspaceProjectGraph,
    affected: &crate::task::workspace::AffectedProjects,
    tasks: &[Task],
) -> Result<()> {
    let mut projects_by_root = BTreeMap::<PathBuf, Vec<_>>::new();
    let projects = affected
        .projects()
        .map(|(id, reasons)| {
            let project = graph.get(id).expect("affected project exists in graph");
            projects_by_root
                .entry(crate::file::desymlink_path(
                    &workspace_root.join(&project.root),
                ))
                .or_default()
                .push(id);
            AffectedProjectOutput {
                id,
                root: &project.root,
                reasons,
            }
        })
        .collect();
    let mut tasks = tasks
        .iter()
        .map(|task| AffectedTaskOutput {
            name: &task.display_name,
            projects: task
                .config_root
                .as_deref()
                .map(crate::file::desymlink_path)
                .and_then(|root| projects_by_root.get(&root))
                .cloned()
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| left.name.cmp(right.name));
    let output = AffectedSelectionOutput {
        base: &revisions.base,
        head: &revisions.head,
        projects,
        tasks,
    };
    miseprintln!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn display_affected_explanation(
    revisions: &crate::task::workspace::git::WorkspaceGitRevisions,
    workspace_root: &std::path::Path,
    graph: &crate::task::workspace::WorkspaceProjectGraph,
    affected: &crate::task::workspace::AffectedProjects,
    tasks: &[Task],
) -> Result<()> {
    use crate::task::workspace::AffectedProjectReason;

    miseprintln!(
        "Affected projects ({}...{}):{}",
        revisions.base,
        revisions.head,
        if affected.is_empty() { " none" } else { "" }
    );
    let mut projects_by_root = BTreeMap::<PathBuf, Vec<_>>::new();
    for (id, reasons) in affected.projects() {
        let project = graph.get(id).expect("affected project exists in graph");
        miseprintln!("  {} ({})", id, display_affected_path(&project.root));
        for reason in reasons {
            match reason {
                AffectedProjectReason::ChangedPath { path } => {
                    miseprintln!("    changed path: {}", display_affected_path(path));
                }
                AffectedProjectReason::GlobalPath { path } => {
                    miseprintln!("    workspace-global path: {}", display_affected_path(path));
                }
                AffectedProjectReason::Lockfile { path } => {
                    miseprintln!("    lockfile change: {}", display_affected_path(path));
                }
                AffectedProjectReason::Dependent { dependency } => {
                    miseprintln!("    depends on affected project: {dependency}");
                }
            }
        }
        projects_by_root
            .entry(crate::file::desymlink_path(
                &workspace_root.join(&project.root),
            ))
            .or_default()
            .push(id);
    }

    miseprintln!(
        "Affected tasks:{}",
        if tasks.is_empty() { " none" } else { "" }
    );
    for task in tasks {
        miseprintln!("  {}", display_affected_text(&task.display_name));
        if let Some(ids) = task
            .config_root
            .as_deref()
            .map(crate::file::desymlink_path)
            .and_then(|root| projects_by_root.get(&root))
        {
            for id in ids {
                miseprintln!("    affected project: {id}");
            }
        }
    }
    Ok(())
}

fn display_affected_path(path: &std::path::Path) -> String {
    display_affected_text(&path.to_string_lossy())
}

fn display_affected_text(text: &str) -> String {
    text.escape_debug().to_string()
}

impl Run {
    pub async fn run(mut self) -> Result<()> {
        // Check help flags before doing any work
        if self.task.as_deref() == Some("-h") {
            self.get_clap_command().print_help()?;
            return Ok(());
        }
        if self.task.as_deref() == Some("--help") {
            self.get_clap_command().print_long_help()?;
            return Ok(());
        }

        let task = self.task.clone().unwrap_or_else(|| "default".to_string());

        Settings::ensure_not_safe("running tasks")?;

        // Unescape task args early so we can check for help flags
        self.args = unescape_task_args(&self.args);
        self.args_last = unescape_task_args(&self.args_last);

        // Temporarily unset cache key to force fresh env computation
        if self.fresh_env {
            env::reset_env_cache_key();
        }

        // Check if --help or -h is in the task args BEFORE toolset/deps
        // NOTE: Only check self.args, not self.args_last, because args_last contains
        // arguments after explicit -- which should always be passed through to the task
        let has_help_in_task_args =
            self.args.contains(&"--help".to_string()) || self.args.contains(&"-h".to_string());

        let mut config = Config::get().await?;

        // Handle task help early to avoid unnecessary toolset/deps work
        if has_help_in_task_args {
            // Build args list to get the task (filter out --help/-h for task lookup)
            let args = once(task.clone())
                .chain(
                    self.args
                        .iter()
                        .filter(|a| *a != "--help" && *a != "-h")
                        .cloned(),
                )
                .collect_vec();

            let task_list = get_task_lists(&config, &args, false, false, false).await?;

            if let Some(task) = task_list.first() {
                // raw_args tasks act as proxies for tools that handle their
                // own --help — fall through to normal execution so the flag
                // reaches the underlying command instead of mise.
                if !task.raw_args {
                    // Get usage spec to check if task has defined args/flags
                    let spec = task.parse_usage_spec_for_display(&config).await?;

                    if has_any_usage_spec(&spec) {
                        // Task has usage spec defined, render help using usage library
                        println!("{}", render_usage_help(&spec, &self.args));
                    } else {
                        // Task has no usage defined, show basic task info
                        display_task_help(task)?;
                    }
                    return Ok(());
                }
            } else {
                // No task found, show run command help
                self.get_clap_command().print_long_help()?;
                return Ok(());
            }
        }

        if !self.skip_deps {
            self.skip_deps = Settings::get().task.skip_depends;
        }

        time!("run init");
        let tmpdir = tempfile::tempdir()?;
        self.tmpdir = tmpdir.path().to_path_buf();

        // Build args list - don't include args_last yet, they'll be added after task resolution
        let args = if self.all {
            vec![]
        } else {
            once(task).chain(self.args.clone()).collect_vec()
        };

        let mut task_list = if self.affected {
            get_affected_task_list(
                &config,
                &args,
                self.skip_deps,
                self.affected_base.as_deref(),
                self.affected_head.as_deref(),
                self.affected_explain,
                self.affected_json,
            )
            .await?
        } else {
            get_task_lists(&config, &args, true, self.skip_deps, self.all).await?
        };
        if self.affected_json {
            return Ok(());
        }

        // Args after -- go directly to tasks (no prefix). They are also
        // recorded on `trailing_args` so the task renderer can detect
        // `-- --help` / `-- -h` and bypass the usage parser for them.
        if !self.args_last.is_empty() {
            for task in &mut task_list {
                task.args.extend(self.args_last.clone());
                task.trailing_args = self.args_last.clone();
            }
        }

        // Fetch remote task files before parsing usage specs, so that
        // file-based remote tasks have their files resolved to local cache.
        let fetcher = crate::task::task_fetcher::TaskFetcher::new(self.no_cache);
        fetcher.fetch_tasks(&config, &mut task_list).await?;

        // Re-render dependency templates with parent task's usage arg/flag values.
        // This enables patterns like: depends = ["child {{usage.app}}"]
        for task in &mut task_list {
            let has_usage_deps = |raw: &Option<Vec<_>>| {
                raw.as_ref()
                    .is_some_and(|r| r.iter().any(crate::task::dep_has_usage_ref))
            };
            if has_usage_deps(&task.depends_raw)
                || has_usage_deps(&task.depends_post_raw)
                || has_usage_deps(&task.wait_for_raw)
            {
                let usage_values = crate::task::parse_usage_values_from_task(&config, task).await?;
                if !usage_values.is_empty() {
                    task.render_depends_with_usage(&config, &usage_values)
                        .await?;
                }
            }
        }
        time!("run get_task_lists");

        // Resolve transitive dependencies once upfront so we can:
        // 1. Discover deps providers from monorepo subdirectory configs
        // 2. Include monorepo subdirectory tools in the toolset before installing
        // 3. Validate and install tools for the complete dependency set before execution
        let execution_tasks = task_list.clone();
        let resolved_tasks = resolve_depends(&config, task_list).await?;

        // Collect subdirectory config files from all resolved tasks. In
        // monorepos these come from sub mise.toml files referenced via the
        // `//sub:taskname` syntax — they aren't in `config.config_files`.
        let subdir_configs: Vec<_> = resolved_tasks
            .iter()
            .filter_map(|task| task.cf.clone())
            .collect();

        // Validate deps configuration before toolset construction can install
        // anything, then retain the engine for execution below.
        let mut layered_subdir_configs = vec![];
        let deps_engine = if self.no_deps {
            None
        } else if subdir_configs.is_empty() {
            Some(DepsEngine::new(&config)?)
        } else {
            let mut deps_config_files = config.config_files.clone();
            let selected_config_roots: HashSet<_> =
                subdir_configs.iter().map(|cf| cf.config_root()).collect();
            for config_root in subdir_configs.iter().map(|cf| cf.config_root()).unique() {
                let (config_paths, idiomatic_filenames) =
                    crate::config::load_config_hierarchy_from_dir(&config_root).await?;
                deps_config_files.extend(
                    crate::config::load_config_files_from_paths(
                        &config_paths,
                        &idiomatic_filenames,
                    )
                    .await?,
                );
            }
            deps_config_files.retain(|_, cf| {
                let config_root = cf.config_root();
                cf.project_root().is_some()
                    && selected_config_roots.contains(&config_root)
                    && config.project_root.as_ref() != Some(&config_root)
            });
            layered_subdir_configs.extend(deps_config_files.values().cloned());
            Some(DepsEngine::new_task_monorepo(
                &config,
                deps_config_files.into_values(),
            )?)
        };

        // Build the toolset using root config files plus subdir configs from
        // resolved tasks, so tools declared in monorepo subdirs are installed
        // before deps (e.g. `[deps.bun] auto=true`) try to use them.
        let mut combined_configs = config.config_files.clone();
        // The hierarchy loader returns higher-precedence files first. Preserve
        // that order so local overlays still win when ToolsetBuilder reverses
        // the map for low-to-high merging.
        for cf in layered_subdir_configs {
            combined_configs
                .entry(cf.get_path().to_path_buf())
                .or_insert(cf);
        }
        for cf in &subdir_configs {
            combined_configs
                .entry(cf.get_path().to_path_buf())
                .or_insert_with(|| cf.clone());
        }

        // Build and install toolset only after tasks resolve. A naked run that
        // does not match any task should fail without installing project tools.
        // Task startup should not fetch remote version metadata just to build
        // the environment. If tools are missing and auto-install is enabled,
        // install_missing_versions re-resolves those specific requests online.
        let resolve_options = ResolveOptions {
            offline: true,
            ..Default::default()
        };
        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .with_default_to_latest(true)
            .with_config_files(combined_configs)
            .with_resolve_options(resolve_options)
            .build(&config)
            .await?;

        let opts = InstallOptions {
            jobs: self.jobs,
            raw: self.raw,
            dry_run: self.dry_run,
            missing_args_only: !Settings::get().task.run_auto_install,
            skip_auto_install: !Settings::get().task.run_auto_install
                || !Settings::get().auto_install,
            ..Default::default()
        };
        let previewed_tools = if !self.skip_tools {
            let (installed, _) = ts.install_missing_versions(&mut config, &opts).await?;
            if self.dry_run {
                installed.into_iter().collect()
            } else {
                HashSet::new()
            }
        } else {
            HashSet::new()
        };

        // Run auto-enabled deps steps (unless --no-deps)
        if let Some(engine) = deps_engine {
            let env = ts.env_with_path(&config).await?;
            let result = engine
                .run(DepsOptions {
                    auto_only: true, // Only run providers with auto=true
                    dry_run: self.dry_run,
                    env,
                    ..Default::default()
                })
                .await?;
            for step in result.steps {
                if let DepsStepResult::WouldRun(id, reason) = step {
                    info!("[dry-run] Would install dependency: {id} ({reason})");
                }
            }
        }

        // Apply global timeout for entire run if configured
        let timeout = if let Some(timeout_str) = &self.timeout {
            Some(duration::parse_duration(timeout_str)?)
        } else {
            Settings::get().task_timeout_duration()
        };

        if let Some(timeout) = timeout {
            tokio::time::timeout(
                timeout,
                self.parallelize_tasks(config, execution_tasks, previewed_tools),
            )
            .await
            .map_err(|_| eyre!("mise run timed out after {:?}", timeout))??
        } else {
            self.parallelize_tasks(config, execution_tasks, previewed_tools)
                .await?
        }

        time!("run done");
        Ok(())
    }

    fn get_clap_command(&self) -> clap::Command {
        Cli::command()
            .get_subcommands()
            .find(|s| s.get_name() == "run")
            .unwrap()
            .clone()
    }

    async fn parallelize_tasks(
        mut self,
        mut config: Arc<Config>,
        tasks: Vec<Task>,
        previewed_tools: HashSet<ToolVersion>,
    ) -> Result<()> {
        time!("parallelize_tasks start");

        // Step 1: Prepare tasks (resolve dependencies, fetch, validate)
        let tasks = self.prepare_tasks(&config, tasks).await?;
        let num_tasks = tasks.all().count();

        // Step 2: Setup output handler and validate tasks
        self.setup_output_and_validate(&tasks)?;
        self.output = Some(self.output(None));

        // Step 3: Install tools needed by tasks
        if !self.skip_tools {
            self.install_task_tools(&mut config, &tasks, &previewed_tools)
                .await?;
        }

        // Step 4: Bracket action caching with this top-level task run. The
        // session owns the local agent and is flushed before results report.
        self.setup_cache_session(&tasks).await?;

        // Step 5: Create TaskExecutor after tool installation
        self.setup_executor()?;

        // Validate every scheduled invocation before starting the scheduler so
        // an invalid parent or dependency cannot run any task commands first.
        let executor = self.executor.as_ref().expect("task executor initialized");
        for task in tasks.all() {
            if let Err(err) = executor
                .preflight_task_usage(&config, task)
                .await
                .wrap_err_with(|| format!("failed to validate task {}", task.name))
            {
                if let Some(session) = &self.cache_session
                    && let Err(finish_err) = session.finish().await
                {
                    warn!("failed to finish action cache session: {finish_err:#}");
                }
                return Err(err);
            }
        }

        // Disable exit-on-ctrl-c so tasks can handle SIGINT gracefully
        ctrlc::exit_on_ctrl_c(false);

        let timer = std::time::Instant::now();
        let this = Arc::new(self);
        let config = config.clone();

        // Step 6: Initialize scheduler and run tasks
        let mut scheduler = crate::task::task_scheduler::Scheduler::new(this.jobs());
        let main_deps = Arc::new(Mutex::new(tasks));

        // Pump deps leaves into scheduler
        let mut main_done_rx = scheduler.pump_deps(main_deps.clone()).await;
        let spawn_context = scheduler.spawn_context(config.clone());
        scheduler
            .run_loop(
                &mut main_done_rx,
                main_deps.clone(),
                || this.is_stopping(),
                || this.is_interrupted(),
                this.continue_on_error,
                |task, deps_for_remove, allow_during_interruption| {
                    let this = this.clone();
                    let spawn_context = spawn_context.clone();
                    async move {
                        Self::spawn_sched_job(
                            this,
                            task,
                            deps_for_remove,
                            allow_during_interruption,
                            spawn_context,
                        )
                        .await
                    }
                },
            )
            .await?;

        let join_result = scheduler.join_all(this.continue_on_error).await;
        if let Some(session) = &this.cache_session {
            crate::cache::session::display_stats(session.finish().await?);
        }
        join_result?;

        // Step 7: Display results and handle failures
        let results_display = crate::task::task_results_display::TaskResultsDisplay::new(
            this.output_handler.clone().unwrap(),
            this.executor.as_ref().unwrap().failed_tasks.clone(),
            this.continue_on_error,
            this.timings(),
            this.is_interrupted(),
        );
        let result = results_display.display_results(num_tasks, timer);
        if this.task_cache_stats {
            this.display_task_cache_stats();
        }
        result?;
        time!("parallelize_tasks done");

        Ok(())
    }

    async fn spawn_sched_job(
        this: Arc<Self>,
        task: Task,
        deps_for_remove: Arc<Mutex<Deps>>,
        inherited_allow_during_interruption: bool,
        ctx: crate::task::task_scheduler::SpawnContext,
    ) -> Result<()> {
        if Self::should_abort_while_stopping(
            &this,
            &task,
            &deps_for_remove,
            inherited_allow_during_interruption,
        )
        .await
        {
            trace!(
                "aborting spawn before start while stopping: {} {}",
                task.name,
                task.args.join(" ")
            );
            return Ok(());
        }
        let needs_permit = task_needs_permit(&task);
        let permit_opt = if needs_permit {
            let wait_start = std::time::Instant::now();
            let p = Some(ctx.semaphore.clone().acquire_owned().await?);
            trace!(
                "semaphore acquired for {} after {}ms",
                task.name,
                wait_start.elapsed().as_millis()
            );
            // If a failure or interruption occurred while waiting for a permit,
            // skip this task unless failures may continue or it is a
            // post-dependency. Interruption always stops new normal tasks.
            if Self::should_abort_while_stopping(
                &this,
                &task,
                &deps_for_remove,
                inherited_allow_during_interruption,
            )
            .await
            {
                trace!(
                    "aborting spawn after wait while stopping: {} {}",
                    task.name,
                    task.args.join(" ")
                );
                return Ok(());
            }
            p
        } else {
            trace!("no semaphore needed for orchestrator task: {}", task.name);
            None
        };

        ctx.in_flight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let in_flight_c = ctx.in_flight.clone();
        trace!("running task: {task}");
        let allow_during_interruption = inherited_allow_during_interruption
            || deps_for_remove.lock().await.is_runnable_post_dep(&task);
        // Mark task as executed synchronously before spawning so that the
        // scheduler's failure-cleanup path (which checks is_runnable_post_dep)
        // always sees the parent in `executed` — avoiding a race where a
        // concurrent task fails between spawn and first poll.
        deps_for_remove.lock().await.mark_executed(&task);
        let semaphore = ctx.semaphore.clone();
        ctx.jset.lock().await.spawn(async move {
            let mut permit = permit_opt;
            let (completion_state, dependency_state) = {
                let deps = deps_for_remove.lock().await;
                (deps.completion_state(), deps.dependency_state(&task))
            };
            let (result, panicked) = match AssertUnwindSafe(this.run_task_sched(TaskRunContext {
                task: &task,
                config: &ctx.config,
                sched_tx: ctx.sched_tx.clone(),
                completion_state,
                dependency_state,
                semaphore,
                permit: &mut permit,
                allow_during_interruption,
            }))
            .catch_unwind()
            .await
            {
                Ok(result) => (result, false),
                Err(payload) => (
                    Err(eyre!("task panicked: {}", panic_payload_message(&payload))),
                    true,
                ),
            };
            // If the task executed or restored outputs and has sources defined,
            // mark it so dependents' source freshness checks are invalidated.
            // Tasks without sources always run and should not trigger invalidation.
            if let Ok(outcome) = &result {
                let mut deps = deps_for_remove.lock().await;
                if outcome.did_work && !task.sources.is_empty() {
                    deps.mark_did_work(&task);
                }
                if let Some(cache_key) = &outcome.cache_key {
                    deps.mark_cache_key(&task, cache_key.clone());
                }
            }
            let interrupted = result.as_ref().is_err_and(|err| {
                !panicked && ctrlc::is_cancelled() && Error::is_task_interrupted(err)
            });
            if let Err(err) = &result {
                if interrupted {
                    this.mark_interrupted();
                }
                let status = if panicked {
                    Some(1)
                } else {
                    Error::get_exit_status(err)
                };
                if !interrupted && !this.is_stopping() && (panicked || status.is_none()) {
                    let prefix = task.estyled_prefix();
                    if Settings::get().verbose {
                        this.eprint(&task, &prefix, &format!("{} {err:?}", style::ered("ERROR")));
                    } else {
                        this.eprint(&task, &prefix, &format!("{} {err}", style::ered("ERROR")));
                        let mut current_err = err.source();
                        while let Some(e) = current_err {
                            this.eprint(&task, &prefix, &format!("{} {e}", style::ered("ERROR")));
                            current_err = e.source();
                        }
                    };
                }
                if !interrupted {
                    this.add_failed_task(task.clone(), status);
                }
                // SIGTERM any still-running siblings so we exit promptly on
                // failure instead of waiting for them to finish naturally.
                // run_loop only sees `is_stopping` when it next iterates,
                // which doesn't happen while it's awaiting an idle select —
                // so the kill has to be triggered from here.
                if !interrupted && !this.continue_on_error {
                    debug!("task {} failed, killing siblings", task.name);
                    #[cfg(unix)]
                    crate::cmd::CmdLineRunner::kill_all(nix::sys::signal::SIGTERM);
                    #[cfg(windows)]
                    crate::cmd::CmdLineRunner::kill_all();
                }
            }
            if let Some(oh) = &this.output_handler
                && oh.output(Some(&task)) == TaskOutput::KeepOrder
            {
                oh.keep_order_state.lock().unwrap().on_task_finished(&task);
            }
            let mut deps = deps_for_remove.lock().await;
            if result
                .as_ref()
                .is_err_and(Error::is_task_interrupted_before_start)
            {
                deps.unmark_executed(&task);
            }
            deps.remove(&task);
            drop(deps);
            trace!("deps removed: {} {}", task.name, task.args.join(" "));
            in_flight_c.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            if interrupted {
                Ok(())
            } else {
                result.map(|_| ())
            }
        });

        Ok(())
    }

    async fn should_abort_while_stopping(
        this: &Self,
        task: &Task,
        deps_for_remove: &Arc<Mutex<Deps>>,
        inherited_allow_during_interruption: bool,
    ) -> bool {
        if !this.is_stopping()
            || (this.continue_on_error && !this.is_interrupted())
            || inherited_allow_during_interruption
        {
            return false;
        }
        let mut deps = deps_for_remove.lock().await;
        if deps.is_runnable_post_dep(task) {
            return false;
        }
        deps.remove(task);
        true
    }

    // ============================================================================
    // High-level workflow methods
    // ============================================================================

    /// Prepare tasks: fetch remote tasks and create dependency graph
    /// Dependencies should already be resolved via resolve_depends() before calling this.
    async fn prepare_tasks(&mut self, config: &Arc<Config>, mut tasks: Vec<Task>) -> Result<Deps> {
        let fetcher = crate::task::task_fetcher::TaskFetcher::new(self.no_cache);
        fetcher.fetch_tasks(config, &mut tasks).await?;
        let mut tasks = Deps::new(config, tasks).await?;
        tasks.mark_ambiguous_prefixes();
        self.is_linear = tasks.is_linear();
        Ok(tasks)
    }

    /// Initialize output handler and validate tasks
    fn setup_output_and_validate(&mut self, tasks: &Deps) -> Result<()> {
        // Initialize OutputHandler AFTER is_linear is determined
        let output_config = crate::task::task_output_handler::OutputHandlerConfig {
            output: self.output,
            silent: self.silent,
            quiet: self.quiet,
            raw: self.raw,
            is_linear: self.is_linear,
            jobs: self.jobs,
        };
        self.output_handler = Some(OutputHandler::new(output_config));

        // Spawn the timed-output printer if any task resolves to the Timed style
        // (run-wide default OR a per-task `output = "timed"` override).
        let any_timed = tasks
            .all()
            .any(|task| self.output(Some(task)) == TaskOutput::Timed);
        if any_timed {
            let timed_outputs = self.output_handler.as_ref().unwrap().timed_outputs.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(100));
                loop {
                    {
                        let mut outputs = timed_outputs.lock().unwrap();
                        for (prefix, out) in outputs.clone() {
                            let (time, lines) = out;
                            if time.elapsed().unwrap().as_secs() >= 1 {
                                for line in lines {
                                    if console::colors_enabled() {
                                        prefix_println!(prefix, "{line}\x1b[0m");
                                    } else {
                                        prefix_println!(prefix, "{line}");
                                    }
                                }
                                outputs.shift_remove(&prefix);
                            }
                        }
                    }
                    interval.tick().await;
                }
            });
        }

        // Validate and initialize task output
        for task in tasks.all() {
            self.validate_task(task)?;
            self.output_handler.as_mut().unwrap().init_task(task);
        }

        Ok(())
    }

    /// Create TaskExecutor after tool installation to ensure caches are populated
    fn setup_executor(&mut self) -> Result<()> {
        let executor_config = crate::task::task_executor::TaskExecutorConfig {
            force: self.force,
            cd: self.cd.clone(),
            shell: self.shell.clone(),
            tool: self.tool.clone(),
            timings: self.timings,
            continue_on_error: self.continue_on_error,
            dry_run: self.dry_run,
            skip_deps: self.skip_deps,
            task_cache: self.task_cache,
            task_cache_explain: self.task_cache_explain,
            task_cache_explain_json: self.task_cache_explain_json,
            cache_session: self
                .cache_session
                .as_ref()
                .map(crate::cache::session::CacheSession::environment),
            sandbox: crate::sandbox::SandboxConfig::from_settings_and_cli(
                &Settings::get().sandbox,
                self.deny_all,
                crate::sandbox::SandboxConfig {
                    deny_read: self.deny_read,
                    deny_write: self.deny_write,
                    deny_net: self.deny_net,
                    deny_local_sockets: false,
                    deny_env: self.deny_env,
                    allow_read: self.allow_read.clone(),
                    allow_write: self.allow_write.clone(),
                    allow_net: self.allow_net.clone(),
                    allow_env: self.allow_env.clone(),
                    pass_through_env: vec![],
                    cache_env: vec![],
                    deny_system_temp_write: false,
                    deny_mise_data_read: false,
                },
            ),
        };
        self.executor = Some(crate::task::task_executor::TaskExecutor::new(
            self.context_builder.clone(),
            self.output_handler.clone().unwrap(),
            executor_config,
        ));

        Ok(())
    }

    async fn setup_cache_session(&mut self, tasks: &Deps) -> Result<()> {
        let enabled = !self.dry_run
            && tasks
                .all()
                .any(|task| task.rust_cache.as_ref().is_some_and(|cache| cache.enabled));
        if !enabled {
            return Ok(());
        }
        if crate::cache::release_cache_context() {
            warn!("Rust action caching is disabled for release CI contexts");
            return Ok(());
        }
        self.cache_session = Some(
            crate::cache::session::CacheSession::start(
                &self.tmpdir,
                crate::task::task_cache::task_cache_dir().join("actions"),
            )
            .await?,
        );
        Ok(())
    }

    /// Collect and install all tools needed by tasks
    async fn install_task_tools(
        &self,
        config: &mut Arc<Config>,
        tasks: &Deps,
        previewed_tools: &HashSet<ToolVersion>,
    ) -> Result<()> {
        let installer = crate::task::task_tool_installer::TaskToolInstaller::new(
            &self.context_builder,
            &self.tool,
        );
        installer
            .install_tools(config, tasks, self.dry_run, previewed_tools)
            .await
    }

    // ============================================================================
    // Helper methods
    // ============================================================================

    fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        self.output_handler
            .as_ref()
            .unwrap()
            .eprint(task, prefix, line);
    }

    fn output(&self, task: Option<&Task>) -> TaskOutput {
        self.output_handler.as_ref().unwrap().output(task)
    }

    fn jobs(&self) -> usize {
        self.output_handler.as_ref().unwrap().jobs()
    }

    fn is_stopping(&self) -> bool {
        ctrlc::is_cancelled()
            || self
                .executor
                .as_ref()
                .map(|e| e.is_stopping())
                .unwrap_or(false)
    }

    fn is_interrupted(&self) -> bool {
        ctrlc::is_cancelled()
            || self
                .executor
                .as_ref()
                .map(|e| e.is_interrupted())
                .unwrap_or(false)
    }

    fn mark_interrupted(&self) {
        if let Some(executor) = &self.executor {
            executor.mark_interrupted();
        }
    }

    async fn run_task_sched(
        &self,
        ctx: TaskRunContext<'_>,
    ) -> Result<crate::task::task_executor::TaskRunOutcome> {
        self.executor
            .as_ref()
            .expect("executor must be initialized before running tasks")
            .run_task_sched(ctx)
            .await
    }

    fn add_failed_task(&self, task: Task, status: Option<i32>) {
        if let Some(executor) = &self.executor {
            executor.add_failed_task(task, status);
        }
    }

    fn validate_task(&self, task: &Task) -> Result<()> {
        use crate::file;
        use crate::ui;
        if self.task_cache.enabled() && task.cache.as_ref().is_some_and(|cache| cache.enabled) {
            Settings::get().ensure_experimental("task artifact caching")?;
        }
        if task.rust_cache.as_ref().is_some_and(|cache| cache.enabled) {
            Settings::get().ensure_experimental("Rust action caching")?;
        }
        if !task.pass_through_env.is_empty() {
            Settings::get().ensure_experimental("task environment pass-through")?;
        }
        if let Some(path) = &task.file
            && path.exists()
            && !file::is_executable(path)
        {
            let dp = crate::file::display_path(path);
            // Only offer the fix where accepting it can change the answer. `make_executable` is a
            // no-op on Windows, so the prompt would take a "yes" and then fail anyway; the same
            // reasoning already keeps `make_task_executable` from running there.
            if cfg!(windows) {
                bail!(
                    "`{dp}` is not executable. {}",
                    file::make_executable_hint(path)
                )
            }
            let msg = format!("Script `{dp}` is not executable. Make it executable?");
            if ui::confirm(msg)? {
                file::make_executable(path)?;
            } else {
                bail!(
                    "`{dp}` is not executable. {}",
                    file::make_executable_hint(path)
                )
            }
        }
        Ok(())
    }

    fn timings(&self) -> bool {
        !self.quiet(None) && !self.no_timings
    }

    fn display_task_cache_stats(&self) {
        let stats = *self
            .executor
            .as_ref()
            .expect("executor must be initialized before displaying cache stats")
            .cache_stats
            .lock()
            .unwrap();
        let lookups = stats.hits.saturating_add(stats.misses);
        if lookups == 0 {
            safe_eprintln!("Task cache: no lookups");
            return;
        }
        let hit_rate = stats.hits.saturating_mul(100) / lookups;
        safe_eprintln!(
            "Task cache: {}/{} hits ({}%), {} restored, {} saved",
            stats.hits,
            lookups,
            hit_rate,
            ByteSize::b(stats.restored_bytes).display().iec(),
            crate::ui::time::format_duration(stats.time_saved),
        );
    }

    fn quiet(&self, task: Option<&Task>) -> bool {
        self.output_handler.as_ref().unwrap().quiet(task)
    }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "unknown panic payload"
    }
}

fn display_task_help(task: &Task) -> Result<()> {
    let name = if task.display_name.is_empty() {
        &task.name
    } else {
        &task.display_name
    };
    info::inline_section("Task", name)?;
    if !task.aliases.is_empty() {
        info::inline_section("Aliases", task.aliases.join(", "))?;
    }
    if !task.description.is_empty() {
        info::inline_section("Description", &task.description)?;
    }
    info::inline_section(
        "Source",
        task.config_sources().iter().map(display_path).join(", "),
    )?;
    if !task.depends.is_empty() {
        info::inline_section("Depends on", task.depends.iter().join(", "))?;
    }
    let run = task.run();
    if !run.is_empty() {
        info::section("Run", run.iter().map(|e| e.to_string()).join("\n"))?;
    }
    miseprintln!();
    miseprintln!("This task does not accept any arguments.");
    let hint = if task.file.is_some() {
        "To define arguments, add #USAGE comments to the script file."
    } else {
        "To define arguments, add a `usage` field to the task definition in the config file."
    };
    miseprintln!("{hint}");
    miseprintln!("See https://mise.jdx.dev/tasks/task-configuration.html for more information.");
    Ok(())
}

fn render_usage_help(spec: &usage::Spec, args: &[String]) -> String {
    let cmd = usage_command_for_args(spec, args);
    usage::docs::cli::render_help(spec, cmd, true)
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Runs the "lint" tasks. This needs to either be defined in mise.toml
    # or as a standalone script. See the project README for more information.
    $ <bold>mise run lint</bold>

    # Forces the "build" tasks to run even if its sources are up-to-date.
    $ <bold>mise run --force build</bold>

    # Run "test" with stdin/stdout/stderr all connected to the current terminal.
    # This forces `--jobs=1` to prevent interleaving of output.
    $ <bold>mise run --raw test</bold>

    # Runs the "lint", "test", and "check" tasks in parallel.
    $ <bold>mise run lint ::: test ::: check</bold>

    # Execute multiple tasks each with their own arguments.
    $ <bold>mise run cmd1 arg1 arg2 ::: cmd2 arg1 arg2</bold>
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panic_payload_message_from_static_str() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("panic message");
        assert_eq!(panic_payload_message(&*payload), "panic message");
    }

    #[test]
    fn test_panic_payload_message_from_string() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("panic message"));
        assert_eq!(panic_payload_message(&*payload), "panic message");
    }

    #[test]
    fn test_panic_payload_message_from_unknown_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(123usize);
        assert_eq!(panic_payload_message(&*payload), "unknown panic payload");
    }

    #[test]
    fn affected_patterns_expand_across_projects_and_preserve_arguments() {
        assert_eq!(
            affected_task_args(&[
                "build".into(),
                "--release".into(),
                ":::".into(),
                "//apps/...:test".into(),
                "unit".into(),
                ":::".into(),
                "node:@scope/app#lint".into(),
            ]),
            vec![
                "//...:build",
                "--release",
                ":::",
                "//apps/...:test",
                "unit",
                ":::",
                "node:@scope/app#lint",
            ]
        );
    }

    #[test]
    fn affected_paths_escape_terminal_control_characters() {
        assert_eq!(
            display_affected_path(std::path::Path::new("src/\x1b[2J\nfile.rs")),
            r"src/\u{1b}[2J\nfile.rs"
        );
        assert_eq!(
            display_affected_text("//app:\x1b]8;;https://example.com\x1b\\build"),
            r"//app:\u{1b}]8;;https://example.com\u{1b}\\build"
        );
    }
}
