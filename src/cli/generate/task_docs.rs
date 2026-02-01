use crate::config::{self, Config, Settings};
use crate::{dirs, file};
use indexmap::IndexMap;
use std::path::PathBuf;

/// Generate documentation for tasks in a project
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TaskDocs {
    /// inserts the documentation into an existing file
    ///
    /// This will look for a special comment, `<!-- mise-tasks -->`, and replace it with the generated documentation.
    /// It will replace everything between the comment and the next comment, `<!-- /mise-tasks -->` so it can be
    /// run multiple times on the same file to update the documentation.
    #[clap(long, short, verbatim_doc_comment)]
    inject: bool,
    /// write only an index of tasks, intended for use with `--multi`
    #[clap(long, short = 'I', verbatim_doc_comment)]
    index: bool,
    /// render each task as a separate document, requires `--output` to be a directory
    #[clap(long, short, verbatim_doc_comment)]
    multi: bool,
    /// writes the generated docs to a file/directory
    #[clap(long, short, verbatim_doc_comment)]
    output: Option<PathBuf>,
    /// root directory to search for tasks
    #[clap(long, short, verbatim_doc_comment, value_hint = clap::ValueHint::DirPath)]
    root: Option<PathBuf>,
    #[clap(long, short, verbatim_doc_comment, value_enum, default_value_t)]
    style: TaskDocsStyle,
}

#[derive(Debug, Default, Clone, clap::ValueEnum)]
enum TaskDocsStyle {
    #[default]
    #[value()]
    Simple,
    #[value()]
    Detailed,
}

impl TaskDocs {
    pub async fn run(self) -> eyre::Result<()> {
        let config = Config::get().await?;
        let dir = dirs::CWD.as_ref().unwrap();
        // Collect task templates from config hierarchy
        let templates = if Settings::get().experimental {
            config
                .config_files
                .values()
                .rev()
                .flat_map(|cf| cf.task_templates())
                .collect()
        } else {
            IndexMap::new()
        };
        let tasks =
            config::load_tasks_in_dir(&config, dir, &config.config_files, &templates).await?;
        let visible_tasks: Vec<_> = tasks.iter().filter(|t| !t.hide).collect();
        if let Some(output) = &self.output {
            if self.multi {
                if output.is_dir() {
                    let mut index = if self.index {
                        Some(String::from("# Tasks\n\n"))
                    } else {
                        None
                    };
                    for task in &visible_tasks {
                        let filename = format!("{}.md", task.name.replace([':', '/'], "-"));
                        file::write(
                            output.join(&filename),
                            &task.render_markdown(&config).await?,
                        )?;
                        if let Some(index) = &mut index {
                            let desc = if task.description.is_empty() {
                                String::new()
                            } else {
                                format!(" - {}", task.description)
                            };
                            index.push_str(&format!("- [{}](./{filename}){desc}\n", task.name));
                        }
                    }
                    if let Some(index) = index {
                        if visible_tasks
                            .iter()
                            .any(|t| t.name.replace([':', '/'], "-") == "index")
                        {
                            warn!("task named \"index\" will be overwritten by index.md");
                        }
                        file::write(output.join("index.md"), &index)?;
                    }
                } else {
                    return Err(eyre::eyre!(
                        "`--output` must be a directory when `--multi` is set"
                    ));
                }
            } else {
                let mut out = vec![];
                for task in &visible_tasks {
                    out.push(task.render_markdown(&config).await?);
                }
                let mut doc = String::new();
                for task in out {
                    doc.push_str(&task);
                    doc.push_str("\n\n");
                }
                if self.inject {
                    doc = format!("\n{}\n", doc.trim());
                    let mut contents = file::read_to_string(output)?;
                    let task_placeholder_start = "<!-- mise-tasks -->";
                    let task_placeholder_end = "<!-- /mise-tasks -->";
                    let start = contents.find(task_placeholder_start).unwrap_or(0);
                    let end = contents[start..]
                        .find(task_placeholder_end)
                        .map(|e| e + start)
                        .unwrap_or(contents.len());
                    contents.replace_range((start + task_placeholder_start.len())..end, &doc);
                    file::write(output, &contents)?;
                } else {
                    doc = format!("{}\n", doc.trim());
                    file::write(output, &doc)?;
                }
            }
        } else {
            let mut out = vec![];
            for task in &visible_tasks {
                out.push(task.render_markdown(&config).await?);
            }
            miseprintln!("{}", out.join("\n\n").trim());
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate task-docs</bold>
"#
);
