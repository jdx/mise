use crate::config::Config;
use crate::{dirs, file};
use std::path::PathBuf;

use crate::config;

/// Generate documentation for tasks in a project
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TaskDocs {
    /// write only an index of tasks, intended for use with `--multi`
    #[clap(long, short = 'I', verbatim_doc_comment)]
    index: bool,
    /// inserts the documentation into an existing file
    ///
    /// This will look for a special comment, <!-- mise-tasks -->, and replace it with the generated documentation.
    /// It will replace everything between the comment and the next comment, <!-- /mise-tasks --> so it can be
    /// run multiple times on the same file to update the documentation.
    #[clap(long, short, verbatim_doc_comment)]
    inject: bool,
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
        let ts = config.get_toolset().await?;
        let dir = dirs::CWD.as_ref().unwrap();
        let tasks = config::load_tasks_in_dir(&config, dir, &config.config_files).await?;
        let mut out = vec![];
        for task in tasks.iter().filter(|t| !t.hide) {
            out.push(task.render_markdown(&config, ts, dir).await?);
        }
        if let Some(output) = &self.output {
            if self.multi {
                if output.is_dir() {
                    for (i, task) in tasks.iter().filter(|t| !t.hide).enumerate() {
                        let path = output.join(format!("{i}.md"));
                        file::write(&path, &task.render_markdown(&config, ts, dir).await?)?;
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
                doc = format!("{}\n", doc.trim());
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
