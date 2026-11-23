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
/// extra paths the task and the command line allow.
pub(crate) async fn key(
    task: &Task,
    config: &Arc<Config>,
    cli: &SandboxConfig,
) -> Result<Option<CacheSandboxKey>> {
    if !enforced(task) {
        return Ok(None);
    }
    let root = task_cwd(task, config).await?;
    let cwd = std::env::current_dir().unwrap_or_default();
    let task_base = task.dir(config).await?.unwrap_or_else(|| cwd.clone());
    Ok(key_for(
        task,
        cli,
        &KeyBases {
            root: &root,
            task_base: &task_base,
            cwd: &cwd,
        },
    ))
}

/// The directories that give the paths in a key their meaning.
struct KeyBases<'a> {
    /// The task's working directory, which its sources are relative to.
    root: &'a Path,
    /// What a relative path in the task's own `allow_read` and `allow_write`
    /// starts from.
    task_base: &'a Path,
    /// What a relative path on the command line starts from.
    cwd: &'a Path,
}

/// The key records where each allowed path leads, not how it was written:
/// `--allow-read=lib` names a different directory from each directory mise is
/// run in, and must not share an entry. A path under the task's working
/// directory is recorded relative to it, like the sources, so that two
/// checkouts of a project agree on the key.
fn key_for(task: &Task, cli: &SandboxConfig, bases: &KeyBases<'_>) -> Option<CacheSandboxKey> {
    if !enforced(task) {
        return None;
    }
    let root = resolve_existing(bases.root);
    let location = |path: &PathBuf, base: &Path| -> Option<String> {
        // An empty entry allows nothing.
        if path.as_os_str().is_empty() {
            return None;
        }
        let path = file::replace_path(path);
        let written = path.as_os_str().as_encoded_bytes();
        let wants_dir = written.ends_with(b"/") || written.ends_with(b"/.");
        // Joining an absolute path keeps it; collecting the components drops `.`.
        // Following the links is what the sandbox does with the path, and it
        // makes `../lib` from a subdirectory the same grant as `lib`.
        let path = resolve_existing(&base.join(&path).components().collect::<PathBuf>());
        // A trailing separator asks for a directory. On a file the sandbox
        // allows nothing for it (`SandboxConfig::resolve_paths`), so it must
        // not share an entry with the spelling that does allow the file.
        let refused = wants_dir && path.exists() && !path.is_dir();
        let path = match path.strip_prefix(&root) {
            Ok(inside) if inside.as_os_str().is_empty() => Path::new("."),
            Ok(inside) => inside,
            Err(_) => &path,
        };
        let mut location = key_text(path);
        if refused {
            location.push('/');
        }
        Some(location)
    };
    let declared = |task_paths: &[PathBuf], cli_paths: &[PathBuf]| {
        let task_paths = task_paths
            .iter()
            .filter_map(|path| location(path, bases.task_base));
        let cli_paths = cli_paths
            .iter()
            .filter_map(|path| location(path, bases.cwd));
        task_paths
            .chain(cli_paths)
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
pub(crate) struct CreatedDirs(BTreeSet<PathBuf>);

impl CreatedDirs {
    pub(crate) fn create(dirs: &[PathBuf]) -> Result<Self> {
        // Filled as it goes, so a failure part-way removes what was created.
        let mut created = Self::default();
        for dir in dirs {
            created.0.extend(
                dir.ancestors()
                    .take_while(|ancestor| !ancestor.exists())
                    .map(Path::to_path_buf),
            );
            file::create_dir_all(dir)?;
        }
        Ok(created)
    }
}

impl Drop for CreatedDirs {
    fn drop(&mut self) {
        // A directory sorts before everything beneath it, so the reverse order
        // reaches each one after its children, including a new parent that
        // several outputs share.
        for dir in self.0.iter().rev() {
            // Fails on a directory that holds something, which is the point.
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Derive the allowed paths for `task` from its sources, outputs and cached
/// dependencies. `allowed_write` holds the directories the task or the command
/// line already made writable, resolved the way the sandbox resolves them
/// ([`SandboxConfig::resolve_paths`]).
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
///
/// A rule is bound to what a path resolves to, not to how it is written, so
/// the checks here compare resolved paths: an output directory that is a link
/// to the sources (`dist -> src`) would otherwise make the sources writable.
fn output_dirs(
    task: &Task,
    root: &Path,
    inputs: &[&Path],
    allowed_write: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    // An input that is a link counts under both names: the task reads the
    // file it leads to, and could replace the link where it sits.
    let inputs: BTreeSet<PathBuf> = inputs
        .iter()
        .flat_map(|input| [input.to_path_buf(), resolve_existing(input)])
        .collect();
    let allowed_write: Vec<PathBuf> = allowed_write
        .iter()
        .map(|allowed| resolve_existing(allowed))
        .collect();
    let holds_inputs = |dir: &Path| {
        let dir = resolve_existing(dir);
        inputs.iter().any(|input| input.starts_with(&dir))
    };
    let already_writable = |dir: &Path| {
        let dir = resolve_existing(dir);
        allowed_write.iter().any(|allowed| dir.starts_with(allowed))
    };
    let describe = |dir: &Path| {
        let resolved = resolve_existing(dir);
        if resolved == dir {
            display_path(dir)
        } else {
            format!(
                "{} (which resolves to {})",
                display_path(dir),
                display_path(&resolved)
            )
        }
    };
    let mut dirs = BTreeSet::new();
    for output in output_glob_patterns(&task.outputs.patterns()) {
        let dir = if is_glob_pattern(&output) {
            // `*.o` has no literal part: joining nothing would add a trailing `/`.
            match literal_dir(&output) {
                literal if literal.as_os_str().is_empty() => root.to_path_buf(),
                literal => root.join(literal),
            }
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
        // A directory that an explicit grant covers is still created: Landlock
        // drops a rule for a path that does not exist, so the grant alone would
        // leave the task unable to make it.
        if !already_writable(&dir) && holds_inputs(&dir) {
            bail!(
                "task {}: cache.sandbox cannot confine writes for output {output:?}: {} also holds the task's sources. \
                 Put the outputs in a directory of their own, or list that directory in allow_write.",
                task.name,
                describe(&dir)
            );
        }
        dirs.insert(dir);
    }
    Ok(dirs.into_iter().collect())
}

/// `path` as text that no other path shares. A lossy conversion gives two names
/// that are not valid UTF-8 the same text, while the sandbox is given the two
/// paths. Bytes that are not UTF-8 are written as `\xNN`, and a `\` is doubled
/// so that this text cannot be the name of another path.
fn key_text(path: &Path) -> String {
    let mut text = String::new();
    for chunk in path.as_os_str().as_encoded_bytes().utf8_chunks() {
        for c in chunk.valid().chars() {
            if c == '\\' {
                text.push(c);
            }
            text.push(c);
        }
        for byte in chunk.invalid() {
            text.push_str(&format!("\\x{byte:02x}"));
        }
    }
    text
}

/// `path` with the symlinks in the part of it that exists followed. The part
/// that does not exist yet is kept as written: mise creates it as plain
/// directories.
fn resolve_existing(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(resolved) = dunce::canonicalize(ancestor) {
            return match path.strip_prefix(ancestor) {
                Ok(rest) if !rest.as_os_str().is_empty() => resolved.join(rest),
                _ => resolved,
            };
        }
    }
    path.to_path_buf()
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
///
/// One task can be a dependency several times, with arguments or environment
/// that give each instance outputs of its own, so an instance is told apart
/// the way the task graph does it, not by name alone.
async fn dependency_outputs(task: &Task, config: &Arc<Config>) -> Result<Vec<PathBuf>> {
    let mut outputs = Vec::new();
    let mut seen = HashSet::from([task.clone()]);
    let mut queue = task.resolve_depends(config, &[]).await?.depends;
    while let Some(dep) = queue.pop() {
        if !seen.insert(dep.clone()) {
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

        // An explicit allow_write is the declared way out. The directory is
        // still returned, so that one that does not exist yet gets created.
        let allowed = [PathBuf::from("/project")];
        assert_eq!(
            output_dirs(&sandboxed_task(&["*.o"]), root, &inputs, &allowed).unwrap(),
            [PathBuf::from("/project")]
        );
        assert_eq!(
            output_dirs(&sandboxed_task(&["dist/**"]), root, &inputs, &allowed).unwrap(),
            [PathBuf::from("/project/dist")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn output_dirs_compare_what_the_paths_resolve_to() {
        let tmp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(tmp.path()).unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.c"), "").unwrap();
        std::os::unix::fs::symlink("src", root.join("linked")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("project")).unwrap();
        let source = root.join("src/main.c");
        let inputs = [source.as_path()];

        // An output directory that is a link to the sources would make them
        // writable, whether it is named as a path or as a glob.
        for outputs in [["linked"], ["linked/**"]] {
            let err = output_dirs(&sandboxed_task(&outputs), &root, &inputs, &[]).unwrap_err();
            assert!(
                err.to_string().contains("also holds the task's sources"),
                "{outputs:?}: {err}"
            );
        }
        // The same directory reached through a link is still a directory of its own.
        let linked_root = root.join("project");
        assert_eq!(
            output_dirs(&sandboxed_task(&["dist"]), &linked_root, &inputs, &[]).unwrap(),
            [linked_root.join("dist")]
        );

        // An allowed directory matches under any spelling of it.
        let allowed = [root.clone()];
        assert_eq!(
            output_dirs(&sandboxed_task(&["*.o"]), &linked_root, &inputs, &allowed).unwrap(),
            std::slice::from_ref(&linked_root)
        );
        let allowed = [linked_root];
        assert_eq!(
            output_dirs(&sandboxed_task(&["*.o"]), &root, &inputs, &allowed).unwrap(),
            std::slice::from_ref(&root)
        );
    }

    #[test]
    fn created_dirs_remove_a_new_parent_that_outputs_share() {
        let tmp = tempfile::tempdir().unwrap();
        let existing = tmp.path().join("kept");
        std::fs::create_dir(&existing).unwrap();
        let dirs = [
            tmp.path().join("nest/a"),
            tmp.path().join("nest/b/c"),
            existing.clone(),
            tmp.path().join("used/deep"),
        ];

        let created = CreatedDirs::create(&dirs).unwrap();
        assert!(dirs.iter().all(|dir| dir.is_dir()));
        std::fs::write(tmp.path().join("used/deep/out.txt"), "").unwrap();
        drop(created);

        // Everything mise created and the task left empty is gone, a shared
        // parent included; what holds a file or was there before stays.
        assert!(!tmp.path().join("nest").exists());
        assert!(existing.is_dir());
        assert!(tmp.path().join("used/deep/out.txt").is_file());
    }

    #[test]
    fn key_marks_only_enforced_tasks_and_carries_declared_paths() {
        let cli = SandboxConfig {
            allow_read: vec![PathBuf::from("/opt/sdk")],
            ..Default::default()
        };
        let mut task = sandboxed_task(&["dist"]);
        task.allow_read = vec![PathBuf::from("vendor")];

        let bases = KeyBases {
            root: Path::new("/project"),
            task_base: Path::new("/project"),
            cwd: Path::new("/project"),
        };
        let marker = key_for(&task, &cli, &bases);
        if cfg!(target_os = "linux") {
            assert_eq!(
                serde_json::to_value(marker.unwrap()).unwrap(),
                serde_json::json!({ "version": 1, "allow_read": ["/opt/sdk", "vendor"] })
            );
        } else {
            assert_eq!(marker, None);
        }

        task.cache.as_mut().unwrap().sandbox = false;
        assert_eq!(key_for(&task, &cli, &bases), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn key_records_where_an_allowed_path_leads() {
        let allowed = |task_paths: &[&str], cli_paths: &[&str], cwd: &str| {
            let mut task = sandboxed_task(&["dist"]);
            task.allow_read = task_paths.iter().map(PathBuf::from).collect();
            let cli = SandboxConfig {
                allow_write: cli_paths.iter().map(PathBuf::from).collect(),
                ..Default::default()
            };
            let bases = KeyBases {
                root: Path::new("/project"),
                task_base: Path::new("/project"),
                cwd: Path::new(cwd),
            };
            let key = serde_json::to_value(key_for(&task, &cli, &bases).unwrap()).unwrap();
            (key["allow_read"].clone(), key["allow_write"].clone())
        };
        let json = |paths: &[&str]| serde_json::json!(paths);
        let none = serde_json::Value::Null;

        // The same spelling from two directories is two different grants.
        assert_eq!(
            allowed(&[], &["lib"], "/project"),
            (none.clone(), json(&["lib"]))
        );
        assert_eq!(
            allowed(&[], &["lib"], "/project/src"),
            (none.clone(), json(&["src/lib"]))
        );
        assert_eq!(
            allowed(&[], &["lib"], "/elsewhere"),
            (none.clone(), json(&["/elsewhere/lib"]))
        );
        // Two spellings of one directory are one grant, whoever wrote them.
        assert_eq!(
            allowed(&["./lib/", "/project/lib"], &["../lib/."], "/project"),
            (json(&["lib"]), json(&["../lib"]))
        );
        assert_eq!(
            allowed(&["."], &["/project"], "/elsewhere"),
            (json(&["."]), json(&["."]))
        );
        // An empty entry allows nothing, so it is not a grant.
        assert_eq!(allowed(&[""], &[""], "/project"), (none.clone(), none));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn key_follows_what_exists_and_keeps_a_refused_directory_spelling() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        std::fs::create_dir_all(project.join("lib")).unwrap();
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("secret.txt"), "").unwrap();
        std::os::unix::fs::symlink("lib", project.join("link")).unwrap();

        let allowed = |path: &str, cwd: &Path| {
            let task = sandboxed_task(&["dist"]);
            let cli = SandboxConfig {
                allow_read: vec![PathBuf::from(path)],
                ..Default::default()
            };
            let bases = KeyBases {
                root: &project,
                task_base: &project,
                cwd,
            };
            key_for(&task, &cli, &bases).unwrap().allow_read
        };

        // A path that goes up and back down, or through a link, is the
        // directory it reaches.
        assert_eq!(allowed("lib", &project), ["lib"]);
        assert_eq!(allowed("../lib", &project.join("src")), ["lib"]);
        assert_eq!(allowed("link/", &project), ["lib"]);
        // A link that now leads elsewhere is another grant.
        std::fs::remove_file(project.join("link")).unwrap();
        std::os::unix::fs::symlink("src", project.join("link")).unwrap();
        assert_eq!(allowed("link", &project), ["src"]);

        // A trailing separator on a file allows nothing, so it is not the grant
        // that the plain spelling is. On a directory it changes nothing.
        assert_eq!(allowed("secret.txt", &project), ["secret.txt"]);
        assert_eq!(allowed("secret.txt/", &project), ["secret.txt/"]);
        assert_eq!(allowed("secret.txt/.", &project), ["secret.txt/"]);
        assert_eq!(allowed("lib/", &project), ["lib"]);

        // Names that are not valid UTF-8 keep their bytes, so two links to two
        // such files are two grants. A name that spells out those bytes is a
        // third.
        use std::os::unix::ffi::OsStrExt;
        for (link, target) in [
            ("fe", &b"data\xfe"[..]),
            ("ff", b"data\xff"),
            ("text", b"data\\xfe"),
        ] {
            let target = std::ffi::OsStr::from_bytes(target);
            std::fs::write(project.join(target), "").unwrap();
            std::os::unix::fs::symlink(target, project.join(link)).unwrap();
        }
        assert_eq!(allowed("fe", &project), ["data\\xfe"]);
        assert_eq!(allowed("ff", &project), ["data\\xff"]);
        assert_eq!(allowed("text", &project), ["data\\\\xfe"]);
        assert_eq!(key_text(Path::new("src/é.txt")), "src/é.txt");
    }
}
