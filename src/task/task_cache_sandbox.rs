//! The sandbox a task's cache declaration implies (`cache.sandbox`).
//!
//! A cached task promises that its sources, tools, declared environment and
//! cached dependencies decide its outputs. This module turns that promise into
//! a sandbox: the task may read what its cache key covers and write where its
//! outputs go, and nothing else in the project. A file the key does not cover
//! cannot be read, so it cannot change a result that the key then restores.

use crate::config::env_directive::EnvDirective;
use crate::config::{Config, Settings};
use crate::file::{self, display_path};
use crate::sandbox::SandboxConfig;
use crate::task::Task;
use crate::task::task_cache::{resolve_output_roots, validate_config};
use crate::task::task_source_checker::{
    is_glob_pattern, output_glob_patterns, task_cwd, task_source_match_root, task_source_paths,
};
use eyre::{Result, bail};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Version of the derived policy. Bump it when a change lets a sandboxed task
/// reach something it could not before, so entries made under the looser
/// policy stop matching.
const POLICY_VERSION: u8 = 1;

/// Whether the task asks for a sandbox derived from its cache declaration.
pub(crate) fn requested(task: &Task) -> bool {
    task.cache
        .as_ref()
        .is_some_and(|cache| cache.enabled && cache.sandbox)
}

/// Whether mise enforces the derived sandbox for the task on this platform.
/// Landlock is what makes per-file read rules possible, so only Linux does.
pub(crate) fn enforced(task: &Task) -> bool {
    cfg!(target_os = "linux") && requested(task)
}

/// What the sandbox adds to a task's cache key.
///
/// Entries made inside the sandbox must not be shared with entries made
/// outside it, and every path the task opens beyond the derived set is an
/// input the key would otherwise miss.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct CacheSandboxKey {
    version: u8,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    allow_read: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    allow_write: Vec<String>,
}

/// The cache-key marker for a task that runs in the derived sandbox, with the
/// extra paths the task and the command line allow, as they were written.
pub(crate) fn key(task: &Task, cli: &SandboxConfig) -> Option<CacheSandboxKey> {
    if !enforced(task) {
        return None;
    }
    let declared = |task_paths: &[PathBuf], cli_paths: &[PathBuf]| {
        task_paths
            .iter()
            .chain(cli_paths)
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    };
    Some(CacheSandboxKey {
        version: POLICY_VERSION,
        allow_read: declared(&task.allow_read, &cli.allow_read),
        allow_write: declared(&task.allow_write, &cli.allow_write),
    })
}

/// What a derived sandbox is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SandboxUse {
    /// Only the environment filter is consulted; no command starts.
    EnvOnly,
    /// A command that inspects the task's inputs, such as `skip_if` or a cache
    /// command input. It reads what the task reads and writes nothing.
    Probe,
    /// The task itself, which also writes its outputs.
    Run,
}

/// Variables the task sets by name in its own `env`. Their values are part of
/// the cache key, so the environment filter lets them through. Directives
/// that do not name a variable (`_.file`, `_.source`) are not covered: name
/// what the task needs from them in `cache.env`.
pub(crate) fn declared_env_keys(task: &Task) -> Vec<String> {
    if !enforced(task) {
        return vec![];
    }
    task.render_env_directives()
        .into_iter()
        .filter_map(|(directive, _)| match directive {
            EnvDirective::Val(key, ..) | EnvDirective::Default(key, ..) => Some(key),
            _ => None,
        })
        .collect()
}

/// Paths the derived sandbox allows.
#[derive(Debug, Default)]
pub(crate) struct CacheSandboxPaths {
    /// Files and directories the task may read.
    pub(crate) read: Vec<PathBuf>,
    /// Directories whose entries the task may list without reading them.
    pub(crate) list: Vec<PathBuf>,
    /// Directories the task may write. They may not exist yet; see [`CreatedDirs`].
    pub(crate) write: Vec<PathBuf>,
}

/// Output directories mise created so that the sandbox can allow them:
/// Landlock binds a rule to an open directory, so it has to exist before the
/// task starts. Dropping this removes the ones the task left empty, so a
/// failed run does not leave outputs behind that look fresh.
#[derive(Debug, Default)]
pub(crate) struct CreatedDirs(Vec<PathBuf>);

impl CreatedDirs {
    pub(crate) fn create(dirs: &[PathBuf]) -> Result<Self> {
        let mut created = Vec::new();
        for dir in dirs {
            // Deepest first, which is the order they can be removed in.
            created.extend(
                dir.ancestors()
                    .take_while(|ancestor| !ancestor.exists())
                    .map(Path::to_path_buf),
            );
            file::create_dir_all(dir)?;
        }
        Ok(Self(created))
    }
}

impl Drop for CreatedDirs {
    fn drop(&mut self) {
        for dir in &self.0 {
            // Fails on a directory that holds something, which is the point.
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Derive the allowed paths for `task` from its sources, outputs and cached
/// dependencies. `allowed_write` holds the directories the task or the command
/// line already made writable.
pub(crate) async fn derive(
    task: &Task,
    config: &Arc<Config>,
    allowed_write: &[PathBuf],
    sandbox_use: SandboxUse,
) -> Result<CacheSandboxPaths> {
    Settings::get().ensure_experimental(mise_config::experimental::TASK_CACHE.what)?;
    let root = task_cwd(task, config).await?;
    // `--task-cache off` skips the cache's own check, and the sandbox still applies.
    validate_config(task, &root)?;
    let sources: Vec<PathBuf> = task_source_paths(task, config)
        .await?
        .into_iter()
        .map(|path| root.join(path))
        .collect();
    let config_sources: Vec<PathBuf> = task
        .config_sources()
        .into_iter()
        .map(Path::to_path_buf)
        .collect();
    let write = if sandbox_use == SandboxUse::Run {
        let inputs: Vec<&Path> = sources
            .iter()
            .chain(&config_sources)
            .map(PathBuf::as_path)
            .collect();
        output_dirs(task, &root, &inputs, allowed_write)?
    } else {
        vec![]
    };
    let mut read = sources;
    read.extend(config_sources);
    read.extend(dependency_outputs(task, config).await?);
    let match_root = task_source_match_root(&root, config);
    let list = if match_root == root {
        vec![root]
    } else {
        vec![match_root, root]
    };
    Ok(CacheSandboxPaths { read, list, write })
}

/// The directories a task must be able to write for its declared outputs.
///
/// Landlock grants access to a directory and everything beneath it, or to one
/// existing file. It cannot grant "create this one name here", so each output
/// maps to a directory: the part of a glob before its first wildcard, or the
/// parent of a plain path. A plain path whose parent also holds the task's
/// inputs is taken to be a directory itself, as in `outputs = ["dist"]`.
fn output_dirs(
    task: &Task,
    root: &Path,
    inputs: &[&Path],
    allowed_write: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let holds_inputs = |dir: &Path| inputs.iter().any(|input| input.starts_with(dir));
    let cwd = std::env::current_dir().unwrap_or_default();
    let already_writable = |dir: &Path| {
        allowed_write
            .iter()
            .any(|allowed| dir.starts_with(cwd.join(allowed)))
    };
    let mut dirs = BTreeSet::new();
    for output in output_glob_patterns(&task.outputs.patterns()) {
        let dir = if is_glob_pattern(&output) {
            root.join(literal_dir(&output))
        } else {
            let path = root.join(&output);
            let parent = path.parent().unwrap_or(root).to_path_buf();
            if !holds_inputs(&parent) || already_writable(&parent) {
                parent
            } else if path.is_file() {
                bail!(
                    "task {}: cache.sandbox cannot make only {} writable, because its directory also holds the task's sources. \
                     Move the output into a directory of its own, or list its directory in allow_write.",
                    task.name,
                    display_path(&path)
                );
            } else {
                path
            }
        };
        if already_writable(&dir) {
            continue;
        }
        if holds_inputs(&dir) {
            bail!(
                "task {}: cache.sandbox cannot confine writes for output {output:?}: {} also holds the task's sources. \
                 Put the outputs in a directory of their own, or list that directory in allow_write.",
                task.name,
                display_path(&dir)
            );
        }
        dirs.insert(dir);
    }
    Ok(dirs.into_iter().collect())
}

/// The directory part of a glob that comes before its first wildcard:
/// `dist/**` gives `dist`, `build/*.o` gives `build`, and `*.o` gives nothing.
fn literal_dir(pattern: &str) -> PathBuf {
    Path::new(pattern)
        .components()
        .take_while(|component| match component {
            Component::Normal(name) => !is_glob_pattern(&name.to_string_lossy()),
            _ => true,
        })
        .collect()
}

/// Outputs of the task's cached dependencies, direct and transitive.
///
/// A cached dependency's key is part of this task's key, and that key covers
/// its own dependencies in turn, so everything those tasks produced is keyed.
/// A dependency without a cache has no key: list what the task needs from it
/// in `sources` instead.
async fn dependency_outputs(task: &Task, config: &Arc<Config>) -> Result<Vec<PathBuf>> {
    let mut outputs = Vec::new();
    let mut seen = HashSet::from([task.name.clone()]);
    let mut queue = task.resolve_depends(config, &[]).await?.depends;
    while let Some(dep) = queue.pop() {
        if !seen.insert(dep.name.clone()) {
            continue;
        }
        if dep.cache.as_ref().is_some_and(|cache| cache.enabled) && !dep.outputs.is_auto() {
            let dep_root = task_cwd(&dep, config).await?;
            outputs.extend(
                resolve_output_roots(&dep, &dep_root, false)?
                    .into_iter()
                    .map(|output| dep_root.join(output)),
            );
        }
        queue.extend(dep.resolve_depends(config, &[]).await?.depends);
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{TaskCacheConfig, TaskOutputs};
    use pretty_assertions::assert_eq;

    fn sandboxed_task(outputs: &[&str]) -> Task {
        Task {
            name: "build".to_string(),
            sources: vec!["src/**".to_string()],
            outputs: TaskOutputs::Files(outputs.iter().map(|o| o.to_string()).collect()),
            cache: Some(TaskCacheConfig {
                enabled: true,
                sandbox: true,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn literal_dir_stops_at_the_first_wildcard() {
        assert_eq!(literal_dir("dist/**"), PathBuf::from("dist"));
        assert_eq!(literal_dir("build/obj/*.o"), PathBuf::from("build/obj"));
        assert_eq!(literal_dir("out/{a,b}/x"), PathBuf::from("out"));
        assert_eq!(literal_dir("*.o"), PathBuf::new());
    }

    #[test]
    fn output_dirs_map_each_output_to_a_directory_free_of_inputs() {
        let root = Path::new("/project");
        let source = Path::new("/project/src/main.c");
        let config = Path::new("/project/mise.toml");
        let inputs = [source, config];

        let task = sandboxed_task(&["dist", "build/obj/*.o", "reports/junit.xml"]);
        assert_eq!(
            output_dirs(&task, root, &inputs, &[]).unwrap(),
            [
                PathBuf::from("/project/build/obj"),
                PathBuf::from("/project/dist"),
                PathBuf::from("/project/reports"),
            ]
        );
    }

    #[test]
    fn output_dirs_refuse_a_directory_that_holds_sources() {
        let root = Path::new("/project");
        let source = Path::new("/project/src/main.c");
        let inputs = [source];

        let err = output_dirs(&sandboxed_task(&["*.o"]), root, &inputs, &[]).unwrap_err();
        assert!(err.to_string().contains("also holds the task's sources"));
        let err = output_dirs(&sandboxed_task(&["src/**"]), root, &inputs, &[]).unwrap_err();
        assert!(err.to_string().contains("also holds the task's sources"));
        // A subdirectory that holds none of them is fine.
        assert_eq!(
            output_dirs(&sandboxed_task(&["src/gen/**"]), root, &inputs, &[]).unwrap(),
            [PathBuf::from("/project/src/gen")]
        );

        // An explicit allow_write is the declared way out.
        let allowed = [PathBuf::from("/project")];
        assert_eq!(
            output_dirs(&sandboxed_task(&["*.o"]), root, &inputs, &allowed).unwrap(),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn key_marks_only_enforced_tasks_and_carries_declared_paths() {
        let cli = SandboxConfig {
            allow_read: vec![PathBuf::from("/opt/sdk")],
            ..Default::default()
        };
        let mut task = sandboxed_task(&["dist"]);
        task.allow_read = vec![PathBuf::from("vendor")];

        let marker = key(&task, &cli);
        if cfg!(target_os = "linux") {
            assert_eq!(
                serde_json::to_value(marker.unwrap()).unwrap(),
                serde_json::json!({ "version": 1, "allow_read": ["/opt/sdk", "vendor"] })
            );
        } else {
            assert_eq!(marker, None);
        }

        task.cache.as_mut().unwrap().sandbox = false;
        assert_eq!(key(&task, &cli), None);
    }
}
