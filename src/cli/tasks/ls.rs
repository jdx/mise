use console::truncate_str;
use eyre::Result;
use itertools::Itertools;
use tabled::Tabled;

use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::task::Task;
use crate::ui::{style, table};

/// [experimental] List available tasks to execute
/// These may be included from the config file or from the project's .mise/tasks directory
/// mise will merge all tasks from all parent directories into this list.
///
/// So if you have global tasks in ~/.config/mise/tasks/* and project-specific tasks in
/// ~/myproject/.mise/tasks/*, then they'll both be available but the project-specific
/// tasks will override the global ones if they have the same name.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TasksLs {
    /// Do not print table header
    #[clap(long, alias = "no-headers", verbatim_doc_comment)]
    pub no_header: bool,

    /// Show all columns
    #[clap(short = 'x', long, verbatim_doc_comment)]
    pub extended: bool,

    /// Show hidden tasks
    #[clap(long, verbatim_doc_comment)]
    pub hidden: bool,

    /// Sort by column. Default is name.
    #[clap(long, value_name = "COLUMN", verbatim_doc_comment)]
    pub sort: Option<SortColumn>,

    /// Sort order. Default is asc.
    #[clap(long, verbatim_doc_comment)]
    pub sort_order: Option<SortOrder>,
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
        let settings = Settings::try_get()?;
        settings.ensure_experimental("`mise tasks ls`")?;
        let rows = config
            .tasks()?
            .values()
            .filter(|t| self.hidden || !t.hide)
            .sorted_by(|a, b| self.sort(a, b))
            .map(|t| t.into())
            .collect::<Vec<Row>>();

        let mut table = tabled::Table::new(rows);
        table::default_style(&mut table, self.no_header);
        // hide columns alias
        if !self.extended {
            table::disable_columns(&mut table, vec![1]);
        }
        miseprintln!("{table}");

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
}

#[derive(Tabled)]
#[tabled(rename_all = "PascalCase")]
struct Row {
    name: String,
    alias: String,
    description: String,
    // command: String,
    source: String,
}

impl From<&Task> for Row {
    fn from(task: &Task) -> Self {
        // let cmd = tasks.command_string().unwrap_or_default();
        Self {
            name: style::nbold(&task.name).bright().to_string(),
            alias: style::ndim(&task.aliases.join(", ")).dim().to_string(),
            description: style::nblue(truncate(&task.description, 40)).to_string(),
            // command: style::ndim(truncate(&cmd, 20)).dim().to_string(),
            source: display_path(&task.config_source),
        }
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or_default()
}

fn truncate(s: &str, len: usize) -> String {
    first_line(&truncate_str(s, len, "…")).to_string()
}

// TODO: fill this out
static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
    
    $ <bold>mise tasks ls</bold>
"#
);

#[cfg(test)]
mod tests {
    #[test]
    fn test_task_ls() {
        assert_cli_snapshot!("t", "--no-headers", @r###"
        configtask                               ~/config/config.toml       
        filetask    This is a test build script  ~/cwd/.mise/tasks/filetask 
        lint                                     ~/config/config.toml       
        test                                     ~/config/config.toml
        "###);
    }
}
