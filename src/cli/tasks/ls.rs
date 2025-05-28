use std::sync::Arc;

use crate::config::Config;
use crate::file::display_rel_path;
use crate::task::Task;
use crate::toolset::Toolset;
use crate::ui::table::MiseTable;
use comfy_table::{Attribute, Cell, Row};
use eyre::Result;
use itertools::Itertools;
use serde_json::json;

/// List available tasks to execute
/// These may be included from the config file or from the project's .mise/tasks directory
/// mise will merge all tasks from all parent directories into this list.
///
/// So if you have global tasks in `~/.config/mise/tasks/*` and project-specific tasks in
/// ~/myproject/.mise/tasks/*, then they'll both be available but the project-specific
/// tasks will override the global ones if they have the same name.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TasksLs {
    /// Display tasks for usage completion
    #[clap(long, hide = true)]
    pub complete: bool,

    /// Show all columns
    #[clap(short = 'x', long, global = true, verbatim_doc_comment)]
    pub extended: bool,

    /// Do not print table header
    #[clap(long, alias = "no-headers", global = true, verbatim_doc_comment)]
    pub no_header: bool,

    /// Show hidden tasks
    #[clap(long, global = true, verbatim_doc_comment)]
    pub hidden: bool,

    /// Only show global tasks
    #[clap(
        short,
        long,
        global = true,
        overrides_with = "local",
        verbatim_doc_comment
    )]
    pub global: bool,

    /// Output in JSON format
    #[clap(short = 'J', global = true, long, verbatim_doc_comment)]
    pub json: bool,

    /// Only show non-global tasks
    #[clap(
        short,
        long,
        global = true,
        overrides_with = "global",
        verbatim_doc_comment
    )]
    pub local: bool,

    /// Sort by column. Default is name.
    #[clap(long, global = true, value_name = "COLUMN", verbatim_doc_comment)]
    pub sort: Option<SortColumn>,

    /// Sort order. Default is asc.
    #[clap(long, global = true, verbatim_doc_comment)]
    pub sort_order: Option<SortOrder>,

    #[clap(long, global = true, hide = true)]
    pub usage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SortColumn {
    Name,
    Alias,
    Description,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl TasksLs {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let ts = config.get_toolset().await?;
        let tasks = config
            .tasks()
            .await?
            .values()
            .filter(|t| self.hidden || !t.hide)
            .filter(|t| !self.local || !t.global)
            .filter(|t| !self.global || t.global)
            .cloned()
            .sorted_by(|a, b| self.sort(a, b))
            .collect::<Vec<Task>>();

        if self.complete {
            return self.complete(tasks);
        } else if self.usage {
            self.display_usage(&config, ts, tasks).await?;
        } else if self.json {
            self.display_json(tasks)?;
        } else {
            self.display(tasks)?;
        }
        Ok(())
    }

    fn complete(&self, tasks: Vec<Task>) -> Result<()> {
        for t in tasks {
            let name = t.display_name.replace(":", "\\:");
            let description = t.description.replace(":", "\\:");
            println!("{name}:{description}",);
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

    async fn display_usage(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
        tasks: Vec<Task>,
    ) -> Result<()> {
        let mut usage = usage::Spec::default();
        for task in tasks {
            let env = task.render_env(config, ts).await?;
            let (mut task_spec, _) = task.parse_usage_spec(config, None, &env).await?;
            for (name, complete) in task_spec.complete {
                task_spec.cmd.complete.insert(name, complete);
            }
            usage
                .cmd
                .subcommands
                .insert(task.display_name.clone(), task_spec.cmd);
        }
        miseprintln!("{}", usage.to_string());
        Ok(())
    }

    fn display_json(&self, tasks: Vec<Task>) -> Result<()> {
        let array_items = tasks
            .into_iter()
            .map(|task| {
                json!({
                  "name": task.display_name,
                  "aliases": task.aliases,
                  "description": task.description,
                  "source": task.config_source,
                  "depends": task.depends,
                  "depends_post": task.depends_post,
                  "wait_for": task.wait_for,
                  "env": task.env,
                  "dir": task.dir,
                  "hide": task.hide,
                  "raw": task.raw,
                  "sources": task.sources,
                  "outputs": task.outputs,
                  "shell": task.shell,
                  "quiet": task.quiet,
                  "silent": task.silent,
                  "tools": task.tools,
                  "run": task.run(),
                  "file": task.file,
                })
            })
            .collect::<serde_json::Value>();
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
            row.push(Cell::new(display_rel_path(&task.config_source)));
        }
        row.push(Cell::new(&task.description).add_attribute(Attribute::Dim));
        row.into()
    }
}

// TODO: fill this out
static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise tasks ls</bold>
"#
);
