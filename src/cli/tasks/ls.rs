use std::sync::Arc;

use crate::config::{self, Config};
use crate::dirs;
use crate::file;
use crate::file::display_rel_path;
use crate::task::Task;
use crate::task::task_fetcher::TaskFetcher;
use crate::task::task_list::find_non_executable_task_files;
use crate::ui::table::MiseTable;
use comfy_table::{Attribute, Cell, Row};
use eyre::{Result, bail};
use itertools::Itertools;
use serde_json::json;

/// List available tasks
///
/// Lists tasks from `[tasks]` in your config files and from task directories
/// such as `mise-tasks/`, `.mise-tasks/`, and `.mise/tasks/`, including global
/// tasks in `~/.config/mise/tasks/`. Tasks from parent directories and the
/// global config are included; a project task overrides a global task with the
/// same name. In a monorepo, `--all` also lists tasks from sibling projects.
///
/// See https://mise.jdx.dev/tasks/task-discovery.html
#[derive(Debug, usage_rs::Args)]
#[usage(
    after_long_help = AFTER_LONG_HELP,
    example("mise tasks ls", help = "List tasks"),
    example("mise tasks ls --hidden", help = "Include hidden tasks"),
    example("mise tasks ls --all", help = "List tasks from every monorepo project"),
    example(
        "mise tasks ls --name-only | fzf",
        help = "Pick a task name interactively"
    )
)]
pub(crate) struct TasksLs {
    /// Only show global tasks
    #[usage(short, long, overrides = ["local", "no_global"], verbatim_doc_comment)]
    pub global: bool,

    /// Output in JSON format
    #[usage(short = 'J', long, verbatim_doc_comment)]
    pub json: bool,

    /// Deprecated alias for --no-global
    ///
    /// Removed in mise 2027.7.0.
    #[usage(short, long, overrides = ["global", "no_global"], verbatim_doc_comment)]
    pub local: bool,

    /// Only show non-global tasks
    #[usage(long, overrides = ["global", "local"], verbatim_doc_comment)]
    pub no_global: bool,

    /// Show all columns
    #[usage(short = 'x', long, verbatim_doc_comment)]
    pub extended: bool,

    /// Include tasks from every project in the monorepo, not only the current
    /// directory's hierarchy
    #[usage(short, long)]
    pub all: bool,

    /// Display tasks for usage completion
    #[usage(long, hide = true)]
    pub complete: bool,

    /// Show hidden tasks
    #[usage(long, verbatim_doc_comment)]
    pub hidden: bool,

    /// Print only task names, one per line (for fzf and similar tools)
    #[usage(
        long,
        verbatim_doc_comment,
        conflicts = ["json", "extended", "usage"]
    )]
    pub name_only: bool,

    /// Do not print the table header
    #[usage(long, alias = "no-headers", verbatim_doc_comment)]
    pub no_header: bool,

    /// Column to sort by (default: name)
    #[usage(long, value_name = "COLUMN", verbatim_doc_comment, value_enum)]
    pub sort: Option<SortColumn>,

    /// Sort direction (default: asc)
    #[usage(long, verbatim_doc_comment, value_enum)]
    pub sort_order: Option<SortOrder>,

    #[usage(long, hide = true)]
    pub usage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, usage_rs::ValueEnum)]
pub(crate) enum SortColumn {
    Name,
    Alias,
    Description,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, usage_rs::ValueEnum)]
pub(crate) enum SortOrder {
    Asc,
    Desc,
}

impl TasksLs {
    pub(super) fn merge(mut self, later: Self) -> Result<Self> {
        if later.global || later.local || later.no_global {
            self.global = later.global;
            self.local = later.local;
            self.no_global = later.no_global;
        }
        self.json |= later.json;
        self.extended |= later.extended;
        self.all |= later.all;
        self.complete |= later.complete;
        self.hidden |= later.hidden;
        self.name_only |= later.name_only;
        self.no_header |= later.no_header;
        self.sort = later.sort.or(self.sort);
        self.sort_order = later.sort_order.or(self.sort_order);
        self.usage |= later.usage;
        if self.name_only && (self.json || self.extended || self.usage) {
            bail!("--name-only cannot be used with --json, --extended, or --usage");
        }
        Ok(self)
    }

    pub(super) fn has_options(&self) -> bool {
        self.json || self.has_non_json_options()
    }

    pub(super) fn has_non_json_options(&self) -> bool {
        self.global
            || self.local
            || self.no_global
            || self.extended
            || self.all
            || self.complete
            || self.hidden
            || self.name_only
            || self.no_header
            || self.sort.is_some()
            || self.sort_order.is_some()
            || self.usage
    }

    pub(super) async fn run(self) -> Result<()> {
        use crate::task::TaskLoadContext;

        if self.local {
            crate::deprecated_at!(
                "2027.1.0",
                "2027.7.0",
                "cli.tasks-ls.local",
                "`mise tasks ls --local` is deprecated. Use `mise tasks ls --no-global` instead."
            );
        }

        let config = Config::get().await?;

        // Create context based on --all flag or when generating completions/usage specs
        // to ensure monorepo tasks (e.g., `//app:task`) are available for autocomplete.
        let ctx = if self.all || self.complete || self.usage {
            Some(TaskLoadContext::all())
        } else {
            None
        };

        let all_tasks = config.tasks_with_context(ctx.as_ref()).await?;

        let tasks = all_tasks
            .values()
            .filter(|t| self.hidden || !t.hide)
            .filter(|t| !(self.local || self.no_global) || !t.global)
            .filter(|t| !self.global || t.global)
            .cloned()
            .sorted_by(|a, b| self.sort(a, b))
            .collect::<Vec<Task>>();

        // Resolve remote task files before any operation that may need them
        let mut tasks = tasks;
        // always pass no_cache=false as the command doesn't take no-cache argument
        // MISE_TASK_REMOTE_NO_CACHE env var is still respected if set
        let fetcher = TaskFetcher::for_inspection(false);
        // Completion output goes to the shell, so only the human-facing modes warn.
        let fetcher = if self.complete || self.usage {
            fetcher.without_warning()
        } else {
            fetcher
        };
        fetcher.fetch_tasks(&config, &mut tasks).await?;

        // Warn about non-executable files only when there are truly no tasks at all
        // (not just filtered out by --hidden/--local/--global)
        if all_tasks.is_empty()
            && let Some(cwd) = &*dirs::CWD
        {
            let includes = config::task_includes_for_dir(cwd, &config.config_files)?;
            let excludes = config::task_excludes_for_dir(cwd, &config.config_files)?;
            // One file is enough to act on, and `make_executable_hint` is the only thing that
            // knows what "make it executable" means on this platform. Bound to a local because
            // under edition 2024 an `if let` scrutinee temporary is dropped before the body runs.
            let non_executable = find_non_executable_task_files(&includes, &excludes);
            if let Some(path) = non_executable.first() {
                warn!(
                    "no tasks found, but non-executable files exist in task directories.\nFiles must be executable to be detected as tasks. {}",
                    file::make_executable_hint(path)
                );
            }
        }

        if self.complete {
            return self.complete(&config, tasks);
        } else if self.usage {
            self.display_usage(&config, tasks).await?;
        } else if self.json {
            self.display_json(&config, tasks).await?;
        } else if self.name_only {
            self.display_name_only(tasks)?;
        } else {
            self.display(tasks)?;
        }
        Ok(())
    }

    fn display_name_only(&self, tasks: Vec<Task>) -> Result<()> {
        for t in tasks {
            calm_io::stdoutln!("{}", t.display_name)?;
        }
        Ok(())
    }

    fn complete(&self, config: &Config, tasks: Vec<Task>) -> Result<()> {
        let current_root = current_root_prefix(config);
        for t in tasks {
            // One line per task: a line break in a description would start a candidate of
            // its own, and a name with a control character cannot be completed anyway.
            let description: String = t
                .description
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            let description = description.replace(":", "\\:");
            let shorthand = shorthand_name(current_root.as_deref(), &t);
            for name in std::iter::once(t.display_name.clone()).chain(shorthand) {
                if name.chars().any(char::is_control) {
                    continue;
                }
                let name = name.replace(":", "\\:");
                calm_io::stdoutln!("{name}:{description}")?;
            }
        }
        Ok(())
    }

    fn display(&self, tasks: Vec<Task>) -> Result<()> {
        let mut table = MiseTable::new(
            self.no_header,
            if self.extended {
                &["Name", "Aliases", "Source", "Description"]
            } else {
                &["Name", "Description"]
            },
        );
        for task in tasks {
            table.add_row(self.task_to_row(&task));
        }
        table.print()
    }

    async fn display_usage(&self, config: &Arc<Config>, tasks: Vec<Task>) -> Result<()> {
        let mut usage = usage::Spec::default();
        let current_root = current_root_prefix(config);
        for task in tasks {
            // Without experimental a `git::` file was not fetched, so its `#USAGE` header is
            // unknown; complete the task from its TOML metadata alone.
            let mut task_spec = match TaskFetcher::toml_only(&task) {
                Some(toml_only) => toml_only.parse_usage_spec_for_display(config).await?,
                None => task.parse_usage_spec_for_display(config).await?,
            };
            for (name, complete) in task_spec.complete {
                task_spec.cmd.complete.insert(name, complete);
            }
            // Completion runs the commands a spec names (`complete run=`, `mount`, ...) while
            // it parses the typed words. For a task nobody trusted, that is code from the
            // repository running before `mise run` was ever entered.
            if !task.is_trusted(config) {
                remove_spec_commands(&mut task_spec.cmd);
            }
            // Absorb words the caller forwards to the task beyond what it declares, so
            // `mise run <task> -- --not-a-mise-flag` completes instead of erroring with
            // "unexpected word". This must NOT be spelled `-- ARGS_LAST`: jdx/usage#762
            // made an explicit `--` route the positional cursor onto the arg that declared
            // the separator, past earlier args, so a `--`-gated catch-all swallowed the
            // word being completed and `mise run <task> -- <TAB>` offered filenames
            // instead of the task's own choices. A plain trailing variadic takes the
            // overflow without hijacking `--`, and behaves the same on usage 4 and 5.
            task_spec.cmd.args.push(
                usage::SpecArgBuilder::new()
                    .name("ARGS")
                    .help("Arguments to pass to the tasks. Use \":::\" to separate tasks.")
                    .hide(true)
                    .var(true)
                    .build(),
            );
            if let Some(path) = crate::task::extract_monorepo_path(&task.display_name) {
                let prefixed_aliases: Vec<String> = task_spec
                    .cmd
                    .aliases
                    .iter()
                    .map(|a| format!("//{}:{}", path, a))
                    .collect();
                task_spec.cmd.aliases.extend(prefixed_aliases);
            }
            // Aliases get no `:` form: they exist to save typing, and listing every spelling
            // of every alias crowds the candidates. `mise tasks ls` shows them.
            if let Some(name) = shorthand_name(current_root.as_deref(), &task) {
                task_spec.cmd.aliases.push(name);
            }
            usage
                .cmd
                .subcommands
                .insert(task.display_name.clone(), task_spec.cmd);
        }
        miseprintln!("{}", usage.to_string());
        Ok(())
    }

    async fn display_json(&self, config: &Arc<Config>, tasks: Vec<Task>) -> Result<()> {
        let mut array_items: Vec<serde_json::Value> = Vec::with_capacity(tasks.len());
        for task in tasks {
            // Report the resolved dir (including any inherited task_config.dir)
            // so consumers see the directory the task will actually run in.
            let resolved_dir = match task.dir(config).await {
                Ok(dir) => dir.map(|p| p.to_string_lossy().to_string()),
                Err(err) => {
                    warn!("{err:#}");
                    None
                }
            };
            let env_strs: Vec<String> = task
                .env
                .0
                .iter()
                .chain(task.overlay_env.iter().map(|(d, _)| d))
                .map(|d| d.to_string())
                .collect();
            array_items.push(json!({
                "name": task.display_name,
                "aliases": task.aliases,
                "description": task.description,
                "source": task.config_source.to_string_lossy(),
                "config_sources": task
                    .config_sources()
                    .iter()
                    .map(|source| source.to_string_lossy())
                    .collect::<Vec<_>>(),
                "depends": task.depends,
                "daemons": task.daemons,
                "secrets": task.secret_names(),
                "depends_post": task.depends_post,
                "wait_for": task.wait_for,
                "env": env_strs,
                "dir": resolved_dir,
                "hide": task.hide,
                "global": task.global,
                "raw": task.raw,
                "interactive": task.interactive,
                "sources": task.sources,
                "outputs": task.outputs,
                "shell": task.shell,
                "quiet": task.quiet,
                "silent": task.silent,
                "tools": task.tools,
                "usage": task.usage,
                "timeout": task.timeout,
                "run": task.run(),
                "args": task.args,
                "file": task.file.as_ref().map(|p| p.to_string_lossy()),
            }));
        }
        miseprintln!("{}", serde_json::to_string_pretty(&array_items)?);
        Ok(())
    }

    fn sort(&self, a: &Task, b: &Task) -> std::cmp::Ordering {
        let cmp = match self.sort.unwrap_or(SortColumn::Name) {
            SortColumn::Alias => a.aliases.join(", ").cmp(&b.aliases.join(", ")),
            SortColumn::Description => a.description.cmp(&b.description),
            SortColumn::Source => a.config_source.cmp(&b.config_source),
            _ => a.name.cmp(&b.name),
        };

        match self.sort_order.unwrap_or(SortOrder::Asc) {
            SortOrder::Desc => cmp.reverse(),
            _ => cmp,
        }
    }

    fn task_to_row(&self, task: &Task) -> Row {
        let mut row = vec![Cell::new(&task.display_name).add_attribute(Attribute::Bold)];
        if self.extended {
            row.push(Cell::new(task.aliases.join(", ")));
            row.push(Cell::new(
                task.config_sources()
                    .iter()
                    .map(display_rel_path)
                    .join(", "),
            ));
        }
        row.push(Cell::new(&task.description).add_attribute(Attribute::Dim));
        row.into()
    }
}

/// Strip from `cmd` everything completing one of its words would execute: `mount`,
/// `complete run=`, `complete delegate=` and `choices run=`, through every subcommand and
/// clause. A task that loaded without trust is inert until `mise run`, and pressing Tab is not
/// that. Names, help, flags and declared choices stay, so the task still completes.
fn remove_spec_commands(cmd: &mut usage::SpecCommand) {
    let executes = |c: &usage::SpecComplete| c.run.is_some() || c.delegate.is_some();
    cmd.mounts.clear();
    cmd.complete.retain(|_, complete| !executes(complete));
    let clause = cmd.clause.as_mut().map(|c| (&mut c.args, &mut c.flags));
    for (args, flags) in std::iter::once((&mut cmd.args, &mut cmd.flags)).chain(clause) {
        let flag_args = flags.iter_mut().filter_map(|flag| flag.arg.as_mut());
        for arg in args.iter_mut().chain(flag_args) {
            arg.complete.take_if(|complete| executes(complete));
            if let Some(choices) = &mut arg.choices
                && choices.run.take().is_some()
            {
                // Without the command the declared values are no longer the whole set, and
                // usage rejects a `choices` that declares nothing at all.
                choices.strict = false;
                if choices.choices.is_empty()
                    && choices.details.is_empty()
                    && choices.env().is_none()
                {
                    arg.choices = None;
                }
            }
        }
    }
    for subcommand in cmd.subcommands.values_mut() {
        remove_spec_commands(subcommand);
    }
}

/// The name prefix of the current config root's tasks, e.g. `//:` or `//apps/web:`, which
/// `:task` stands for. `None` outside a monorepo, where `:task` names nothing.
fn current_root_prefix(config: &Config) -> Option<String> {
    crate::task::expand_colon_task_syntax(":", config).ok()
}

/// `:task` for a task of the current config root.
///
/// It runs the same task as its `//` name, but completion needs it as a candidate of its own:
/// a shell only offers candidates that extend the word being completed, so without one
/// `mise :bu<TAB>` matched nothing and fell back to completing file paths.
fn shorthand_name(current_root: Option<&str>, task: &Task) -> Option<String> {
    let name = task.display_name.strip_prefix(current_root?)?;
    Some(format!(":{name}"))
}

// TODO: fill this out

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r###"<bold><underline>Deprecation:</underline></bold>

The `-l` and `--local` flags are deprecated in favor of `--no-global`. mise warns about
them from 2027.1.0 and removes them in 2027.7.0; `--local` returns in 2027.10.0 as the
nearest project's config alone, as with `mise settings --local`."###
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_spec_commands_leaves_nothing_to_execute() {
        let mut spec: usage::Spec = r#"
cmd "task" {
    mount run="touch mount"
    arg "<named>"
    complete "named" run="touch named"
    arg "<delegated>"
    complete "delegated" delegate="git"
    arg "<typed>"
    complete "typed" type="dir"
    arg "<inline>" {
        complete run="touch inline"
    }
    arg "<only-run>" {
        choices run="touch only-run"
    }
    flag "--flag <value>" {
        choices "declared" run="touch choices"
    }
    cmd "sub" {
        mount run="touch sub-mount"
        clause "step" separator="and" {
            flag "--clause-flag <value>" {
                complete run="touch clause-flag"
            }
            arg "<clause-arg>" {
                choices run="touch clause-arg"
            }
        }
    }
}
"#
        .parse()
        .unwrap();
        remove_spec_commands(&mut spec.cmd);
        let kdl = spec.to_string();
        for executed in ["mount", "run=", "delegate=", "touch"] {
            assert!(!kdl.contains(executed), "{executed} is left in:\n{kdl}");
        }
        for kept in [
            "type=dir",
            "declared",
            "--clause-flag",
            "<clause-arg>",
            "<only-run>",
        ] {
            assert!(kdl.contains(kept), "{kept} is missing from:\n{kdl}");
        }
        kdl.parse::<usage::Spec>().unwrap();
    }
}
