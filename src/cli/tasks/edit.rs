use crate::config::Config;
use crate::task::Task;
use crate::{dirs, env, file};
use eyre::Result;
use indoc::formatdoc;

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
        let cwd = dirs::CWD.clone().unwrap_or_default();
        let project_root = config.project_root.clone().unwrap_or(cwd);
        let path = Task::task_dir().join(&self.task);

        let task = config
            .tasks_with_aliases()?
            .remove(&self.task)
            .cloned()
            .or_else(|| Task::from_path(&path, path.parent().unwrap(), &project_root).ok())
            .map_or_else(
                || Task::new(&path, path.parent().unwrap(), &project_root),
                Ok,
            )?;
        let file = &task.config_source;
        if !file.exists() {
            file::write(file, default_task())?;
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
