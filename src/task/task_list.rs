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
use std::collections::HashSet;
use std::iter::once;
use std::path::PathBuf;
use std::sync::Arc;

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
            style::eunderline(
                "https://mise.jdx.dev/tasks/task-configuration.html#monorepo-support"
            )
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

/// Show an error when a task is not found, with helpful suggestions
async fn err_no_task(config: &Config, name: &str) -> Result<()> {
    // Check early if the name looks like a mistyped CLI subcommand
    let similar_cmds = suggest_similar_commands(name);

    if config.tasks().await.is_ok_and(|t| t.is_empty()) {
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
        // that might contain tasks
        if let Some(cwd) = &*dirs::CWD {
            use crate::config::config_file::{config_trust_root, is_trusted};
            use crate::config::config_files_in_dir;

            let config_files = config_files_in_dir(cwd);
            let untrusted_configs: Vec<_> = config_files
                .iter()
                .filter(|p| !is_trusted(&config_trust_root(p)) && !is_trusted(p))
                .collect();

            if !untrusted_configs.is_empty() {
                let paths = untrusted_configs
                    .iter()
                    .map(display_path)
                    .collect::<Vec<_>>()
                    .join(", ");
                bail!(
                    "Config file(s) in {} are not trusted: {}\nTrust them with `mise trust`. See https://mise.jdx.dev/cli/trust.html for more information.",
                    display_path(cwd),
                    paths
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
    if name.starts_with("//") {
        // Load ALL monorepo tasks for suggestions
        if let Ok(tasks) = config
            .tasks_with_context(Some(&TaskLoadContext::all()))
            .await
        {
            let matcher = SkimMatcherV2::default().use_cache(true).smart_case();
            let similar: Vec<String> = tasks
                .keys()
                .filter(|k| k.starts_with("//"))
                .filter_map(|k| {
                    matcher
                        .fuzzy_match(&k.to_lowercase(), &name.to_lowercase())
                        .map(|score| (score, k.clone()))
                })
                .sorted_by_key(|(score, _)| -1 * *score)
                .take(5)
                .map(|(_, k)| k)
                .collect();

            if !similar.is_empty() {
                err_msg.push_str("\n\nDid you mean one of these?");
                for task_name in similar {
                    err_msg.push_str(&format!("\n  - {}", task_name));
                }
            }
        }
    }

    if !similar_cmds.is_empty() {
        err_msg.push_str("\n\nDid you mean the command:");
        for cmd_name in &similar_cmds {
            err_msg.push_str(&format!("\n  mise {cmd_name}"));
        }
    }

    bail!(err_msg);
}

/// Prompt the user to select a task interactively
async fn prompt_for_task() -> Result<Task> {
    let config = Config::get().await?;
    let tasks = config.tasks().await?;
    ensure!(
        !tasks.is_empty(),
        "no tasks defined. see {url}",
        url = style::eunderline("https://mise.jdx.dev/tasks/")
    );
    let theme = crate::ui::theme::get_theme();
    let mut s = Select::new("Tasks")
        .description("Select a task to run")
        .filtering(true)
        .filterable(true)
        .theme(&theme);
    for t in tasks.values().filter(|t| !t.hide) {
        // Truncate description to first line only, like tasks ls does
        let desc = t.description.lines().next().unwrap_or_default();
        s = s.option(
            DemandOption::new(&t.name)
                .label(&t.display_name)
                .description(desc),
        );
    }
    ctrlc::show_cursor_after_ctrl_c();
    match s.run() {
        Ok(name) => match tasks.get(name) {
            Some(task) => Ok(task.clone()),
            None => bail!("no tasks {} found", style::ered(name)),
        },
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
    // For monorepo patterns, we need to load tasks from the relevant parts of the monorepo
    let task_context = if args.is_empty() {
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
                err_no_task(config, &t).await?;
            }
            tasks.push(prompt_for_task().await?);
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
