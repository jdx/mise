use crate::config::Config;
use crate::task::Task;
use crate::{dirs, file};
use eyre::Result;
use indoc::formatdoc;
use std::path::MAIN_SEPARATOR_STR;

/// Edit a task with $EDITOR
///
/// The task will be created as a standalone script if it does not already exist.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(super) struct TasksEdit {
    /// Task to edit
    #[usage()]
    task: String,

    /// Display the path to the task instead of editing it
    #[usage(long, short, verbatim_doc_comment)]
    path: bool,
}

impl TasksEdit {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let cwd = dirs::CWD.clone().unwrap_or_default();
        let project_root = config.project_root.clone().unwrap_or(cwd);
        let task = if let Some(task) = config.tasks_with_aliases().await?.get(&self.task).cloned() {
            task
        } else {
            let path = Task::task_dir()
                .await?
                .join(self.task.replace(':', MAIN_SEPARATOR_STR));
            Task::from_path(&config, &path, path.parent().unwrap(), &project_root)
                .await
                .or_else(|_| Task::new(&path, path.parent().unwrap(), &project_root))?
        };
        let file = &task.config_source;
        if !file.exists() {
            file::create_dir_all(file.parent().unwrap())?;
            file::write(file, default_task())?;
            file::make_executable(file)?;
        }
        if self.path {
            miseprintln!("{}", file.display());
        } else {
            crate::cli::editor::open_in_editor(file.as_path())?;
        }

        Ok(())
    }
}

fn default_task() -> String {
    formatdoc!(
        r#"#!/usr/bin/env bash
        set -euxo pipefail

        "#
    )
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise tasks edit build</bold>
    $ <bold>mise tasks edit test</bold>
"#
);
