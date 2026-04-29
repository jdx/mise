use crate::config::{self, Config, Settings};
use crate::file::display_path;
use crate::task::{
    GetMatchingExt, Task, TaskLoadContext, extract_monorepo_path, resolve_task_pattern,
};
use crate::ui::ctrlc;
use crate::ui::{prompt, style};
use crate::{dirs, file};
use console::Term;
use demand::{DemandOption, Select};
use eyre::{Result, bail, ensure, eyre};
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use itertools::Itertools;
use std::collections::{BTreeMap, HashSet};
use std::iter::once;
use std::path::PathBuf;
use std::sync::Arc;

const MAX_AVAILABLE_TASKS_IN_ERROR: usize = 20;

/// Find non-executable files in task include directories.
/// These are files that likely should be tasks but are missing the executable bit.
/// Skips hidden files (e.g., .gitkeep, .DS_Store) to match load_tasks_includes behavior.
pub fn find_non_executable_task_files(includes: &[PathBuf]) -> Vec<PathBuf> {
    includes
        .iter()
        .filter(|d| d.is_dir())
        .flat_map(|d| {
            let root = d.clone();
            walkdir::WalkDir::new(d)
                .into_iter()
                // skip hidden directories, but allow the root itself to be hidden (e.g. .mise-tasks)
                .filter_entry(move |e| {
                    e.path() == root || !e.file_name().to_string_lossy().starts_with('.')
                })
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file() && !file::is_executable(e.path()))
                .map(|e| e.path().to_path_buf())
        })
        .collect()
}

/// Split a task spec into name and args
/// e.g., "task arg1 arg2" -> ("task", vec!["arg1", "arg2"])
pub fn split_task_spec(spec: &str) -> (&str, Vec<String>) {
    let mut parts = spec.split_whitespace();
    let name = parts.next().unwrap_or("");
    let args = parts.map(|s| s.to_string()).collect_vec();
    (name, args)
}

/// Validate that monorepo features are properly configured
fn validate_monorepo_setup(config: &Arc<Config>) -> Result<()> {
    // Check if experimental mode is enabled
    if !Settings::get().experimental {
        bail!(
            "Monorepo task paths (like `//path:task` or `:task`) require experimental mode.\n\
            \n\
            To enable experimental features, set:\n\
            {}\n\
            \n\
            Or run with: {}",
            style::eyellow("  export MISE_EXPERIMENTAL=true"),
            style::eyellow("MISE_EXPERIMENTAL=1 mise run ...")
        );
    }

    // Check if a monorepo root is configured
    if !config.is_monorepo() {
        bail!(
            "Monorepo task paths (like `//path:task` or `:task`) require a monorepo root configuration.\n\
            \n\
            To set up monorepo support, add this to your root mise.toml:\n\
            {}\n\
            \n\
            Then create task files in subdirectories that will be automatically discovered.\n\
            See {} for more information.",
            style::eyellow("  experimental_monorepo_root = true"),
            style::eunderline("https://mise.en.dev/tasks/task-configuration.html#monorepo-support")
        );
    }

    Ok(())
}

/// Check if a name is similar to any known CLI subcommands using fuzzy matching
fn suggest_similar_commands(name: &str) -> Vec<String> {
    use clap::CommandFactory;
    let cmd = crate::cli::Cli::command();
    let matcher = SkimMatcherV2::default().use_cache(true).smart_case();
    cmd.get_subcommands()
        .flat_map(|s| std::iter::once(s.get_name()).chain(s.get_all_aliases()))
        .filter_map(|subcmd| {
            matcher
                .fuzzy_match(subcmd, name)
                .filter(|&score| score > 0)
                .map(|score| (score, subcmd.to_string()))
        })
        .sorted_by_key(|(score, _)| -1 * *score)
        .take(3)
        .map(|(_, subcmd)| subcmd)
        .collect()
}

async fn tasks_for_missing_task_error(
    config: &Config,
    name: &str,
    all: bool,
) -> Result<(Arc<BTreeMap<String, Task>>, bool)> {
    // In monorepos, users usually need `tasks ls --all` after a miss. Load that
    // same view for the error so sibling package tasks can be suggested.
    // Also if --all flag is used, show all tasks in the error message.
    if all
        || name.starts_with("//")
        || config.is_monorepo()
    {
        if let Ok(all_tasks) = config
            .tasks_with_context(Some(&TaskLoadContext::all()))
            .await
        && !all_tasks.is_empty()
        {
            return Ok((all_tasks, true));
        }
    }

    let tasks = config.tasks().await?;
    Ok((tasks, false))
}

fn similar_tasks(name: &str, tasks: &BTreeMap<String, Task>) -> Vec<String> {
    let candidates = tasks
        .values()
        .filter(|t| !t.hide)
        .map(|t| t.display_name.clone())
        .unique()
        .collect_vec();
    xx::suggest::similar_n_with_threshold(name, &candidates, 5, 0.75)
}

fn append_available_tasks(
    err_msg: &mut String,
    tasks: &BTreeMap<String, Task>,
    showing_all_tasks: bool,
) {
    let visible_tasks = tasks
        .values()
        .filter(|t| !t.hide)
        .sorted_by(|a, b| a.display_name.cmp(&b.display_name))
        .unique_by(|t| t.display_name.clone())
        .collect_vec();
    if visible_tasks.is_empty() {
        return;
    }

    let listed_tasks = visible_tasks
        .iter()
        .take(MAX_AVAILABLE_TASKS_IN_ERROR)
        .collect_vec();
    let name_width = listed_tasks
        .iter()
        .map(|t| t.display_name.len())
        .chain(once("Name".len()))
        .max()
        .unwrap_or("Name".len());

    if showing_all_tasks {
        err_msg.push_str("\n\nAvailable tasks (`mise tasks ls --all`):");
    } else {
        err_msg.push_str("\n\nAvailable tasks:");
    }
    err_msg.push_str(&format!(
        "\n  {:name_width$}  Description",
        "Name",
        name_width = name_width
    ));
    for task in listed_tasks {
        let desc = task.description.lines().next().unwrap_or_default();
        err_msg.push_str(&format!(
            "\n  {:name_width$}  {}",
            task.display_name,
            desc,
            name_width = name_width
        ));
    }

    let remaining = visible_tasks
        .len()
        .saturating_sub(MAX_AVAILABLE_TASKS_IN_ERROR);
    if remaining > 0 {
        let noun = if remaining == 1 { "task" } else { "tasks" };
        err_msg.push_str(&format!(
            "\n  ... and {remaining} more {noun}. Run `mise tasks ls --all` to list all tasks."
        ));
    }
}

/// Show an error when a task is not found, with helpful suggestions
async fn err_no_task(config: &Config, name: &str, all: bool) -> Result<()> {
    // Check early if the name looks like a mistyped CLI subcommand
    let similar_cmds = suggest_similar_commands(name);
    let (tasks_for_error, showing_all_tasks) = tasks_for_missing_task_error(config, name, all).await?;

    if tasks_for_error.is_empty() {
        // If the name matches a CLI subcommand closely, suggest that instead of
        // the confusing "no tasks defined" message
        if !similar_cmds.is_empty() {
            let mut err_msg = format!("unknown command: {}", style::ered(name));
            err_msg.push_str("\n\nDid you mean:");
            for cmd_name in &similar_cmds {
                err_msg.push_str(&format!("\n  mise {cmd_name}"));
            }
            bail!(err_msg);
        }

        // Check if there are any untrusted config files in the current directory
        // that might contain tasks.
        if let Some(cwd) = &*dirs::CWD {
            use crate::config::config_file::{config_trust_root, is_trusted};
            use crate::config::{config_files_in_dir, is_tool_versions_file};

            let config_files = config_files_in_dir(cwd);
            let untrusted_configs: Vec<_> = config_files
                .iter()
                .filter(|p| {
                    !is_tool_versions_file(p)
                        && !is_trusted(&config_trust_root(p))
                        && !is_trusted(p)
                })
                .collect();

            if !untrusted_configs.is_empty() {
                let paths = untrusted_configs
                    .iter()
                    .map(display_path)
                    .collect::<Vec<_>>()
                    .join(", ");
                bail!(
                    "Config file(s) in {} are not trusted: {}\nTrust them with `mise trust`. See https://mise.en.dev/cli/trust.html for more information.",
                    display_path(cwd),
                    paths
                );
            }
        }

        // Check if there are non-executable files in task include directories
        if !cfg!(windows)
            && let Some(cwd) = &*dirs::CWD
        {
            let includes = config::task_includes_for_dir(cwd, &config.config_files);
            let non_exec_files = find_non_executable_task_files(&includes);
            if !non_exec_files.is_empty() {
                let dirs_with_files: Vec<String> = includes
                    .iter()
                    .filter(|d| d.is_dir())
                    .map(display_path)
                    .collect();
                bail!(
                    "no tasks defined in {}, but found {} non-executable file(s) in {}.\n\
                        Files must be executable to be detected as tasks.\n\
                        Run `chmod +x` on the task files to fix this, e.g.:\n  chmod +x {}",
                    display_path(dirs::CWD.clone().unwrap_or_default()),
                    non_exec_files.len(),
                    dirs_with_files.join(", "),
                    non_exec_files
                        .iter()
                        .take(5)
                        .map(display_path)
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
        }

        bail!(
            "no tasks defined in {}. Are you in a project directory?",
            display_path(dirs::CWD.clone().unwrap_or_default())
        );
    }
    if let Some(cwd) = &*dirs::CWD {
        let includes = config::task_includes_for_dir(cwd, &config.config_files);
        let path = includes
            .iter()
            .map(|d| d.join(name))
            .find(|d| d.is_file() && !file::is_executable(d));
        if let Some(path) = path
            && !cfg!(windows)
        {
            warn!(
                "no task {} found, but a non-executable file exists at {}",
                style::ered(name),
                display_path(&path)
            );
            let yn =
                prompt::confirm("Mark this file as executable to allow it to be run as a task?")?;
            if yn {
                file::make_executable(&path)?;
                info!("marked as executable, try running this task again");
            }
        }
    }

    // Suggest similar tasks using fuzzy matching for monorepo tasks
    let mut err_msg = format!("no task {} found", style::ered(name));

    let similar = similar_tasks(name, &tasks_for_error);
    if !similar.is_empty() {
        err_msg.push_str("\n\nDid you mean one of these?");
        for task_name in similar {
            err_msg.push_str(&format!("\n  - {}", task_name));
        }
    }

    if !similar_cmds.is_empty() {
        err_msg.push_str("\n\nDid you mean the command:");
        for cmd_name in &similar_cmds {
            err_msg.push_str(&format!("\n  mise {cmd_name}"));
        }
    }

    append_available_tasks(&mut err_msg, &tasks_for_error, showing_all_tasks);

    bail!(err_msg);
}

/// Prompt the user to select a task interactively
async fn prompt_for_task(ctx: Option<&TaskLoadContext>) -> Result<Task> {
    let config = Config::get().await?;
    let all_tasks = config.tasks_with_context(ctx).await?;
    let visible_tasks: Vec<_> = all_tasks
        .values()
        .filter(|t| !t.hide)
        .collect_vec();
    ensure!(
        !visible_tasks.is_empty(),
        "no tasks defined. see {url}",
        url = style::eunderline("https://mise.en.dev/tasks/")
    );
    let theme = crate::ui::theme::get_theme();
    let mut s = Select::new("Tasks")
        .description("Select a task to run")
        .filtering(true)
        .filterable(true)
        .theme(&theme);
    for t in &visible_tasks {
        let desc = t.description.lines().next().unwrap_or_default();
        s = s.option(
            DemandOption::new(&t.name)
                .label(&t.display_name)
                .description(desc),
        );
    }
    ctrlc::show_cursor_after_ctrl_c();
    match s.run() {
        Ok(name) => all_tasks
            .get(name.as_str())
            .cloned()
            .ok_or_else(|| eyre!("no tasks {} found", style::ered(name))),
        Err(err) => {
            Term::stderr().show_cursor()?;
            Err(eyre!(err))
        }
    }
}

/// Get a list of tasks to run from command-line arguments
/// Handles task patterns, monorepo paths, and interactive selection
pub async fn get_task_lists(
    config: &Arc<Config>,
    args: &[String],
    prompt: bool,
    only: bool,
    all: bool,
) -> Result<Vec<Task>> {
    let args = args
        .iter()
        .map(|s| vec![s.to_string()])
        .coalesce(|a, b| {
            if b == vec![":::".to_string()] {
                Err((a, b))
            } else if a == vec![":::".to_string()] {
                Ok(b)
            } else {
                Ok(a.into_iter().chain(b).collect_vec())
            }
        })
        .flat_map(|args| args.split_first().map(|(t, a)| (t.clone(), a.to_vec())))
        .collect::<Vec<_>>();

    // Determine the appropriate task loading context based on patterns
    // For monorepo patterns, we need to load tasks from relevant parts of the monorepo
    let task_context = if all {
        // --all flag: load all tasks from the entire monorepo
        validate_monorepo_setup(config)?;
        Some(TaskLoadContext::all())
    } else if args.is_empty() {
        None
    } else {
        // Collect all monorepo patterns
        let monorepo_patterns: Vec<&str> = args
            .iter()
            .filter_map(|(t, _)| {
                if t.starts_with("//") || t.contains("...") || t.starts_with(':') {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect();

        if monorepo_patterns.is_empty() {
            None
        } else {
            // Validate monorepo setup before attempting to load tasks
            validate_monorepo_setup(config)?;

            // Merge all path hints from the patterns into a single context
            Some(TaskLoadContext::from_patterns(
                monorepo_patterns.into_iter(),
            ))
        }
    };

    let mut tasks = vec![];
    let arg_re = xx::regex!(r#"^((\.*|~)(/|\\)|\w:\\)"#);
    for (t, args) in args {
        // Expand :task pattern to match tasks in current directory's config root
        let original_name = t.clone();
        let t = crate::task::expand_colon_task_syntax(&t, config)?;

        // A path starting with "//" on Windows will be treated as a UNC path by
        // PathBuf, but "//" in UNIX will be collapsed to "/" by PathBuf.
        // Checking a non-existent UNC path for Windows will incur a large
        // hiccup (~2.8s) due to Windows trying to resolve the UNC path.
        let t_for_path_check = t
            .strip_prefix("//")
            .map(|s| format!("/{s}"))
            .unwrap_or_else(|| t.clone());

        // can be any of the following:
        // - ./path/to/script
        // - ~/path/to/script
        // - /path/to/script
        // - ../path/to/script
        // - C:\path\to\script
        // - .\path\to\script
        if arg_re.is_match(&t_for_path_check) {
            let path = PathBuf::from(&t_for_path_check);
            if path.exists() {
                let config_root = config
                    .project_root
                    .clone()
                    .or_else(|| dirs::CWD.clone())
                    .unwrap_or_default();
                let task = Task::from_path(config, &path, &PathBuf::new(), &config_root).await?;
                return Ok(vec![task.with_args(args)]);
            }
        }
        // Load tasks with the appropriate context
        // If the task was expanded to monorepo syntax (e.g., bare "build" -> "//packages/foo:build"),
        // we need to create a context from the expanded name to load tasks from that location
        let effective_context = if task_context.is_some() {
            task_context.clone()
        } else if t.starts_with("//") {
            // Task was expanded to monorepo syntax, create context from the expanded name
            Some(TaskLoadContext::from_pattern(&t))
        } else {
            None
        };

        let all_tasks = if let Some(ref ctx) = effective_context {
            config.tasks_with_context(Some(ctx)).await?
        } else {
            config.tasks().await?
        };

        let tasks_with_aliases = crate::task::build_task_ref_map(all_tasks.iter());

        let mut cur_tasks = tasks_with_aliases
            .get_matching(&t)?
            .into_iter()
            .cloned()
            .collect_vec();
        // If the task name was auto-expanded to monorepo syntax (e.g., "hello" -> "//:hello")
        // but no monorepo task matched, fall back to the original bare name to find global tasks
        if cur_tasks.is_empty()
            && t != original_name
            && !original_name.starts_with("//")
            && !original_name.starts_with(':')
        {
            cur_tasks = tasks_with_aliases
                .get_matching(&original_name)?
                .into_iter()
                .cloned()
                .collect_vec();
        }
        if cur_tasks.is_empty() {
            // Check if this is a "default" task (either plain "default" or monorepo syntax like "//:default")
            // For monorepo tasks, ensure it starts with "//" and has exactly one ":" before "default"
            // This matches "//:default" and "//subfolder:default" but not "//subfolder:task-group:default"
            let is_default_task = t == "default" || {
                t.starts_with("//") && t.ends_with(":default") && t[2..].matches(':').count() == 1
            };
            if !is_default_task || !prompt || !console::user_attended_stderr() {
                err_no_task(config, &t, all).await?;
            }
            tasks.push(prompt_for_task(effective_context.as_ref()).await?);
        } else {
            cur_tasks
                .into_iter()
                .map(|t| t.clone().with_args(args.to_vec()))
                .for_each(|t| tasks.push(t));
        }
    }
    if only {
        for task in &mut tasks {
            task.depends.clear();
            task.depends_post.clear();
            task.wait_for.clear();
        }
    }
    Ok(tasks)
}

/// Resolve all dependencies for a list of tasks
/// Iteratively discovers path hints by loading tasks and their dependencies
pub async fn resolve_depends(config: &Arc<Config>, tasks: Vec<Task>) -> Result<Vec<Task>> {
    // Iteratively discover all path hints by loading tasks and their dependencies
    // This handles chains like: //A:B -> :C -> :D -> //E:F where we need to discover E
    let mut all_path_hints = HashSet::new();
    let mut tasks_to_process: Vec<Task> = tasks.clone();
    let mut processed_tasks = HashSet::new();

    // Iteratively discover paths until no new paths are found
    while !tasks_to_process.is_empty() {
        // Extract path hints from current batch of tasks
        let new_hints: Vec<String> = tasks_to_process
            .iter()
            .filter_map(|t| extract_monorepo_path(&t.name))
            .chain(tasks_to_process.iter().flat_map(|t| {
                t.depends
                    .iter()
                    .chain(t.wait_for.iter())
                    .chain(t.depends_post.iter())
                    .map(|td| resolve_task_pattern(&td.task, Some(t)))
                    .filter_map(|resolved| extract_monorepo_path(&resolved))
            }))
            .collect();

        // Check if we found any new paths
        let mut had_new_hints = false;
        for h in &new_hints {
            if all_path_hints.insert(h.clone()) {
                had_new_hints = true;
            }
        }
        if !had_new_hints {
            break;
        }

        // Load tasks with current path hints to discover dependencies
        let ctx = Some(TaskLoadContext {
            path_hints: all_path_hints.iter().cloned().collect(),
            load_all: false,
        });

        let loaded_tasks = config.tasks_with_context(ctx.as_ref()).await?;

        // Find new tasks that haven't been processed yet
        tasks_to_process = loaded_tasks
            .values()
            .filter(|t| processed_tasks.insert(t.name.clone()))
            .cloned()
            .collect();
    }

    // Now load all tasks with the complete set of path hints
    let ctx = if !all_path_hints.is_empty() {
        Some(TaskLoadContext {
            path_hints: all_path_hints.into_iter().collect(),
            load_all: false,
        })
    } else {
        None
    };

    let all_tasks = config.tasks_with_context(ctx.as_ref()).await?;

    tasks
        .into_iter()
        .map(|t| {
            let depends = t.all_depends(&all_tasks)?;
            Ok(once(t).chain(depends).collect::<Vec<_>>())
        })
        .flatten_ok()
        .collect()
}
