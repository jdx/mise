use eyre::Result;

use crate::config::{Config, Settings};
use crate::task::Task;
use crate::{env, file};

/// [experimental] Edit a task with $EDITOR
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct TaskEdit {
    /// Task to edit
    #[clap()]
    task: String,
}

impl TaskEdit {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::try_get()?;
        settings.ensure_experimental()?;

        let task = config.tasks().get(&self.task).cloned().map_or_else(
            || {
                let path = config
                    .project_root
                    .as_ref()
                    .unwrap_or(&env::PWD)
                    .join(".rtx")
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
        cmd!(&*env::EDITOR, &file).run()?;

        Ok(())
    }
}
