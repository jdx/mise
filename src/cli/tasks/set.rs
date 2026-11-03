use eyre::{Result, bail};
use toml_edit::{Item, Table, Value};

use super::write::{self, Action, Definition, Kind, Report, TargetArgs, TaskFields};
use crate::file::{self, display_path};

/// Change fields of an existing TOML task
///
/// Updates only the fields given and leaves the rest of the task, and the
/// rest of the file, as they are. A list field such as `--depends` replaces
/// the whole list; `--env-var` and `--tools` add or replace single entries of
/// the task's `env` and `tools` tables. `--unset` removes a field, which is
/// also how a flag such as `hide` goes back to false. A command given after
/// `--` replaces the task's `run`.
///
/// Without `--env`, `--global` or `--path`, edits the one config file that
/// defines the task, and fails when several do. Like `mise tasks add`, it
/// trusts the file's config root so mise loads it, except in safe mode
/// (`MISE_SAFE=1`). File tasks are scripts: use `mise tasks edit` for them.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        "mise tasks set build --description \"Build the app for release\"",
        help = "Change a task's description"
    ),
    example(
        "mise tasks set test --env-var RUST_BACKTRACE=1 --unset quiet",
        help = "Add an environment variable and remove the quiet flag"
    ),
    example(
        "mise tasks set --env local lint -- npx eslint --fix .",
        help = "Change the command of a task in mise.local.toml"
    )
)]
pub(super) struct TasksSet {
    /// Name of the task to change
    #[usage()]
    task: String,

    /// Command to run instead of the current one, given after `--`
    #[usage(double_dash = "required")]
    run: Vec<String>,

    /// Show what would be written without writing it
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Print what was written as JSON
    #[usage(long, short = 'J')]
    json: bool,

    /// Remove this field from the task; repeat for several
    #[usage(long, short = 'u', value_name = "FIELD")]
    unset: Vec<String>,

    #[usage(flatten)]
    target: TargetArgs,

    #[usage(flatten)]
    fields: TaskFields,
}

impl TasksSet {
    pub(super) async fn run(self) -> Result<()> {
        if self.run.is_empty() && self.fields.is_empty() && self.unset.is_empty() {
            bail!("nothing to change; pass a field flag, --unset, or a command after --");
        }
        for key in &self.unset {
            write::ensure_task_field(key)?;
        }
        let path = match write::choose_definition(&self.task, &self.target).await? {
            Definition::Toml(path) => path,
            Definition::File(path) => bail!(
                "task {} is a file task ({}); change its script, for example with `mise tasks edit {}`",
                self.task,
                display_path(&path),
                self.task
            ),
            Definition::Other(path) => bail!(
                "task {} is defined in {}, which mise tasks set does not edit",
                self.task,
                display_path(&path)
            ),
        };

        let before = file::read_to_string(&path)?;
        let mut doc: toml_edit::DocumentMut = before.parse()?;
        let tasks = write::tasks_table(&mut doc, &path, false)?.unwrap();
        // `name = "cmd"` and `name = ["cmd", ...]` are shorthand for a task with only `run`.
        let item = tasks.get_mut(&self.task).unwrap();
        if let Some(run) = item.as_value().filter(|value| !value.is_inline_table()) {
            let mut table = Table::new();
            table.insert("run", Item::Value(run.clone()));
            *item = Item::Table(table);
            // The key's spacing was for `name = ...` and would end up inside `[tasks.name]`.
            if let Some(mut key) = tasks.key_mut(&self.task) {
                key.fmt();
            }
        }
        let item = tasks.get_mut(&self.task).unwrap();
        let Some(task) = item.as_table_like_mut() else {
            bail!(
                "task {} in {} is not a table",
                self.task,
                display_path(&path)
            );
        };
        for (key, value) in self.fields.toml_entries(&self.run)? {
            let merge =
                matches!(key, "env" | "tools") && task.get(key).is_some_and(Item::is_table_like);
            match value {
                Value::InlineTable(entries) if merge => {
                    let existing = task.get_mut(key).unwrap();
                    let table = existing.as_table_like_mut().unwrap();
                    for (name, value) in entries {
                        table.insert(&name, Item::Value(value));
                    }
                    if let Some(inline) = existing.as_inline_table_mut() {
                        // Otherwise the old last entry's trailing space ends up before a comma.
                        inline.fmt();
                    }
                }
                mut value => {
                    // Keep the comment and spacing around a value that is replaced.
                    if let Some(old) = task.get(key).and_then(Item::as_value) {
                        *value.decor_mut() = old.decor().clone();
                    }
                    task.insert(key, Item::Value(value));
                }
            }
        }
        for key in &self.unset {
            task.remove(key);
        }
        write::hide_empty_tasks_header(&mut doc);

        let after = doc.to_string();
        let old = write::task_value(&before, &self.task)?;
        let new = write::task_value(&after, &self.task)?;
        let action = if old == new {
            Action::Unchanged
        } else {
            Action::Updated
        };
        let newly_trusted = if action == Action::Updated && !self.dry_run {
            write::write_config(&path, &after)?
        } else {
            false
        };
        let mut report = Report::new(&self.task, action, Kind::Toml, &path)
            .with_table(new)?
            .trusted(newly_trusted);
        report.dry_run = self.dry_run;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            report.print();
        }
        Ok(())
    }
}
