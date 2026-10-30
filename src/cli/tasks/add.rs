use crate::file;
use crate::file::display_path;
use crate::task::Task;
use eyre::{Result, bail};
use std::path::MAIN_SEPARATOR_STR;
use toml_edit::{Item, Value};

use super::write::{self, Action, Kind, Report, TargetArgs, TaskFields};

/// Add a task
///
/// Writes a `[tasks.<name>]` table to the nearest project config file, or to
/// the file `--env`, `--global` or `--path` chooses, and trusts that file's
/// config root so mise loads it. See
/// https://mise.jdx.dev/configuration.html#target-file-for-write-operations for
/// how the file is chosen. In safe mode (`MISE_SAFE=1`) it writes without
/// trusting anything.
///
/// A task that already has that name in the file is replaced only with
/// `--force`. Adding exactly the same task again changes nothing. To change
/// some fields of an existing task, use `mise tasks set`.
///
/// With `--file`, creates an executable script in the project's task directory
/// (for example `mise-tasks/`) instead, and does not change any config file.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        "mise tasks add build --description \"Build the app\" --sources 'src/**/*.ts' --outputs 'dist/**' -- npm run build",
        help = "Add a TOML task that is skipped while its outputs are up to date"
    ),
    example(
        "mise tasks add --env local lint --tools node@24 -- npx eslint .",
        help = "Add a personal task to mise.local.toml"
    ),
    example(
        "mise tasks add --file lint -- eslint .",
        help = "Create an executable lint script in the task directory"
    ),
    example(
        "mise tasks add ci --depends lint --depends test -- echo done",
        help = "Add a task that runs after lint and test"
    ),
    example(
        "mise tasks add --json --dry-run deploy --confirm \"Deploy to production?\" -- ./deploy.sh",
        help = "Print the task that would be written, as JSON, without writing it"
    )
)]
pub(super) struct TasksAdd {
    /// Name of the task to add
    #[usage()]
    task: String,

    /// Command to run, given after `--`
    #[usage(double_dash = "required")]
    run: Vec<String>,

    /// Create a file task instead of a TOML task
    #[usage(long, short)]
    file: bool,
    /// Replace a task that already has this name in the file
    #[usage(long, short = 'F')]
    force: bool,
    /// Show what would be written without writing it
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Print what was written as JSON
    #[usage(long, short = 'J')]
    json: bool,

    #[usage(flatten)]
    target: TargetArgs,

    #[usage(flatten)]
    fields: TaskFields,
}

impl TasksAdd {
    pub(super) async fn run(self) -> Result<()> {
        let mut report = if self.file {
            if self.target.is_set() {
                bail!(
                    "--file writes to the task directory; it cannot be used with --env, --global or --path"
                );
            }
            self.add_file_task().await?
        } else {
            self.add_toml_task()?
        };
        report.dry_run = self.dry_run;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            report.print();
        }
        Ok(())
    }

    async fn add_file_task(&self) -> Result<Report> {
        let mut path = Task::task_dir()
            .await?
            .join(self.task.replace(':', MAIN_SEPARATOR_STR));
        if path.is_dir() {
            path = path.join("_default");
        }
        let mut lines = vec![format!(
            "#!/usr/bin/env {}",
            self.fields.shell.clone().unwrap_or("bash".into())
        )];
        lines.extend(self.fields.header_lines()?);
        lines.push("set -euxo pipefail".into());
        lines.push("".into());
        if !self.run.is_empty() {
            lines.push(self.run.join(" "));
            lines.push("".into());
        }
        let content = lines.join("\n");
        let action = if !path.exists() {
            Action::Added
        } else if file::read_to_string(&path)? == content {
            Action::Unchanged
        } else {
            self.allow_replace(&path)?;
            Action::Replaced
        };
        if action != Action::Unchanged && !self.dry_run {
            file::create_dir_all(path.parent().unwrap())?;
            file::write(&path, content)?;
            file::make_executable(&path)?;
        }
        Ok(Report::new(&self.task, action, Kind::File, &path))
    }

    fn add_toml_task(&self) -> Result<Report> {
        let path = self.target.config_path()?;
        write::ensure_toml(&path)?;
        let before = if path.exists() {
            file::read_to_string(&path)?
        } else {
            String::new()
        };
        let mut doc: toml_edit::DocumentMut = before.parse()?;
        let mut task = toml_edit::Table::new();
        for (key, value) in self.fields.toml_entries(&self.run)? {
            task.insert(key, Item::Value(value));
        }
        let inline = doc.get("tasks").is_some_and(Item::is_inline_table);
        let tasks = write::tasks_table(&mut doc, &path, true)?.unwrap();
        let exists = tasks.contains_key(&self.task);
        if inline {
            // An inline `tasks = { ... }` can only hold inline tables.
            tasks.insert(
                &self.task,
                Item::Value(Value::InlineTable(task.into_inline_table())),
            );
        } else {
            tasks.insert(&self.task, Item::Table(task));
        }
        let after = doc.to_string();
        let old = write::task_value(&before, &self.task)?;
        let new = write::task_value(&after, &self.task)?;
        let action = if !exists {
            Action::Added
        } else if old == new {
            Action::Unchanged
        } else {
            self.allow_replace(&path)?;
            Action::Replaced
        };
        let newly_trusted = if action != Action::Unchanged && !self.dry_run {
            write::write_config(&path, &after)?
        } else {
            false
        };
        Ok(Report::new(&self.task, action, Kind::Toml, &path)
            .with_table(new)?
            .trusted(newly_trusted))
    }

    /// Lets the add replace the existing task at `path`, which needs `--force`. Without it the
    /// replacement still happens for now, with a deprecation warning, since it used to be silent
    /// and scaffolding scripts rely on it.
    fn allow_replace(&self, path: &std::path::Path) -> Result<()> {
        if self.force {
            return Ok(());
        }
        if self.dry_run {
            // A preview reports `replaced` instead of warning about a write it does not make.
            return Ok(());
        }
        // Once this is removed, replace it with an error that suggests --force and `mise tasks set`.
        deprecated_at!(
            "2026.10.0",
            "2027.10.0",
            "tasks-add-replace",
            "mise tasks add replaced task {} in {} without --force. Pass --force to replace a task, or use `mise tasks set` to change some of its fields; replacing one without --force will fail.",
            self.task,
            display_path(path)
        );
        Ok(())
    }
}
