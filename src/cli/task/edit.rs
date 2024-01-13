use eyre::Result;

use crate::config::{Config, Settings};
use crate::task::Task;
use crate::{env, file};

/// [experimental] Edit a task with $EDITOR
///
/// The task will be created as a standalone script if it does not already exist.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TaskEdit {
    /// Task to edit
    #[clap()]
    task: String,

    /// Display the path to the task instead of editing it
    #[clap(long, short, verbatim_doc_comment)]
    path: bool,
}

impl TaskEdit {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::try_get()?;
        settings.ensure_experimental()?;

        let task = config
            .tasks_with_aliases()
            .get(&self.task)
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
                    Task::from_path(path)
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
  $ <bold>mise task edit build</bold>
  $ <bold>mise task edit test</bold>
"#
);
