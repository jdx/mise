use comfy_table::{Attribute, Cell, Row};
use eyre::Result;
use itertools::Itertools;

use crate::config::Config;
use crate::file::{display_path, display_rel_path};
use crate::task::Task;
use crate::ui::table::MiseTable;

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
    /// Do not print table header
    #[clap(long, alias = "no-headers", global = true, verbatim_doc_comment)]
    pub no_header: bool,

    /// Show all columns
    #[clap(short = 'x', long, global = true, verbatim_doc_comment)]
    pub extended: bool,

    /// Show hidden tasks
    #[clap(long, global = true, verbatim_doc_comment)]
    pub hidden: bool,

    /// Sort by column. Default is name.
    #[clap(long, global = true, value_name = "COLUMN", verbatim_doc_comment)]
    pub sort: Option<SortColumn>,

    /// Sort order. Default is asc.
    #[clap(long, global = true, verbatim_doc_comment)]
    pub sort_order: Option<SortOrder>,

    /// Output in JSON format
    #[clap(short = 'J', global = true, long, verbatim_doc_comment)]
    pub json: bool,

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
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let tasks = config
            .tasks()?
            .values()
            .filter(|t| self.hidden || !t.hide)
            .cloned()
            .sorted_by(|a, b| self.sort(a, b))
            .collect::<Vec<Task>>();

        if self.usage {
            self.display_usage(tasks)?;
        } else if self.json {
            self.display_json(tasks)?;
        } else {
            self.display(tasks)?;
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

    fn display_usage(&self, tasks: Vec<Task>) -> Result<()> {
        let mut usage = usage::Spec::default();
        for task in tasks {
            let (mut task_spec, _) = task.parse_usage_spec(None)?;
            for (name, complete) in task_spec.complete {
                task_spec.cmd.complete.insert(name, complete);
            }
            usage
                .cmd
                .subcommands
                .insert(task.name.clone(), task_spec.cmd);
        }
        miseprintln!("{}", usage.to_string());
        Ok(())
    }

    fn display_json(&self, tasks: Vec<Task>) -> Result<()> {
        let array_items = tasks
            .into_iter()
            .filter(|t| self.hidden || !t.hide)
            .map(|task| {
                let mut inner = serde_json::Map::new();
                inner.insert("name".to_string(), task.display_name().into());
                if !task.aliases.is_empty() {
                    inner.insert("aliases".to_string(), task.aliases.join(", ").into());
                }
                if task.hide {
                    inner.insert("hide".to_string(), task.hide.into());
                }
                inner.insert("description".to_string(), task.description.into());
                inner.insert(
                    "source".to_string(),
                    display_path(task.config_source).into(),
                );
                inner
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
        let mut row = vec![Cell::new(task.display_name()).add_attribute(Attribute::Bold)];
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
