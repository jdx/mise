use crate::config::{Config, config_file};
use crate::task::Task;
use crate::{dirs, file};
use eyre::Result;
use indoc::formatdoc;
use std::path::MAIN_SEPARATOR_STR;

/// Open a task in your editor
///
/// Opens the file that defines the task in `$VISUAL` or `$EDITOR`: the script
/// for a file task, or the config file for a TOML task. If no task has that
/// name, mise creates an executable bash script for it in the project's task
/// directory (for example `mise-tasks/`) and opens that. `--path` prints the
/// file's path instead of opening it; a missing task is still created.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise tasks edit build", help = "Edit the build task"),
    example(
        "mise tasks edit --path build",
        help = "Print the build task's file path"
    )
)]
pub(super) struct TasksEdit {
    /// Task to edit
    #[usage()]
    task: String,

    /// Print the task's file path instead of opening it
    #[usage(long, short)]
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
            // Only the file's path is needed. A file discovery skipped (not executable, a dotfile)
            // was never trust-checked, so its `#MISE` headers are not parsed or rendered here.
            Task::new(&path, path.parent().unwrap(), &project_root)?
        };
        let file = &task.config_source;
        let create = !file.exists();
        // A file the editor will write is checked like one mise writes, also when it exists: the
        // editor follows its links, and the check refuses one out of the project. A script is
        // checked as `mise tasks add --file` checks it, a TOML file as config writes are. The
        // editor then opens the destination the check approved, not the links again.
        let destination = if create || (!self.path && !super::write::is_toml(file)) {
            super::write::script_check_before_write(file).await?
        } else if !self.path {
            config_file::checked_write_destination(file)?
        } else {
            file.clone()
        };
        if create {
            if let Some(parent) = destination.parent() {
                file::create_dir_all(parent)?;
            }
            file::write(&destination, default_task())?;
            file::make_executable(&destination)?;
        }
        if self.path {
            miseprintln!("{}", file.display());
        } else {
            crate::cli::editor::open_in_editor(&destination)?;
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
