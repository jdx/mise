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

    /// Explain provider attribution for inferred projects and tasks
    #[clap(long, conflicts_with = "json", verbatim_doc_comment)]
    explain: bool,

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
        } else if self.explain {
            self.display_explain(&graph)
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

    fn display_explain(&self, graph: &WorkspaceProjectGraph) -> Result<()> {
        for (index, project) in graph.projects().enumerate() {
            if index > 0 {
                miseprintln!();
            }
            miseprintln!("Project: {}", project.id);
            print_provenance("  ", &project.provenance)?;
            if !project.dependencies.is_empty() {
                miseprintln!("  Dependencies:");
                for dependency in &project.dependencies {
                    miseprint!("    {dependency}")?;
                    print_inline_provenance(project.dependency_provenance.get(dependency))?;
                }
            }
            if !project.tasks.is_empty() {
                miseprintln!("  Tasks:");
                for (name, task) in &project.tasks {
                    miseprintln!("    {name}");
                    print_provenance("      ", &task.provenance)?;
                    let suggestions = &task.suggestions.provenance;
                    for input in &task.suggestions.inputs {
                        miseprint!("      Input: {input}")?;
                        print_inline_provenance(suggestions.inputs.as_ref())?;
                    }
                    if let Some(outputs) = &task.suggestions.outputs {
                        if outputs.is_empty() {
                            miseprint!("      Outputs: no files")?;
                            print_inline_provenance(suggestions.outputs.as_ref())?;
                        } else {
                            for output in outputs {
                                miseprint!("      Output: {output}")?;
                                print_inline_provenance(suggestions.outputs.as_ref())?;
                            }
                        }
                    }
                    if let Some(cache) = task.suggestions.cache {
                        miseprint!(
                            "      Cache: {}",
                            if cache { "enabled" } else { "disabled" }
                        )?;
                        print_inline_provenance(suggestions.cache.as_ref())?;
                    }
                    if let Some(dependencies) = &task.suggestions.depends {
                        for dependency in dependencies {
                            miseprint!("      Task dependency: {dependency}")?;
                            print_inline_provenance(suggestions.depends.as_ref())?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn print_provenance(
    indent: &str,
    provenance: &crate::task::workspace::WorkspaceProvenance,
) -> Result<()> {
    miseprintln!(
        "{indent}Provider: {}",
        provenance.provider.as_deref().unwrap_or("configuration")
    );
    if let Some(source) = &provenance.source {
        miseprintln!("{indent}Source: {}", source.display());
    }
    Ok(())
}

fn print_inline_provenance(
    provenance: Option<&crate::task::workspace::WorkspaceProvenance>,
) -> Result<()> {
    let provider = provenance
        .and_then(|provenance| provenance.provider.as_deref())
        .unwrap_or("configuration");
    if let Some(source) = provenance.and_then(|provenance| provenance.source.as_ref()) {
        miseprintln!(" — {provider} ({})", source.display());
    } else {
        miseprintln!(" — {provider}");
    }
    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Inspect projects and their dependency edges
    $ <bold>mise tasks graph</bold>

    # Emit the project graph as JSON
    $ <bold>mise tasks graph --json</bold>

    # Explain where inferred projects and task fields came from
    $ <bold>mise tasks graph --explain</bold>
"#
);
