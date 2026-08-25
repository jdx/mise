use crate::Result;
use crate::config::Config;
use crate::file;
use crate::file::display_path;
use crate::task::Task;
use eyre::bail;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Generates shims to run mise tasks
///
/// By default, this will build shims like ./bin/<task>. These can be paired with `mise generate install-script`
/// so contributors to a project can execute mise tasks without installing mise into their system.
/// When a parent and nested task both exist, the parent stub is written to `<parent>/_default`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(super) struct TaskStubs {
    /// Directory to create task stubs inside of
    #[usage(long, short, verbatim_doc_comment, default="bin", value_hint=ValueHint::DirPath)]
    dir: PathBuf,

    /// Path to a mise bin to use when running the task stub.
    ///
    /// Use `--mise-bin=./bin/mise` to use a mise bin generated from `mise generate install-script`
    #[usage(long, short, verbatim_doc_comment, default = "mise")]
    mise_bin: PathBuf,
}

impl TaskStubs {
    pub(super) async fn run(self) -> eyre::Result<()> {
        let config = Config::get().await?;
        let tasks = config.tasks().await?;
        // Two paths per task, and they differ only for a task that came from a file: `name` keeps
        // the file's extension, `display_name` does not. The stub is named after the task, and the
        // file-named path is kept so a stub written under the old spelling can be migrated away.
        let task_paths = tasks.values().map(Task::name_to_path).collect::<Vec<_>>();
        let base_paths = stub_base_paths(&tasks, &task_paths);
        let paths = resolve_stub_paths(&self.dir, &base_paths)?;
        let stubs = tasks
            .values()
            .zip(task_paths)
            .zip(paths)
            .map(|((task, legacy_path), path)| {
                Ok(TaskStub {
                    task,
                    legacy_path: self.dir.join(legacy_path),
                    path,
                    output: self.generate(task)?,
                    legacy_output: self.generate_legacy(task)?,
                    launcher: self.generate_launcher(task),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let migrations = validate_stub_paths(&self.dir, &stubs)?;

        for migration in migrations {
            match migration {
                StubMigration::File(path) => {
                    // The launcher goes with the stub it belongs to, or it keeps running a task
                    // that no longer has a stub here. Only one mise wrote is removed, so a
                    // hand-written .cmd is left alone.
                    remove_generated_launcher(&path)?;
                    file::remove_file(path)?
                }
                StubMigration::Directory(path) => file::remove_all(path)?,
            }
        }
        for stub in &stubs {
            if let Some(parent) = stub.path.parent() {
                file::create_dir_all(parent)?;
            }
            file::write(&stub.path, &stub.output)?;
            file::make_executable(&stub.path)?;
            miseprintln!("Wrote to {}", display_path(&stub.path));
            // Windows will not execute the `#!/bin/sh` stub, so it needs something it can launch.
            // Written on every host: stubs are committed, and the contributor who runs one on
            // Windows is not the person who generated it.
            //
            // Reported like the stub is: it is a second committed file, and a run that names one
            // path while leaving two behind is how it goes unnoticed into a commit.
            if let Some(launcher_path) = super::windows_launcher_path(&stub.path) {
                file::write(&launcher_path, &stub.launcher)?;
                miseprintln!("Wrote to {}", display_path(&launcher_path));
            }
        }
        Ok(())
    }

    /// The Windows launcher body for `task`, mirroring what the stub itself runs.
    ///
    /// `mise_bin` is embedded as given: with the default `mise` it resolves off PATH, and a
    /// `--mise-bin` pointing at a `mise generate install-script` script will start working here as soon
    /// as that script gains a Windows form of its own.
    fn generate_launcher(&self, task: &Task) -> String {
        let mise_bin = super::cmd_quote(&self.mise_bin.to_string_lossy());
        // The task name goes through the same quoting: it is interpolated into the same cmd line,
        // so a `&` or `%` in it breaks the launcher exactly the way one in the path does.
        let display_name = super::cmd_quote(&task.display_name);
        super::windows_launcher_body(&format!("{mise_bin} run {display_name}"))
    }

    fn generate(&self, task: &Task) -> Result<String> {
        let mise_bin = self.mise_bin.to_string_lossy();
        let mise_bin = shell_words::quote(&mise_bin);
        let display_name = &task.display_name;
        let script = format!(
            r#"
#!/bin/sh
# generated by mise task-stubs
exec {mise_bin} run {display_name} "$@"
"#
        );
        Ok(script.trim().to_string())
    }

    fn generate_legacy(&self, task: &Task) -> Result<String> {
        let mise_bin = self.mise_bin.to_string_lossy();
        let mise_bin = shell_words::quote(&mise_bin);
        let display_name = &task.display_name;
        let script = format!(
            r#"
#!/bin/sh
exec {mise_bin} run {display_name} "$@"
"#
        );
        Ok(script.trim().to_string())
    }
}

struct TaskStub<'a> {
    task: &'a Task,
    legacy_path: PathBuf,
    path: PathBuf,
    output: String,
    legacy_output: String,
    launcher: String,
}

/// Remove the Windows launcher beside a stub that is being migrated away.
///
/// Recognised by [`super::is_generated_launcher`] rather than by comparing against the launchers
/// this run would produce. Those bodies embed `--mise-bin` and the task name, so a run that changes
/// either would not recognise the launcher it wrote last time and would leave `<task>.cmd` behind,
/// still runnable and still pointing at the old mise. Anything mise did not write stays put, and a
/// missing file is fine: most stubs never had one.
fn remove_generated_launcher(stub_path: &Path) -> Result<()> {
    let Some(launcher) = super::windows_launcher_path(stub_path) else {
        return Ok(());
    };
    let Ok(existing) = file::read_to_string(&launcher) else {
        return Ok(());
    };
    if super::is_generated_launcher(&existing) {
        file::remove_file(&launcher)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum StubMigration {
    File(PathBuf),
    Directory(PathBuf),
}

/// The path each stub should take, named for the task, falling back where that is ambiguous.
///
/// A file task is *named* after its file — `build.sh` — while everything else about it, including
/// the command the stub runs, uses the extensionless display name. Naming the stub after the file
/// produced `bin/build.sh`, and on Windows a `#!/bin/sh` script called `bin/build.bat`, which
/// cmd.exe runs as a batch file line by line.
///
/// Two file tasks that differ only by extension share a display name, and on platforms other than
/// Windows both survive as separate tasks — `prefer_windows_file_task_siblings` collapses the pair
/// only there. Those keep their file-named paths, because renaming both onto one path would take a
/// working project and fail its `task-stubs` run. Windows never reaches that branch, so the case
/// this exists to fix is always unambiguous.
fn stub_base_paths(tasks: &BTreeMap<String, Task>, task_paths: &[PathBuf]) -> Vec<PathBuf> {
    let display_paths = tasks
        .values()
        .map(Task::display_name_to_path)
        .collect::<Vec<_>>();
    let mut counts: HashMap<&PathBuf, usize> = HashMap::new();
    for path in &display_paths {
        *counts.entry(path).or_default() += 1;
    }
    display_paths
        .iter()
        .zip(task_paths)
        .map(|(display_path, task_path)| {
            if counts.get(display_path).copied().unwrap_or_default() > 1 {
                task_path.clone()
            } else {
                display_path.clone()
            }
        })
        .collect()
}

fn resolve_stub_paths(dir: &Path, task_paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let base_paths = task_paths
        .iter()
        .map(|path| dir.join(path))
        .collect::<Vec<_>>();
    let paths = base_paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            if base_paths.iter().enumerate().any(|(other_index, other)| {
                index != other_index && other != path && other.starts_with(path)
            }) {
                path.join("_default")
            } else {
                path.clone()
            }
        })
        .collect::<Vec<_>>();

    let mut seen = HashSet::new();
    for path in &paths {
        if !seen.insert(path) {
            bail!(
                "multiple tasks map to task stub path {}",
                display_path(path)
            );
        }
    }
    Ok(paths)
}

fn validate_stub_paths(dir: &Path, stubs: &[TaskStub<'_>]) -> Result<Vec<StubMigration>> {
    let mut migrations = HashSet::new();
    for stub in stubs.iter().filter(|stub| stub.legacy_path != stub.path) {
        match fs::symlink_metadata(&stub.legacy_path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                let existing = file::read_to_string(&stub.legacy_path)?;
                if existing != stub.output && existing != stub.legacy_output {
                    bail!(
                        "cannot create nested task stubs because {} is not the generated stub for task {}",
                        display_path(&stub.legacy_path),
                        stub.task.display_name
                    );
                }
                migrations.insert(StubMigration::File(stub.legacy_path.clone()));
            }
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => bail!(
                "cannot create nested task stubs because {} is not a directory",
                display_path(&stub.legacy_path)
            ),
            Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {}
            Err(err) => return Err(err.into()),
        }
    }

    for stub in stubs {
        match fs::symlink_metadata(&stub.path) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                validate_generated_stub_directory(&stub.path, &stub.output, stub.task)?;
                migrations.insert(StubMigration::Directory(stub.path.clone()));
            }
            Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                "cannot write task stub because {} is a symbolic link",
                display_path(&stub.path)
            ),
            Ok(metadata) if metadata.file_type().is_file() => {
                let existing = file::read_to_string(&stub.path)?;
                let legacy_leaf = stub.legacy_path == stub.path && existing == stub.legacy_output;
                if existing != stub.output && !legacy_leaf {
                    bail!(
                        "cannot write task stub because {} is not a generated task stub",
                        display_path(&stub.path)
                    );
                }
            }
            Ok(_) => bail!(
                "cannot write task stub because {} is not a regular file",
                display_path(&stub.path)
            ),
            Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {}
            Err(err) => return Err(err.into()),
        }
        validate_launcher_path(stub)?;
        for parent in stub.path.ancestors().skip(1) {
            match fs::symlink_metadata(parent) {
                Ok(metadata)
                    if metadata.file_type().is_dir()
                        || migrations.contains(&StubMigration::File(parent.to_path_buf())) => {}
                Ok(_) => bail!(
                    "cannot create task stub directory because {} is not a directory",
                    display_path(parent)
                ),
                Err(err)
                    if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {}
                Err(err) => return Err(err.into()),
            }
            if parent == dir {
                break;
            }
        }
    }
    Ok(migrations.into_iter().collect())
}

/// Refuse to replace a `.cmd` beside a stub that mise did not write.
///
/// The stub path is the user's choice, so `bin/<task>.cmd` is a name a project may already be
/// using for a script of its own — and unlike the stub itself, nothing about the name says mise
/// owns it. Checked during validation rather than at the write, so a launcher that is not ours
/// stops the whole run instead of leaving a half-generated `bin/`.
fn validate_launcher_path(stub: &TaskStub<'_>) -> Result<()> {
    let Some(launcher) = super::windows_launcher_path(&stub.path) else {
        return Ok(());
    };
    match fs::symlink_metadata(&launcher) {
        Ok(metadata) if metadata.file_type().is_file() => {
            if !super::is_generated_launcher(&file::read_to_string(&launcher)?) {
                bail!(
                    "cannot write Windows launcher because {} is not a generated launcher",
                    display_path(&launcher)
                );
            }
        }
        Ok(_) => bail!(
            "cannot write Windows launcher because {} is not a regular file",
            display_path(&launcher)
        ),
        Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {}
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

fn validate_generated_stub_directory(path: &Path, expected: &str, task: &Task) -> Result<()> {
    let default = path.join("_default");
    match fs::symlink_metadata(&default) {
        Ok(metadata)
            if metadata.file_type().is_file() && file::read_to_string(&default)? == expected => {}
        _ => bail!(
            "cannot replace task stub directory because {} does not contain the generated stub for task {}",
            display_path(path),
            task.display_name
        ),
    }

    validate_generated_stub_tree(path)?;
    Ok(())
}

fn validate_generated_stub_tree(path: &Path) -> Result<usize> {
    let mut files = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)?;
        if metadata.file_type().is_dir() {
            let child_files = validate_generated_stub_tree(&entry_path)?;
            if child_files == 0 {
                bail!(
                    "cannot replace task stub directory because {} is empty",
                    display_path(&entry_path)
                );
            }
            files += child_files;
        } else if metadata.file_type().is_file()
            && is_generated_task_stub(&file::read_to_string(&entry_path)?)
        {
            files += 1;
        } else if metadata.file_type().is_file()
            && super::is_generated_launcher(&file::read_to_string(&entry_path)?)
        {
            // Our own Windows launcher. Not counted towards `files`: a directory holding nothing
            // but launchers has no stubs left and should still be reported as empty.
        } else {
            bail!(
                "cannot replace task stub directory because {} is not a generated task stub",
                display_path(&entry_path)
            );
        }
    }
    Ok(files)
}

fn is_generated_task_stub(contents: &str) -> bool {
    let mut lines = contents.lines();
    matches!(lines.next(), Some("#!/bin/sh"))
        && matches!(lines.next(), Some("# generated by mise task-stubs"))
        && lines
            .next()
            .and_then(|line| line.strip_prefix("exec "))
            .and_then(|line| line.strip_suffix(" \"$@\""))
            .is_some_and(|line| line.contains(" run "))
        && lines.next().is_none()
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise tasks add test -- echo 'running tests'</bold>
    $ <bold>mise generate task-stubs</bold>
    $ <bold>./bin/test</bold>
    running tests
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_parent_and_nested_task_paths() {
        let paths = resolve_stub_paths(
            Path::new("bin"),
            &[
                PathBuf::from("foo"),
                PathBuf::from("foo/bar"),
                PathBuf::from("foo/bar/baz"),
                PathBuf::from("foobar"),
            ],
        )
        .unwrap();

        assert_eq!(
            paths,
            [
                PathBuf::from("bin/foo/_default"),
                PathBuf::from("bin/foo/bar/_default"),
                PathBuf::from("bin/foo/bar/baz"),
                PathBuf::from("bin/foobar"),
            ]
        );
    }

    #[test]
    fn rejects_duplicate_resolved_paths() {
        let err = resolve_stub_paths(
            Path::new("bin"),
            &[PathBuf::from("foo"), PathBuf::from("foo/_default")],
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("multiple tasks map to task stub path"));
        assert!(message.contains("_default"));
    }
}
