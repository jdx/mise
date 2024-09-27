use crate::config::settings::SETTINGS;
use crate::config::CONFIG;
use crate::{dirs, file};
use std::path::PathBuf;

/// [experimental] Generate documentation for tasks in a project
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TaskDocs {
    /// render each task as a separate document, requires `--output` to be a directory
    #[clap(long, short, verbatim_doc_comment)]
    multi: bool,
    /// inserts the documentation into an existing file
    ///
    /// This will look for a special comment, <!-- mise-tasks -->, and replace it with the generated documentation.
    /// It will replace everything between the comment and the next comment, <!-- /mise-tasks --> so it can be
    /// run multiple times on the same file to update the documentation.
    #[clap(long, short, verbatim_doc_comment)]
    inject: bool,
    /// write only an index of tasks, intended for use with `--multi`
    #[clap(long, short = 'I', verbatim_doc_comment)]
    index: bool,
    /// writes the generated docs to a file/directory
    #[clap(long, short, verbatim_doc_comment)]
    output: Option<PathBuf>,
}

impl TaskDocs {
    pub fn run(self) -> eyre::Result<()> {
        SETTINGS.ensure_experimental("generate task-docs")?;
        let tasks = CONFIG.load_tasks_in_dir(dirs::CWD.as_ref().unwrap())?;
        let mut out = vec![];
        for task in &tasks {
            out.push(task.render_markdown()?);
        }
        if let Some(output) = &self.output {
            if self.multi {
                if output.is_dir() {
                    for (i, task) in tasks.iter().enumerate() {
                        let path = output.join(format!("task-{}.md", i));
                        file::write(&path, &task.render_markdown()?)?;
                    }
                } else {
                    return Err(eyre::eyre!(
                        "`--output` must be a directory when `--multi` is set"
                    ));
                }
            } else {
                let mut doc = String::new();
                for task in out {
                    doc.push_str(&task);
                    doc.push_str("\n\n");
                }
                if self.inject {
                    let mut contents = file::read_to_string(output)?;
                    let start = contents.find("<!-- mise-tasks -->").unwrap_or(0);
                    let end = contents[start..]
                        .find("<!-- /mise-tasks -->")
                        .unwrap_or(contents.len());
                    contents.replace_range(start..end, &doc);
                    file::write(output, &contents)?;
                } else {
                    file::write(output, &doc)?;
                }
            }
        } else {
            for task in out {
                miseprintln!("{}", task);
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate task-docs</bold>
"#
);

#[cfg(test)]
mod tests {
    use test_log::test;

    use crate::test::{cleanup, reset, setup_git_repo};

    #[test]
    fn test_task_docs() {
        reset();
        setup_git_repo();
        assert_cli_snapshot!("generate", "task-docs");
        cleanup();
    }
}
