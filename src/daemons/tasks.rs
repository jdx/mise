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
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Whether a task asks for any daemon. `false` and an empty list ask for none,
/// so such a task never touches daemon configuration and must not be judged
/// against it.
pub(crate) fn declares_daemons(task: &Task) -> bool {
    match &task.daemons {
        None | Some(crate::task::TaskDaemons::All(false)) => false,
        Some(crate::task::TaskDaemons::Names(names)) => !names.is_empty(),
        Some(_) => true,
    }
}

fn project_for_task(project_root: Option<&Path>, task: &Task) -> Result<Option<PathBuf>> {
    if !declares_daemons(task) {
        return Ok(None);
    }
    task.config_root
        .clone()
        .or_else(|| project_root.map(Path::to_path_buf))
        .map(Some)
        .ok_or_else(|| {
            eyre::eyre!(
                "task {} requires daemons but has no project root",
                task.display_name
            )
        })
}

/// The key under which `set` holds the daemon a task asked for.
///
/// A daemon imported from another project is keyed by its qualified ID, and the
/// name this project gave it lives in the alias table, so a bare local name has
/// to be resolved before it can be looked up.
fn key_for(set: &DaemonSet, name: &str) -> Option<String> {
    if set.daemons.contains_key(name) {
        return Some(name.to_string());
    }
    let resolved = set.resolve_alias(name);
    set.daemons.contains_key(&resolved).then_some(resolved)
}

/// Daemons required by `tasks`, in declaration order, as keys into `set`.
pub(crate) fn required(tasks: &[Task], set: &DaemonSet) -> Result<IndexSet<String>> {
    let mut names = IndexSet::new();
    for task in tasks {
        let Some(daemons) = &task.daemons else {
            continue;
        };
        let requested: Vec<String> = match daemons {
            crate::task::TaskDaemons::All(false) => continue,
            // "every daemon" means the ones this project declares. A daemon
            // imported from elsewhere belongs to that project, and starting it
            // because a local task said `true` would reach further than asked.
            crate::task::TaskDaemons::All(true) => set
                .daemons
                .iter()
                .filter(|(_, d)| !d.imported)
                .map(|(key, _)| key.clone())
                .collect(),
            crate::task::TaskDaemons::One(name) => vec![name.clone()],
            crate::task::TaskDaemons::Names(requested) => requested.clone(),
        };
        for name in requested {
            let Some(key) = key_for(set, &name) else {
                bail!(
                    "task {} requires daemon {name:?}, which is not defined in [daemons]",
                    task.display_name
                );
            };
            names.insert(key);
        }
    }
    Ok(names)
}

/// Whether this run has to start daemons at all, erroring when a task declares
/// them without `experimental`. The flag is a parameter because unit tests force
/// `experimental` on, and the gate is the behavior worth pinning: a run whose
/// tasks declare no daemons is never held to the requirement.
pub(crate) fn gate(experimental: bool, tasks: &[Task]) -> Result<bool> {
    let declared = tasks.iter().any(declares_daemons);
    if declared && !experimental {
        bail!("{}", super::EXPERIMENTAL);
    }
    Ok(declared)
}

/// Start every daemon the given tasks require and wait until pitchfork reports
/// them ready. Already-running daemons are left alone, so this is cheap to
/// repeat.
///
/// A dry run still applies the experimental gate, resolves every name, and
/// validates task-backed daemons, so configuration errors fail there as they
/// would on a real run; it just stops before pitchfork.
///
/// Names are resolved per project, not against one merged set. In a monorepo a
/// dependency task can live in a different subproject, and two subprojects may
/// each declare a daemon of the same name; each task's names are therefore
/// looked up in its own configuration hierarchy.
pub(crate) async fn start(
    config: &Arc<Config>,
    tasks: &[Task],
    dry_run: bool,
    install_tools: bool,
) -> Result<()> {
    // This run is the body of a task-backed daemon. Starting daemons again from
    // here is what would recurse, so stop at this one point and leave the rest
    // of the run, including the task's own `depends`, untouched.
    if crate::env::var_is_true(super::DAEMON_TASK_MARKER) {
        return Ok(());
    }
    if !gate(Settings::get().experimental, tasks)? {
        return Ok(());
    }
    Settings::ensure_not_safe("starting task daemons")?;
    let mut by_project: IndexMap<PathBuf, Vec<Task>> = IndexMap::new();
    for task in tasks {
        let Some(project) = project_for_task(config.project_root.as_deref(), task)? else {
            continue;
        };
        by_project.entry(project).or_default().push(task.clone());
    }
    // A daemon root can be shared by several projects, so collect the whole
    // request before touching pitchfork. Only the names travel: each root's
    // configuration is loaded from the invoking config below, not from the
    // config of whichever task happened to name the daemon first.
    // Names travel as the daemon is known inside the project that owns it, which
    // is not the key this project holds it under when it was imported.
    let mut wanted: IndexMap<PathBuf, IndexSet<String>> = IndexMap::new();
    // Roots reached only because a task named a daemon imported from them. Such
    // a root belongs to another project, which owns its profile and the rest of
    // its daemons.
    let mut foreign: IndexSet<PathBuf> = IndexSet::new();
    let mut owned: IndexSet<PathBuf> = IndexSet::new();
    for (project, tasks) in by_project {
        let scoped = runtime::config_for_root(config, &project).await?;
        let set = scoped.daemons()?;
        let keys = required(&tasks, set)?;
        // Pitchfork starts a daemon's dependencies with it, so this covers what
        // comes along and not only what the task named.
        let starting = set.with_dependencies(&keys.iter().cloned().collect::<Vec<_>>());
        super::ensure_not_blocked(set, &starting, None)?;
        // Take the closure, not only the names the task gave. A dependency can
        // live in a project reached by `project =`, and that project has to be
        // registered and started or the daemon comes up without it.
        for daemon in starting.daemons.values() {
            if daemon.imported {
                foreign.insert(daemon.root.clone());
            } else {
                owned.insert(daemon.root.clone());
            }
            wanted
                .entry(daemon.root.clone())
                .or_default()
                .insert(daemon.name.clone());
        }
    }
    // Reaching a root through one project's own daemon settles it, whether or
    // not something else reached the same root through an import.
    foreign.retain(|root| !owned.contains(root));
    if dry_run {
        for (root, names) in &wanted {
            let scoped = runtime::config_for_root(config, root).await?;
            let set = scoped.daemons()?.for_root(root);
            let starting = set.with_dependencies(&names.iter().cloned().collect::<Vec<_>>());
            // Narrowed for another project's root exactly as the real run
            // narrows it, so an unrelated daemon of theirs cannot fail a
            // dry run that the run itself would have completed.
            if foreign.contains(root) {
                starting.validate_tasks(&scoped).await?;
            } else {
                set.validate_tasks(&scoped).await?;
            }
            super::ensure_not_blocked(&set, &starting, Some(root))?;
            for name in names {
                info!("[dry-run] would start daemon {name} in {}", root.display());
            }
        }
        return Ok(());
    }
    // Pitchfork starts a daemon's dependencies with it, and a dependency can
    // live in another project's root. Register every root first and start only
    // once they all exist, so a local daemon listed before an imported one
    // cannot start while that dependency is still unregistered.
    //
    // Holding several project locks at once means the order they are taken in
    // matters: `mise daemons start` sorts its roots, so this takes them in the
    // same order rather than in whatever order the configuration produced, and
    // the two cannot deadlock against each other.
    wanted.sort_keys();
    let mut pending = Vec::new();
    for (root, names) in wanted {
        let scoped = runtime::config_for_root(config, &root).await?;
        // The generated pitchfork configuration describes every daemon in the
        // project, not only the ones this run starts.
        let set = scoped.daemons()?.for_root(&root);
        if set.daemons.is_empty() {
            continue;
        }
        let previous = runtime::read_state(&root)?;
        let (scoped, ts) = if install_tools {
            runtime::toolset(&scoped, true).await?
        } else {
            // Keeps the install path out of this future entirely, so callers
            // inside a spawned task can await it.
            let ts = runtime::toolset_resolved(&scoped, false).await?;
            (scoped, ts)
        };
        let rt = runtime::Runtime::from_toolset(&scoped, &ts, Some(&previous.bin)).await?;
        let owned = !foreign.contains(&root);
        // Another project's root is registered whole but only checked for what
        // this run starts, so an unrelated daemon of theirs cannot fail a
        // `mise run` here.
        let starting = if owned {
            set.clone()
        } else {
            set.with_dependencies(&names.iter().cloned().collect::<Vec<_>>())
        };
        runtime::validate_tools(&starting, &scoped, &ts).await?;
        starting.validate_tasks(&scoped).await?;
        // This root's own configuration, which is the only view that knows
        // about imports the referenced project itself declares.
        let will_start = set.with_dependencies(&names.iter().cloned().collect::<Vec<_>>());
        super::ensure_not_blocked(&set, &will_start, Some(&root))?;
        // Let the configuration hash short-circuit re-registration. Forcing it
        // would re-probe `pitchfork usage` and re-run `config add` on every
        // `mise run` of a task that requires daemons, even when nothing about
        // the daemons changed and they are already running.
        // The profile belongs to the project that owns the root, so a task that
        // reached another project's daemon does not impose its own.
        let (state, _project_lock) = rt.prepare(&root, &set, false, owned).await?;
        let ids: Vec<String> = state
            .ids
            .iter()
            .filter(|id| {
                // `names` holds daemons as the owning project names them, and
                // this root's own set is keyed the same way.
                let name = id.rsplit('/').next().unwrap_or(id);
                names.contains(name) && set.find(name).is_some()
            })
            .cloned()
            .collect();
        if ids.is_empty() {
            continue;
        }
        // The project lock rides along so every root stays held until the last
        // one has started.
        pending.push((rt, root, ids, _project_lock));
    }
    for (rt, root, ids, _project_lock) in pending {
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

    /// A set holding one local daemon and one imported from another project,
    /// the way `load` keys them: the import by qualified ID, with the name this
    /// project gave it in the alias table.
    fn set_with_import() -> DaemonSet {
        let mut set = set(&["api"]);
        let mut worker = set.daemons["api"].clone();
        worker.name = "worker".into();
        worker.root = PathBuf::from("/mirror");
        worker.imported = true;
        set.daemons.insert("mirror/worker".into(), worker);
        set.aliases.insert(
            (PathBuf::from("/project"), "pipeline".into()),
            "mirror/worker".into(),
        );
        set.namespaces
            .insert(PathBuf::from("/mirror"), "mirror".into());
        set.namespaces
            .insert(PathBuf::from("/project"), "project".into());
        set
    }

    #[test]
    fn a_task_can_require_a_daemon_imported_from_another_project() {
        let set = set_with_import();
        // The name this project gave the import resolves to its qualified ID.
        let by_alias = [task("dev", Some(TaskDaemons::One("pipeline".into())))];
        assert_eq!(
            required(&by_alias, &set)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            ["mirror/worker"]
        );
        // So does the qualified ID itself.
        let by_id = [task("dev", Some(TaskDaemons::One("mirror/worker".into())))];
        assert_eq!(
            required(&by_id, &set)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            ["mirror/worker"]
        );
        // `true` means this project's own daemons; another project's daemon is
        // not started because a local task asked for everything.
        let all = [task("dev", Some(TaskDaemons::All(true)))];
        assert_eq!(
            required(&all, &set)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            ["api"]
        );
        // A name that is neither is still rejected.
        let unknown = [task("dev", Some(TaskDaemons::One("nope".into())))];
        assert!(
            required(&unknown, &set)
                .unwrap_err()
                .to_string()
                .contains("not defined in [daemons]")
        );
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
                            imported: false,
                            port: None,
                            host: None,
                        },
                    )
                })
                .collect(),
            ..Default::default()
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
        assert!(!gate(false, &[task("test", Some(TaskDaemons::Names(vec![])))]).unwrap());
    }

    #[test]
    fn daemon_requirement_without_project_root_errors() {
        let task = task("server", Some(TaskDaemons::One("postgres".into())));
        let err = project_for_task(None, &task).unwrap_err();
        assert_eq!(
            err.to_string(),
            "task server requires daemons but has no project root"
        );
    }
}
