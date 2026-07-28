use crate::config::{Config, Settings};
use crate::task::workspace::{WorkspaceProject, WorkspaceProjectGraph};
use crate::ui::table::MiseTable;
use comfy_table::{Cell, Row};
use eyre::Result;
use serde::Serialize;

/// [experimental] Inspect the workspace project graph
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TasksGraph {
    /// Output the project graph as JSON
    #[clap(short = 'J', long, verbatim_doc_comment)]
    json: bool,

    /// Do not print table headers
    #[clap(long, alias = "no-headers", verbatim_doc_comment)]
    no_header: bool,
}

#[derive(Serialize)]
struct ProjectGraphOutput<'a> {
    projects: Vec<&'a WorkspaceProject>,
}

impl TasksGraph {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("workspace project graph")?;
        let config = Config::get().await?;
        let graph = config.workspace_project_graph()?;

        if self.json {
            self.display_json(&graph)
        } else {
            self.display(&graph)
        }
    }

    fn display(&self, graph: &WorkspaceProjectGraph) -> Result<()> {
        let mut table = MiseTable::new(
            self.no_header,
            &["Project", "Root", "Dependencies", "Metadata"],
        );
        for project in graph.projects() {
            let dependencies = project
                .dependencies
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            table.add_row(Row::from(vec![
                Cell::new(&project.id),
                Cell::new(project.root.display()),
                Cell::new(if dependencies.is_empty() {
                    "-"
                } else {
                    &dependencies
                }),
                Cell::new(serde_json::to_string(&project.metadata)?),
            ]));
        }
        table.print()
    }

    fn display_json(&self, graph: &WorkspaceProjectGraph) -> Result<()> {
        let output = ProjectGraphOutput {
            projects: graph.projects().collect(),
        };
        miseprintln!("{}", serde_json::to_string_pretty(&output)?);
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Inspect projects and their dependency edges
    $ <bold>mise tasks graph</bold>

    # Emit the project graph as JSON
    $ <bold>mise tasks graph --json</bold>
"#
);
