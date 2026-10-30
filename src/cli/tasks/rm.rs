use eyre::{Result, bail};
use itertools::Itertools;

use super::write::{self, Action, Definition, Kind, Report, TargetArgs};
use crate::config::Settings;
use crate::config::config_file;
use crate::file::{self, display_path};
use crate::ui::prompt;

/// Remove tasks
///
/// Removes a TOML task's `[tasks.<name>]` table from the config file that
/// defines it, and deletes a file task's script. Without `--env`, `--global`
/// or `--path`, mise uses the one file that defines each task and fails when
/// several do; `--path` can also name a file task's script. Asks before
/// removing anything unless `--yes` (`MISE_YES=1`) is set, and fails without a
/// terminal to ask on.
///
/// Other tasks that depend on a removed task are left as they are; `mise tasks
/// validate` reports them.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "remove",
    example("mise tasks rm build", help = "Remove the build task"),
    example(
        "mise tasks rm --env local --yes lint fmt",
        help = "Remove two tasks from mise.local.toml without asking"
    ),
    example(
        "mise tasks rm --dry-run --json build",
        help = "Show which file defines build without changing it"
    )
)]
pub(super) struct TasksRm {
    /// Tasks to remove
    #[usage(required = true)]
    tasks: Vec<String>,

    /// Show what would be removed without removing it
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Print what was removed as JSON
    #[usage(long, short = 'J')]
    json: bool,

    #[usage(flatten)]
    target: TargetArgs,
}

impl TasksRm {
    pub(super) async fn run(self) -> Result<()> {
        // Find every task first, so that nothing changes when one of them cannot be removed.
        let mut definitions = vec![];
        for task in self.tasks.iter().unique() {
            let definition = match write::choose_definition(task, &self.target).await? {
                Definition::Other(path) => bail!(
                    "task {task} is defined in {}, which mise tasks rm does not edit",
                    display_path(&path)
                ),
                definition => definition,
            };
            // `build` and `build.sh` both name a `build.sh` script, which is removed once.
            if matches!(definition, Definition::File(_))
                && definitions.iter().any(|(_, def)| def == &definition)
            {
                continue;
            }
            definitions.push((task.as_str(), definition));
        }
        if !self.dry_run && !Settings::get().yes {
            let list = definitions
                .iter()
                .map(|(task, def)| format!("{task} ({})", display_path(def.path())))
                .join(", ");
            if !prompt::confirm_destructive(format!("remove {list}?"), "mise tasks rm")? {
                bail!("no tasks removed");
            }
        }

        let mut reports = vec![];
        for (task, definition) in definitions {
            let mut report = match definition {
                Definition::Toml(path) => self.remove_toml_task(task, &path)?,
                Definition::File(path) => {
                    if !self.dry_run {
                        file::remove_file(&path)?;
                    }
                    Report::new(task, Action::Removed, Kind::File, &path)
                }
                Definition::Other(_) => unreachable!(),
            };
            report.dry_run = self.dry_run;
            reports.push(report);
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&reports)?);
        } else {
            reports.iter().for_each(Report::print);
        }
        Ok(())
    }

    fn remove_toml_task(&self, task: &str, path: &std::path::Path) -> Result<Report> {
        let before = file::read_to_string(path)?;
        let mut doc: toml_edit::DocumentMut = before.parse()?;
        let tasks = write::tasks_table(&mut doc, path, false)?.unwrap();
        tasks.remove(task);
        if tasks.is_empty() {
            doc.remove("tasks");
        }
        write::hide_empty_tasks_header(&mut doc);
        if !self.dry_run {
            // Removing a task needs no trust, so none is asked for or recorded. A paranoid trust
            // record is bound to the file's content, though, so it is renewed for a file that
            // was trusted before the edit.
            let renew_trust =
                Settings::get().paranoid.config() && config_file::is_path_trusted(path);
            file::write(path, doc.to_string())?;
            if renew_trust {
                config_file::trust_after_write(path)?;
            }
        }
        Report::new(task, Action::Removed, Kind::Toml, path)
            .with_table(write::task_value(&before, task)?)
    }
}
