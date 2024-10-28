use eyre::Result;

use crate::config::Config;
use crate::task::Task;
use crate::{env, file};

/// Edit a tasks with $EDITOR
///
/// The tasks will be created as a standalone script if it does not already exist.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TasksEdit {
    /// Tasks to edit
    #[clap()]
    task: String,

    /// Display the path to the tasks instead of editing it
    #[clap(long, short, verbatim_doc_comment)]
    path: bool,
}

impl TasksEdit {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;

        let task = config
            .tasks_with_aliases()?
            .remove(&self.task)
            .cloned()
            .map_or_else(
                || {
                    let path = config
                        .project_root
                        .as_ref()
                        .unwrap_or(&env::current_dir()?)
                        .join(".mise")
                        .join("tasks")
                        .join(&self.task);
                    Task::from_path(&path)
                },
                Ok,
            )?;
        let file = &task.config_source;
        if !file.exists() {
            file::create(file)?;
            file::make_executable(file)?;
        }
        if self.path {
            miseprintln!("{}", file.display());
        } else {
            cmd!(&*env::EDITOR, &file).run()?;
        }

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise tasks edit build</bold>
    $ <bold>mise tasks edit test</bold>
"#
);
