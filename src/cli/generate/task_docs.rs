use crate::config::{self, Config};
use crate::{dirs, file};
use color_eyre::eyre::bail;
use std::path::{Path, PathBuf};

const TASK_PLACEHOLDER_START: &str = "<!-- mise-tasks -->";
const TASK_PLACEHOLDER_END: &str = "<!-- /mise-tasks -->";

/// Generate documentation for tasks in a project
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TaskDocs {
    /// inserts the documentation into an existing file
    ///
    /// This will look for a special comment, `<!-- mise-tasks -->`, and replace it with the generated documentation.
    /// It will replace everything between the comment and the next comment, `<!-- /mise-tasks -->` so it can be
    /// run multiple times on the same file to update the documentation.
    /// The file must already contain both comments; mise errors instead of modifying the file if they are missing.
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
        let templates = config
            .config_files
            .values()
            .rev()
            .flat_map(|cf| cf.task_templates())
            .collect();
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
                    let contents = file::read_to_string(output)?;
                    let contents = inject_task_docs(&contents, &doc, output)?;
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

/// Replace everything between the mise-tasks markers in `contents` with `doc`.
///
/// Both markers must be present, with the end marker after the start marker. When
/// a marker is missing this errors instead of writing anything: falling back to
/// "the block starts at byte 0" truncated files that had no markers at all, and
/// panicked outright on files shorter than the marker (discussions/4676).
fn inject_task_docs(contents: &str, doc: &str, output: &Path) -> eyre::Result<String> {
    let Some(start) = contents.find(TASK_PLACEHOLDER_START) else {
        bail!(
            "{} does not contain the `{TASK_PLACEHOLDER_START}` marker required by --inject, add:\n\n{TASK_PLACEHOLDER_START}\n{TASK_PLACEHOLDER_END}",
            file::display_path(output)
        );
    };
    let body_start = start + TASK_PLACEHOLDER_START.len();
    let Some(end) = contents[body_start..]
        .find(TASK_PLACEHOLDER_END)
        .map(|e| e + body_start)
    else {
        bail!(
            "{} contains `{TASK_PLACEHOLDER_START}` but no `{TASK_PLACEHOLDER_END}` after it, --inject requires both markers",
            file::display_path(output)
        );
    };
    let mut contents = contents.to_string();
    contents.replace_range(body_start..end, doc);
    Ok(contents)
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate task-docs</bold>
"#
);

#[cfg(test)]
mod tests {
    use super::{TASK_PLACEHOLDER_END, TASK_PLACEHOLDER_START, inject_task_docs};
    use std::path::Path;

    fn inject(contents: &str) -> eyre::Result<String> {
        inject_task_docs(contents, "\n## `task`\n", Path::new("README.md"))
    }

    #[test]
    fn injects_between_markers() {
        let contents = format!(
            "# Title\n\n{TASK_PLACEHOLDER_START}\nold\n{TASK_PLACEHOLDER_END}\n\ntrailer\n"
        );
        assert_eq!(
            inject(&contents).unwrap(),
            format!(
                "# Title\n\n{TASK_PLACEHOLDER_START}\n## `task`\n{TASK_PLACEHOLDER_END}\n\ntrailer\n"
            )
        );
    }

    #[test]
    fn errors_without_start_marker() {
        // used to keep the first 19 bytes and replace the rest of the file
        let err = inject("# My Project\n\nImportant paragraph that must survive.\n").unwrap_err();
        assert!(err.to_string().contains(TASK_PLACEHOLDER_START));
    }

    #[test]
    fn errors_on_file_shorter_than_marker() {
        // used to panic: range start index 19 out of range for slice of length 3
        assert!(inject("hi\n").is_err());
    }

    #[test]
    fn errors_when_byte_offset_is_not_a_char_boundary() {
        // multibyte content: the old code sliced at byte 19 regardless
        assert!(inject("日本語のドキュメント\n").is_err());
    }

    #[test]
    fn errors_without_end_marker() {
        let contents = format!("# Title\n\n{TASK_PLACEHOLDER_START}\nold\n");
        let err = inject(&contents).unwrap_err();
        assert!(err.to_string().contains(TASK_PLACEHOLDER_END));
    }

    #[test]
    fn errors_when_end_marker_precedes_start_marker() {
        let contents = format!("{TASK_PLACEHOLDER_END}\nold\n{TASK_PLACEHOLDER_START}\n");
        assert!(inject(&contents).is_err());
    }
}
