//! Starting the daemons a task declares with `daemons = [...]`.
//!
//! This replaces hand-written prerequisite tasks that start a background
//! process and then poll it: pitchfork owns both the process and the readiness
//! check, so `pitchfork start` returns only once every requested daemon reports
//! ready.

use super::{DaemonSet, runtime};
use crate::config::{Config, Settings};
use crate::task::Task;
use eyre::{Result, bail};
use indexmap::{IndexMap, IndexSet};
use std::path::PathBuf;
use std::sync::Arc;

/// Daemon names required by `tasks`, in declaration order.
pub(crate) fn required(tasks: &[Task], set: &DaemonSet) -> Result<IndexSet<String>> {
    let mut names = IndexSet::new();
    for task in tasks {
        let Some(daemons) = &task.daemons else {
            continue;
        };
        let requested: Vec<String> = match daemons {
            crate::task::TaskDaemons::All(false) => continue,
            crate::task::TaskDaemons::All(true) => set.daemons.keys().cloned().collect(),
            crate::task::TaskDaemons::One(name) => vec![name.clone()],
            crate::task::TaskDaemons::Names(requested) => requested.clone(),
        };
        for name in requested {
            if !set.daemons.contains_key(&name) {
                bail!(
                    "task {} requires daemon {name:?}, which is not defined in [daemons]",
                    task.display_name
                );
            }
            names.insert(name);
        }
    }
    Ok(names)
}

/// Whether this run has to start daemons at all, erroring when a task declares
/// them without `experimental`. The flag is a parameter because unit tests force
/// `experimental` on, and the gate is the behavior worth pinning: a run whose
/// tasks declare no daemons is never held to the requirement.
pub(crate) fn gate(experimental: bool, tasks: &[Task]) -> Result<bool> {
    let declared = tasks.iter().any(|task| match &task.daemons {
        None | Some(crate::task::TaskDaemons::All(false)) => false,
        Some(_) => true,
    });
    if declared && !experimental {
        bail!("{}", super::EXPERIMENTAL);
    }
    Ok(declared)
}

/// Start every daemon the given tasks require and wait until pitchfork reports
/// them ready. Already-running daemons are left alone, so this is cheap to
/// repeat.
///
/// A dry run still applies the experimental gate and resolves every name, so a
/// typo fails there as it would on a real run; it just stops before pitchfork.
///
/// Names are resolved per project, not against one merged set. In a monorepo a
/// dependency task can live in a different subproject, and two subprojects may
/// each declare a daemon of the same name; each task's names are therefore
/// looked up in its own configuration hierarchy.
pub(crate) async fn start(config: &Arc<Config>, tasks: &[Task], dry_run: bool) -> Result<()> {
    if !gate(Settings::get().experimental, tasks)? {
        return Ok(());
    }
    Settings::ensure_not_safe("starting task daemons")?;
    let mut by_project: IndexMap<PathBuf, Vec<Task>> = IndexMap::new();
    for task in tasks {
        let Some(project) = task
            .config_root
            .clone()
            .or_else(|| config.project_root.clone())
        else {
            continue;
        };
        by_project.entry(project).or_default().push(task.clone());
    }
    // A daemon root can be shared by several projects, so collect the whole
    // request before touching pitchfork.
    let mut wanted: IndexMap<PathBuf, (Arc<Config>, IndexSet<String>)> = IndexMap::new();
    for (project, tasks) in by_project {
        let scoped = runtime::config_for_root(config, &project).await?;
        let set = scoped.daemons()?;
        for name in required(&tasks, set)? {
            let root = set.daemons[&name].root.clone();
            wanted
                .entry(root)
                .or_insert_with(|| (scoped.clone(), IndexSet::new()))
                .1
                .insert(name);
        }
    }
    if dry_run {
        for (root, (_, names)) in &wanted {
            for name in names {
                info!("[dry-run] would start daemon {name} in {}", root.display());
            }
        }
        return Ok(());
    }
    for (root, (scoped, names)) in wanted {
        let scoped = runtime::config_for_root(&scoped, &root).await?;
        // The generated pitchfork configuration describes every daemon in the
        // project, not only the ones this run starts.
        let set = scoped.daemons()?.for_root(&root);
        if set.daemons.is_empty() {
            continue;
        }
        let previous = runtime::read_state(&root)?;
        let (scoped, ts) = runtime::toolset(&scoped, true).await?;
        let rt = runtime::Runtime::from_toolset(&scoped, &ts, Some(&previous.bin)).await?;
        runtime::validate_tools(&set, &scoped, &ts).await?;
        set.validate_tasks(&scoped).await?;
        let (state, _project_lock) = rt.prepare(&root, &set, true).await?;
        let ids: Vec<String> = state
            .ids
            .iter()
            .filter(|id| {
                let name = id.rsplit('/').next().unwrap_or(id);
                names.contains(name) && set.daemons.contains_key(name)
            })
            .cloned()
            .collect();
        if ids.is_empty() {
            continue;
        }
        rt.exec(&root, [vec!["start".into()], ids].concat()).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemons::Daemon;
    use crate::task::TaskDaemons;

    fn task(name: &str, daemons: Option<TaskDaemons>) -> Task {
        Task {
            name: name.to_string(),
            display_name: name.to_string(),
            daemons,
            ..Default::default()
        }
    }

    fn set(names: &[&str]) -> DaemonSet {
        DaemonSet {
            daemons: names
                .iter()
                .map(|name| {
                    (
                        (*name).to_string(),
                        Daemon {
                            name: (*name).to_string(),
                            source: PathBuf::from("/project/mise.toml"),
                            root: PathBuf::from("/project"),
                            table: toml::Table::new(),
                            preset: None,
                            task: None,
                            tool: None,
                            exports: Default::default(),
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn required_daemons_are_deduplicated_and_checked() {
        let set = set(&["postgres", "nats"]);
        let tasks = [
            task("dev", Some(TaskDaemons::Names(vec!["postgres".into()]))),
            task("api", Some(TaskDaemons::One("nats".into()))),
            task("web", Some(TaskDaemons::Names(vec!["postgres".into()]))),
            task("test", None),
        ];
        assert_eq!(
            required(&tasks, &set)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            ["postgres", "nats"]
        );
        // `true` requires everything the project declares.
        let all = [task("dev", Some(TaskDaemons::All(true)))];
        assert_eq!(required(&all, &set).unwrap().len(), 2);
        // `false` is the same as not declaring any.
        let none = [task("dev", Some(TaskDaemons::All(false)))];
        assert!(required(&none, &set).unwrap().is_empty());
        let unknown = [task("dev", Some(TaskDaemons::One("redis".into())))];
        let err = required(&unknown, &set).unwrap_err().to_string();
        assert!(err.contains("task dev requires daemon \"redis\""), "{err}");
    }

    #[test]
    fn tasks_requiring_daemons_fail_without_experimental() {
        let dev = [task("dev", Some(TaskDaemons::All(true)))];
        let err = gate(false, &dev).unwrap_err().to_string();
        assert_eq!(err, super::super::EXPERIMENTAL);
        assert!(gate(true, &dev).unwrap());
        // A run that requires no daemons is never held to the requirement.
        assert!(!gate(false, &[task("test", None)]).unwrap());
        assert!(!gate(false, &[task("test", Some(TaskDaemons::All(false)))]).unwrap());
    }
}
