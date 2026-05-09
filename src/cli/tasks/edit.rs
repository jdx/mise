use crate::config::Config;
use crate::task::Task;
use crate::{dirs, env, file};
use eyre::{Result, eyre};
use indoc::formatdoc;
use std::path::MAIN_SEPARATOR_STR;

/// Edit a task with $EDITOR
///
/// The task will be created as a standalone script if it does not already exist.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TasksEdit {
    /// Task to edit
    #[clap()]
    task: String,

    /// Display the path to the task instead of editing it
    #[clap(long, short, verbatim_doc_comment)]
    path: bool,
}

impl TasksEdit {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let cwd = dirs::CWD.clone().unwrap_or_default();
        let project_root = config.project_root.clone().unwrap_or(cwd);
        let path = Task::task_dir()
            .await
            .join(self.task.replace(':', MAIN_SEPARATOR_STR));

        let task = if let Some(task) = config.tasks_with_aliases().await?.get(&self.task).cloned() {
            task
        } else {
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
            open_in_editor(file.as_path())?;
        }

        Ok(())
    }
}

fn open_in_editor(file: &std::path::Path) -> Result<()> {
    let (program, mut args) = split_editor_command(&env::EDITOR)?;
    args.push(file.as_os_str().into());

    crate::cmd::cmd(&program, args).run()?;
    Ok(())
}

fn split_editor_command(editor: &str) -> Result<(String, Vec<std::ffi::OsString>)> {
    let mut parts = shell_words::split(editor)
        .map_err(|e| eyre!("failed to parse EDITOR/VISUAL value {:?}: {}", editor, e))?
        .into_iter();
    let program = parts
        .next()
        .ok_or_else(|| eyre!("EDITOR/VISUAL is empty"))?;

    Ok((program, parts.map(Into::into).collect()))
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

#[cfg(test)]
mod tests {
    use super::split_editor_command;

    #[test]
    fn parses_editor_with_arguments() {
        let (program, args) = split_editor_command("cat -n").unwrap();

        assert_eq!(program, "cat");
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].as_os_str(), std::ffi::OsStr::new("-n"));
    }

    #[test]
    fn parses_editor_with_quoted_path() {
        let (program, args) =
            split_editor_command(r#""/Applications/My Editor.app/editor" --wait"#).unwrap();

        assert_eq!(program, "/Applications/My Editor.app/editor");
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], "--wait");
    }

    #[test]
    fn errors_on_empty_editor() {
        assert!(split_editor_command("").is_err());
    }
}
