//! Removal of daemon state left behind by deleted project roots.
//!
//! Every project root (including each linked git worktree) owns one directory
//! under `$MISE_STATE_DIR/daemons/`. Removing the root leaves that directory,
//! its registered pitchfork configuration, and any database data behind.
//! Selection here is pure filesystem inspection so it can be tested without
//! pitchfork; the pitchfork calls live in [`remove`].

use super::runtime::{Runtime, State};
use crate::file::display_path;
use eyre::Result;
use std::path::{Path, PathBuf};

/// One `<state-dir>/state.json` and the directory that contains it.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub dir: PathBuf,
    pub state: State,
}

impl Entry {
    pub(crate) fn config_file(&self) -> PathBuf {
        self.dir.join("pitchfork.toml")
    }

    /// Whether the project this state belongs to no longer exists on disk.
    ///
    /// A missing path is necessary but not sufficient. `Path::is_dir` would
    /// answer false for every metadata error, so a share that is down or a
    /// directory mise cannot stat would read as a deleted project and cost the
    /// user a database; only a definite `NotFound` counts. A root that still
    /// exists keeps its state even when it declares no daemons any more:
    /// `prepare()` already unregisters its configuration, and its data may
    /// still be wanted.
    ///
    /// Whether that absence is enough on its own is a separate question, which
    /// [`Self::ambiguity`] answers.
    pub(crate) fn orphaned(&self) -> bool {
        if self.state.root.as_os_str().is_empty() {
            return false;
        }
        match std::fs::symlink_metadata(&self.state.root) {
            Ok(_) => false,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                // Reported by whoever is about to act on this, not here:
                // `mise daemons start` asks the same question on every run and
                // has no business complaining about another project's volume.
                debug!(
                    "keeping {}: cannot read {}: {err}",
                    display_path(&self.dir),
                    display_path(&self.state.root)
                );
                false
            }
        }
    }

    /// Why this entry's root could not be examined, if it could not.
    ///
    /// Distinct from [`Self::ambiguity`]: nothing can be decided about such an
    /// entry at all, so it is not a prune candidate. Worth saying once, where
    /// somebody is looking at prune's output, rather than on every command that
    /// happens to scan.
    pub(crate) fn unreadable_root(&self) -> Option<String> {
        match std::fs::symlink_metadata(&self.state.root) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Some(format!(
                "keeping {}: cannot read {}: {err}",
                display_path(&self.dir),
                display_path(&self.state.root)
            )),
            _ => None,
        }
    }

    /// Why a missing root might mean something other than a deleted project,
    /// if it might.
    ///
    /// Two cases are indistinguishable from a deleted project on disk, and
    /// neither can be settled by looking harder, so each is put to a person
    /// instead: never removed without an explicit answer, and never by `--yes`.
    ///
    /// A volume that is not mounted reports `NotFound` under it exactly as a
    /// deleted project does, and it goes missing in two shapes. Some mounts
    /// leave their mount point behind as an empty directory; others take the
    /// whole thing with them, as macOS does with `/Volumes/Name` and Windows
    /// with a drive letter. So two signs stand in for it: the first ancestor
    /// that still exists is empty or cannot be listed, or the project's own
    /// parent is gone as well. Deleting a project removes the project, not the
    /// directory it sat in.
    ///
    /// A project reached through a symlink has its state directory named for
    /// the canonical target while an older `prepare()` recorded the alias, so
    /// deleting only the symlink leaves a live project whose recorded root
    /// reads as missing. A recorded path that no longer names its own directory
    /// is the sign of that. It is not proof: `canonicalize` also rewrites
    /// `/tmp` on macOS and every path on Windows, so ordinary old state can
    /// carry a spelling its directory was not named from. Hence a question
    /// rather than a refusal, which would strand that state forever.
    pub(crate) fn ambiguity(&self) -> Option<String> {
        if let Some(parent) = self.state.root.parent()
            && !parent.exists()
        {
            return Some(format!(
                "{} is gone as well, so {} may be an unmounted volume rather than a deleted project",
                display_path(parent),
                display_path(&self.state.root)
            ));
        }
        if let Some(existing) = self.state.root.ancestors().skip(1).find(|p| p.exists()) {
            let looks_unmounted = match std::fs::read_dir(existing) {
                Ok(mut dir) => dir.next().is_none(),
                // A mount point nobody else may read is still a mount point.
                Err(_) => true,
            };
            if looks_unmounted {
                return Some(format!(
                    "{} is empty or unreadable, so {} may be an unmounted volume rather than a deleted project",
                    display_path(existing),
                    display_path(&self.state.root)
                ));
            }
        }
        if super::state_dir(&self.state.root).file_name() != self.dir.file_name() {
            return Some(format!(
                "{} is not the path {} was created for, which may still exist",
                display_path(&self.state.root),
                display_path(&self.dir)
            ));
        }
        None
    }
}

/// What [`remove`] did with one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The state directory and its data are gone.
    Removed,
    /// Left in place. A warning says why, and a later run can retry.
    Kept,
}

/// The directory holding every project's daemon state.
pub(crate) fn base_dir() -> PathBuf {
    crate::dirs::STATE.join("daemons")
}

/// Every readable `state.json` directly below `base`, in path order.
///
/// A directory without a state file was never fully prepared and is skipped
/// silently. One whose state file cannot be read or parsed is skipped loudly:
/// mise will not guess at data it does not understand, but such a directory can
/// never be pruned either, so saying nothing would leave it invisible forever.
/// One unreadable entry does not hide the rest.
pub(crate) fn scan(base: &Path) -> Result<Vec<Entry>> {
    let Ok(read_dir) = std::fs::read_dir(base) else {
        return Ok(vec![]);
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        let dir = match entry {
            Ok(entry) => entry.path(),
            Err(err) => {
                warn!("skipping an entry of {}: {err}", display_path(base));
                continue;
            }
        };
        // The lock files guarding these directories sit beside them, and a path
        // under a file is not a missing file, so they would otherwise be
        // reported as unreadable state. `is_dir` would answer false for a
        // metadata error too, which is the way to skip a real state directory
        // without anyone hearing about it.
        match std::fs::metadata(&dir) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => continue,
            Err(err) => {
                warn!("cannot read {}: {err}", display_path(&dir));
                continue;
            }
        }
        let path = dir.join("state.json");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            // Not every directory here has one; only an existing file that
            // cannot be read is worth reporting.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                warn!("cannot read {}: {err}", display_path(&path));
                continue;
            }
        };
        match serde_json::from_slice::<State>(&bytes) {
            Ok(state) => entries.push(Entry { dir, state }),
            Err(err) => warn!(
                "cannot understand {}, so it will not be pruned: {err}",
                display_path(&path)
            ),
        }
    }
    entries.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(entries)
}

/// Entries whose project root has been deleted.
pub(crate) fn orphans(base: &Path) -> Result<Vec<Entry>> {
    Ok(scan(base)?.into_iter().filter(Entry::orphaned).collect())
}

/// Total size in bytes of everything below `path`; `0` when it does not exist.
/// Symlinks are not followed, so a linked data directory counts once.
pub(crate) fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum()
}

pub(crate) fn human_size(bytes: u64) -> String {
    bytesize::ByteSize::b(bytes).display().iec().to_string()
}

/// One line per orphan for the confirmation prompt and dry-run output.
pub(crate) fn describe(entries: &[(Entry, u64)]) -> Vec<String> {
    entries
        .iter()
        .map(|(entry, size)| {
            format!(
                "{} ({}) from deleted {}",
                display_path(&entry.dir),
                human_size(*size),
                display_path(&entry.state.root)
            )
        })
        .collect()
}

/// Stops the entry's daemons, unregisters its generated configuration, and
/// deletes the state directory including data.
///
/// Every step that cannot be confirmed keeps the state. A pitchfork call that
/// times out or errors may leave a daemon alive on top of the very data this
/// would delete, so the entry is left for a later run rather than deleted on an
/// unverified assumption. The same applies when pitchfork cannot be located at
/// all (`runtime` is `None`): deleting the directory would take `state.json`
/// and the generated configuration with it, which is the only record of the
/// registration a later run could act on.
pub(crate) async fn remove(entry: &Entry, runtime: Option<&Runtime>) -> Result<Outcome> {
    // The root is gone, so pitchfork runs from the state directory instead.
    let cwd = &entry.dir;
    let lock = match super::ProjectLock::try_acquire(cwd) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            warn!(
                "keeping {}: another mise process holds its daemon lock",
                display_path(cwd)
            );
            return Ok(Outcome::Kept);
        }
        // One unwritable leftover directory must not end the run; the other
        // entries are independent of this one.
        Err(err) => {
            warn!("keeping {}: cannot lock it: {err:#}", display_path(cwd));
            return Ok(Outcome::Kept);
        }
    };
    // Selection and confirmation both happen before this lock is held, and a
    // project directory can come back in between (a restored worktree, a
    // re-clone). Ask again now that nothing else can prepare this state.
    if !entry.orphaned() {
        warn!(
            "keeping {}: {} exists again",
            display_path(cwd),
            display_path(&entry.state.root)
        );
        return Ok(Outcome::Kept);
    }
    if let Some(runtime) = runtime {
        match runtime.supervisor_up(cwd).await {
            // A supervisor that is down cannot be asked to stop anything, but
            // it did not necessarily take its children with it, so the daemons
            // are checked either way below.
            Ok(false) => {}
            Ok(true) if !entry.state.ids.is_empty() => {
                // One id at a time: `state.ids` keeps every id this project
                // ever declared, and pitchfork asked to stop a list it cannot
                // fully resolve may refuse the whole list, leaving a live
                // daemon running on data that is about to go.
                for id in &entry.state.ids {
                    let args = ["stop".to_string(), id.clone()];
                    if let Err(err) = runtime.raw_output(cwd, &args).await {
                        warn!("keeping {}: cannot stop {id}: {err:#}", display_path(cwd));
                        return Ok(Outcome::Kept);
                    }
                }
            }
            Ok(_) => {}
            Err(err) => {
                warn!(
                    "keeping {}: cannot establish supervisor status: {err:#}",
                    display_path(cwd)
                );
                return Ok(Outcome::Kept);
            }
        }
        // Whatever the supervisor said about itself, every id this project ever
        // declared has to be accounted for: a supervisor that crashed can leave
        // a database running, and not every database writes a lock file to find
        // it by. Redis, for one, does not.
        for id in &entry.state.ids {
            if let Err(err) = confirm_stopped(runtime, cwd, id).await {
                warn!("keeping {}: {err:#}", display_path(cwd));
                return Ok(Outcome::Kept);
            }
        }
        // Unconditionally, even when the generated file is already gone:
        // pitchfork keeps the registration separately, and skipping the call
        // would leave it pointing at a path nothing can discover again once
        // this directory is deleted.
        let config = entry.config_file();
        let args = [
            "config".to_string(),
            "remove".to_string(),
            config.to_string_lossy().into_owned(),
        ];
        if let Err(err) = tolerate_unknown(runtime.raw_output(cwd, &args).await, &args) {
            warn!(
                "keeping {}: cannot unregister {}: {err:#}",
                display_path(cwd),
                display_path(config)
            );
            return Ok(Outcome::Kept);
        }
    } else {
        warn!(
            "keeping {}: pitchfork is required to stop its daemons and unregister {}; run `mise use pitchfork`",
            display_path(cwd),
            display_path(entry.config_file())
        );
        return Ok(Outcome::Kept);
    }
    // Last thing before anything goes: a database writes a lock file beside its
    // data while it is alive, and one still sitting there means a process may be
    // using what is about to be deleted -- a supervisor that crashed without its
    // children, or a stop that pitchfork reported as nothing to do.
    if let Some(marker) = live_database_lock(cwd) {
        warn!(
            "keeping {}: {} is still there, so a database may still be running",
            display_path(cwd),
            display_path(&marker)
        );
        return Ok(Outcome::Kept);
    }
    if let Err(err) = delete_state_dir(cwd, lock) {
        // One unreadable or busy file must not end the run: the other entries
        // are independent, and this one stays discoverable for a later run.
        warn!("keeping {}: {err:#}", display_path(cwd));
        return Ok(Outcome::Kept);
    }
    Ok(Outcome::Removed)
}

/// Confirms that pitchfork is not running this daemon.
///
/// Only two answers allow the data to go: a daemon pitchfork reports as not
/// running, and an id pitchfork does not have at all, which `state.ids`
/// accumulates and which cannot be running by definition. Everything else --
/// a timeout, a reply that will not parse, a state field that is missing, a
/// failure that says something other than "unknown" -- means the question was
/// not answered, and an unanswered question here is a live process writing to
/// the data below. The database lock scan is a second line of defence, not a
/// substitute: it only knows the markers it recognizes.
async fn confirm_stopped(runtime: &Runtime, cwd: &Path, id: &str) -> Result<()> {
    let args = ["status".to_string(), id.to_string(), "--json".to_string()];
    let output = runtime
        .raw_output(cwd, &args)
        .await
        .map_err(|err| eyre::eyre!(err).wrap_err(format!("cannot check {id}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Tolerated only when pitchfork says which id it does not have. A
        // failure that says nothing at all is a crash, and "No such file or
        // directory" on its own is an ordinary I/O error -- a missing
        // supervisor socket, say. Neither is pitchfork telling us the daemon is
        // not running, and this is the last check before the data goes.
        if names_this_as_unknown(&stderr, id) {
            debug!("pitchfork does not know {id}: {stderr}");
            return Ok(());
        }
        eyre::bail!("cannot check {id}: {stderr}");
    }
    let status: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|err| eyre::eyre!("cannot read the status of {id}: {err}"))?;
    match status["status"].as_str() {
        Some("running" | "waiting" | "stopping") => {
            eyre::bail!("{id} is still {}", status["status"].as_str().unwrap_or(""))
        }
        Some(_) => Ok(()),
        None => eyre::bail!("cannot tell whether {id} is running: {status}"),
    }
}

/// Whether a pitchfork failure is it saying it has never heard of something.
///
/// Deliberately narrow. Phrases like "unknown" or "is not" turn up in plenty of
/// real failures, and tolerating one of those would delete a database whose
/// daemon is still running.
fn names_something_unknown(stderr: &str) -> bool {
    let lowered = stderr.to_lowercase();
    ["not found", "no such", "not registered"]
        .iter()
        .any(|phrase| lowered.contains(phrase))
}

/// Whether a pitchfork failure is it saying it has never heard of `id`.
///
/// The same phrases, but they have to be about this daemon. "No such file or
/// directory" is what a missing socket reports too, and a daemon nobody can
/// reach is not a daemon that has stopped.
fn names_this_as_unknown(stderr: &str, id: &str) -> bool {
    if !names_something_unknown(stderr) {
        return false;
    }
    let lowered = stderr.to_lowercase();
    let name = id.rsplit('/').next().unwrap_or(id).to_lowercase();
    lowered.contains(&id.to_lowercase()) || lowered.contains(&name)
}

/// Turns a pitchfork invocation into success, a tolerated non-failure, or an
/// error.
///
/// Forgetting something pitchfork has never heard of is the outcome prune
/// wants, but pitchfork reports it as a failure. `prepare()` may already have
/// unregistered a configuration file, so "not registered" is ordinary here.
/// Treating it as an error would make an entry permanently unprunable, with
/// `rm -rf` as the only way out.
///
/// Used only where the result can be checked another way or cannot cost data:
/// whether daemons are stopped is settled by asking the supervisor, never by
/// reading a message.
fn tolerate_unknown(output: Result<std::process::Output>, args: &[String]) -> Result<()> {
    let output = output?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if names_something_unknown(&stderr) {
        debug!("pitchfork {} had nothing to do: {stderr}", args.join(" "));
        return Ok(());
    }
    eyre::bail!("pitchfork {}: {stderr}", args.join(" "))
}

/// The lock file of a database that is still running on this project's data, if
/// there is one.
///
/// Database daemons drop a lock file beside their data while they are alive:
/// PostgreSQL writes `postmaster.pid`, and other engines keep a comparable
/// marker. A crash leaves the file behind, so its presence alone proves
/// nothing, which is why the process it names has to be asked about too. A
/// marker naming a process that is gone is not a reason to keep data forever.
fn live_database_lock(dir: &Path) -> Option<PathBuf> {
    walkdir::WalkDir::new(dir.join("data"))
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name == "postmaster.pid" || name.ends_with(".pid") || name == "LOCK"
            })
        })
        .find(|path| marker_names_a_live_process(path))
}

/// Whether a lock file names a process that is still around.
///
/// A pid file's first line is the pid, which is all that can be checked without
/// knowing the engine. A marker that holds no pid at all, such as a RocksDB or
/// Pebble `LOCK`, says nothing either way; those are treated as live, since a
/// file with no pid in it is the one case where there is nothing to disprove.
fn marker_names_a_live_process(path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return true;
    };
    let Some(pid) = contents
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<i32>().ok())
    else {
        return true;
    };
    process_is_alive(pid)
}

#[cfg(unix)]
fn process_is_alive(pid: i32) -> bool {
    // Signal 0 performs the permission and existence checks without sending
    // anything. `EPERM` means the process is there and owned by someone else.
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
        Ok(()) => true,
        Err(nix::errno::Errno::EPERM) => true,
        Err(_) => false,
    }
}

#[cfg(windows)]
fn process_is_alive(_pid: i32) -> bool {
    // No cheap equivalent here, and the presets that write these files are
    // Unix-only for now, so the file is taken at its word.
    true
}

/// Deletes a project's daemon state and data, under both locks.
///
/// Nothing here is released early, so no `prepare()` -- from this mise or from
/// an older one holding only the in-directory lock -- can be writing while the
/// files go. That is what keeps the removal free of any check-then-delete gap,
/// and it is why the two lock files are the one thing left behind: the inner
/// one cannot be deleted while it is held, which Windows enforces, and
/// releasing it first would reopen the window it exists to close. Both are
/// empty, and mise leaves its lock files in place everywhere else too.
///
/// `state.json` goes last. It is what makes a directory an entry at all, so if
/// a file above it turns out to be busy or unreadable, what is left is still
/// selected, still reported, and still retried.
fn delete_state_dir(dir: &Path, lock: super::ProjectLock) -> Result<()> {
    let state_file = dir.join("state.json");
    let legacy = super::legacy_lock_file_for_state_dir(dir);
    for child in std::fs::read_dir(dir)? {
        let path = child?.path();
        if path == state_file || path == legacy {
            continue;
        }
        crate::file::remove_all(path)?;
    }
    crate::file::remove_all(&state_file)?;
    drop(lock);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_state(base: &Path, name: &str, root: &Path, data: &[(&str, usize)]) -> PathBuf {
        // Named by the hash of the canonical root, as `state_dir` names it.
        let dir = base.join(crate::hash::hash_to_str(
            &root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let state = State {
            root: root.to_path_buf(),
            namespace: format!("{name}-ns"),
            ids: vec![format!("{name}-ns/db")],
            ..State::default()
        };
        std::fs::write(
            dir.join("state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("pitchfork.toml"), "[daemons]\n").unwrap();
        for (file, size) in data {
            let path = dir.join("data").join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![b'x'; *size]).unwrap();
        }
        dir
    }

    #[test]
    fn selects_only_states_whose_root_is_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let live_root = tmp.path().join("live");
        std::fs::create_dir(&live_root).unwrap();
        let deleted_root = tmp.path().join("deleted");
        std::fs::create_dir(&deleted_root).unwrap();
        write_state(&base, "aaa-live", &live_root, &[("db/one", 10)]);
        let orphan = write_state(
            &base,
            "bbb-gone",
            &deleted_root,
            &[("db/one", 100), ("db/sub/two", 24)],
        );
        std::fs::remove_dir_all(&deleted_root).unwrap();
        // A directory that was never prepared has no state.json and is ignored.
        std::fs::create_dir_all(base.join("ccc-partial").join("data")).unwrap();
        std::fs::create_dir_all(base.join("ddd-corrupt")).unwrap();
        std::fs::write(base.join("ddd-corrupt/state.json"), "{").unwrap();

        let all = scan(&base).unwrap();
        assert_eq!(all.len(), 2);
        let orphans = orphans(&base).unwrap();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].dir, orphan);
        assert_eq!(orphans[0].state.root, deleted_root);
        assert_eq!(dir_size(&orphan.join("data")), 124);

        let lines = describe(&[(orphans[0].clone(), dir_size(&orphan))]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with(&display_path(&orphan)), "{}", lines[0]);
        assert!(lines[0].contains("from deleted"), "{}", lines[0]);
        assert!(
            lines[0].ends_with(&display_path(&deleted_root)),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn missing_base_and_live_roots_yield_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(orphans(&tmp.path().join("none")).unwrap().is_empty());
        let base = tmp.path().join("daemons");
        write_state(&base, "live", tmp.path(), &[]);
        assert!(orphans(&base).unwrap().is_empty());
        assert_eq!(dir_size(&tmp.path().join("nothing")), 0);
    }

    #[tokio::test]
    async fn state_survives_a_held_lock_and_a_missing_pitchfork() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let gone = tmp.path().join("gone");
        let dir = write_state(&base, "gone", &gone, &[("db/one", 1)]);
        let entry = orphans(&base).unwrap().remove(0);

        let held = super::super::ProjectLock::try_acquire(&dir)
            .unwrap()
            .unwrap();
        assert_eq!(remove(&entry, None).await.unwrap(), Outcome::Kept);
        assert!(dir.join("state.json").exists(), "locked state must survive");
        drop(held);

        // Without pitchfork nothing can be stopped or unregistered, and
        // deleting state.json would destroy the record a later run needs.
        assert_eq!(remove(&entry, None).await.unwrap(), Outcome::Kept);
        assert!(dir.join("state.json").exists());
    }

    #[tokio::test]
    async fn the_lock_guarding_a_state_directory_outlives_it() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let dir = write_state(&base, "gone", &tmp.path().join("gone"), &[("db/one", 1)]);
        let lock = super::super::lock_file_for_state_dir(&dir);
        // A sibling, not a file inside the directory being deleted: prune holds
        // it across the removal, so no other process can write state into a
        // directory that is on its way out.
        assert_eq!(lock.parent(), dir.parent());
        assert!(!lock.starts_with(&dir));
        // The lock an older mise takes is still honored, so a start from one
        // and a prune from this version continue to exclude each other.
        let legacy = super::super::legacy_lock_file_for_state_dir(&dir);
        let held = crate::lock_file::LockFile::at(&legacy).try_lock().unwrap();
        let entry = orphans(&base).unwrap().remove(0);
        assert_eq!(remove(&entry, None).await.unwrap(), Outcome::Kept);
        assert!(dir.join("state.json").exists());
        drop(held);

        assert_eq!(remove(&entry, None).await.unwrap(), Outcome::Kept);
        // A lock file is not a state directory, so it is never itself an entry
        // and never reported as unreadable state either.
        std::fs::write(&lock, "").unwrap();
        assert_eq!(scan(&base).unwrap().len(), 1);
    }

    /// Unix only: the failure is injected with POSIX permissions, which is the
    /// one portable way to make a delete fail part way through. The ordering it
    /// checks is platform independent.
    #[test]
    #[cfg(unix)]
    fn a_partial_delete_leaves_an_entry_that_is_still_found() {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let dir = write_state(&base, "gone", &tmp.path().join("gone"), &[("db/one", 1)]);
        // Stand in for a file that cannot be removed: a directory entry that
        // read_dir reports and remove_all then fails on.
        let busy = dir.join("data");
        let mut perms = std::fs::metadata(&busy).unwrap().permissions();
        perms.set_mode(0o500);
        std::fs::set_permissions(&busy, perms).unwrap();

        let lock = super::super::ProjectLock::try_acquire(&dir)
            .unwrap()
            .unwrap();
        let result = delete_state_dir(&dir, lock);
        // Root ignores the mode, so which outcome is correct depends on who is
        // running the test; asserting the wrong one, or either, would let this
        // pass without exercising anything.
        if std::fs::metadata(tmp.path()).unwrap().uid() == 0 {
            result.unwrap();
            assert!(!dir.join("state.json").exists());
        } else {
            assert!(
                result.is_err(),
                "a directory it cannot read must not vanish"
            );
            assert!(dir.join("state.json").exists());
            assert_eq!(orphans(&base).unwrap().len(), 1);
        }
    }

    #[test]
    fn deletion_keeps_only_the_lock_files_that_made_it_safe() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let dir = write_state(&base, "gone", &tmp.path().join("gone"), &[("db/one", 64)]);
        let legacy = super::super::legacy_lock_file_for_state_dir(&dir);
        let lock = super::super::ProjectLock::try_acquire(&dir)
            .unwrap()
            .unwrap();

        delete_state_dir(&dir, lock).unwrap();

        // The data, the generated configuration and the state are gone; the
        // in-directory lock stays, because it is held for the whole removal and
        // releasing it early is the race this design avoids.
        assert!(!dir.join("data").exists());
        assert!(!dir.join("pitchfork.toml").exists());
        assert!(!dir.join("state.json").exists());
        assert!(legacy.exists());
        assert_eq!(dir_size(&dir), 0);
        // Without state.json it is no longer an entry, so it is neither
        // reported nor pruned again.
        assert!(scan(&base).unwrap().is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn a_deleted_alias_is_never_removed_without_being_asked_about() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let project = tmp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&project, &alias).unwrap();

        // What `prepare()` records when the project was reached through the
        // alias: the raw path, against a directory named for the canonical one.
        let dir = base.join(crate::hash::hash_to_str(&project));
        std::fs::create_dir_all(&dir).unwrap();
        let state = State {
            root: alias.clone(),
            ..State::default()
        };
        std::fs::write(
            dir.join("state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("data/db")).unwrap();

        // Removing only the symlink leaves the project, and its database,
        // entirely intact. The recorded root now reads as missing, so what
        // keeps this state is the caveat, not the selection.
        std::fs::remove_file(&alias).unwrap();
        assert!(project.is_dir());
        let entry = orphans(&base).unwrap().remove(0);
        let why = entry.ambiguity().expect("must not be deleted unasked");
        assert!(why.contains("may still exist"), "{why}");

        // State recorded the way `prepare()` records it now carries no caveat.
        std::fs::remove_dir_all(&project).unwrap();
        let dir = write_state(&base, "canonical", &project, &[]);
        let entry = orphans(&base)
            .unwrap()
            .into_iter()
            .find(|e| e.dir == dir)
            .unwrap();
        assert_eq!(entry.ambiguity(), None);
    }

    #[test]
    fn a_root_whose_parent_is_gone_too_is_flagged_for_a_person() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        // An unplugged disk takes its whole mount point with it on macOS, and a
        // drive letter does the same on Windows, so the first existing ancestor
        // is an ordinary busy directory.
        let volumes = tmp.path().join("Volumes");
        std::fs::create_dir(&volumes).unwrap();
        std::fs::create_dir(volumes.join("Another")).unwrap();
        write_state(&base, "unplugged", &volumes.join("Disk/project"), &[]);

        let entry = orphans(&base).unwrap().remove(0);
        let why = entry
            .ambiguity()
            .expect("an unplugged disk must reach a person");
        assert!(why.contains("unmounted volume"), "{why}");
    }

    #[test]
    fn a_root_under_an_empty_ancestor_is_flagged_for_a_person() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        // An unmounted mount point is an ordinary empty directory, and a path
        // under it reports NotFound exactly as a deleted project does.
        let mount = tmp.path().join("mnt");
        std::fs::create_dir(&mount).unwrap();
        write_state(&base, "unmounted", &mount.join("project"), &[]);
        let entry = orphans(&base).unwrap().remove(0);
        let why = entry.ambiguity().expect("must reach a person");
        assert!(why.contains("unmounted volume"), "{why}");

        // A deleted project leaves its parent behind with other things in it.
        let projects = tmp.path().join("src");
        std::fs::create_dir(&projects).unwrap();
        std::fs::create_dir(projects.join("other-project")).unwrap();
        write_state(&base, "deleted", &projects.join("project"), &[]);
        let entry = orphans(&base)
            .unwrap()
            .into_iter()
            .find(|e| e.state.root.starts_with(&projects))
            .unwrap();
        assert_eq!(entry.ambiguity(), None);
    }

    #[test]
    #[cfg(unix)]
    fn state_that_cannot_be_examined_is_reported_and_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        write_state(&base, "gone", &tmp.path().join("gone"), &[]);
        // Readable but not searchable: `read_dir` still lists the entry while
        // every `metadata` call on it fails, which is the case `is_dir` would
        // answer with a plain false.
        let mut perms = std::fs::metadata(&base).unwrap().permissions();
        perms.set_mode(0o400);
        std::fs::set_permissions(&base, perms).unwrap();

        let scanned = scan(&base);

        let mut perms = std::fs::metadata(&base).unwrap().permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(&base, perms).unwrap();
        // Skipped rather than fatal, and warned about rather than silent.
        assert!(scanned.unwrap().is_empty());
        assert_eq!(scan(&base).unwrap().len(), 1);
    }

    #[test]
    fn state_from_another_version_is_still_selectable() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let gone = tmp.path().join("gone");
        let dir = base.join(crate::hash::hash_to_str(&gone));
        std::fs::create_dir_all(&dir).unwrap();
        // Only the field that matters for selection, plus one this version has
        // never heard of. A state file that cannot be parsed is state that can
        // never be cleaned up, so every field has to be optional.
        std::fs::write(
            dir.join("state.json"),
            format!(
                r#"{{"root":{:?},"something_new":42}}"#,
                gone.to_str().unwrap()
            ),
        )
        .unwrap();
        let entry = orphans(&base).unwrap().remove(0);
        assert_eq!(entry.state.root, gone);
        assert!(entry.state.ids.is_empty());
        assert_eq!(entry.ambiguity(), None);
    }

    #[test]
    fn state_that_cannot_be_understood_is_reported_and_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        write_state(&base, "fine", &tmp.path().join("gone"), &[]);
        std::fs::create_dir_all(base.join("corrupt")).unwrap();
        std::fs::write(base.join("corrupt/state.json"), "{not json").unwrap();
        // The readable entry is still found; the corrupt one is skipped, and
        // `scan` warns rather than passing over it in silence.
        assert_eq!(scan(&base).unwrap().len(), 1);
    }

    #[test]
    fn a_database_lock_counts_only_while_its_process_does() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let dir = write_state(&base, "gone", &tmp.path().join("gone"), &[]);
        let marker = dir.join("data/postgres/postmaster.pid");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        assert!(live_database_lock(&dir).is_none());

        // PostgreSQL's file, with the pid on the first line.
        std::fs::write(&marker, format!("{}\n/data\n", std::process::id())).unwrap();
        assert_eq!(live_database_lock(&dir).as_deref(), Some(marker.as_path()));

        // A crash leaves the same file behind naming a process that is gone,
        // which must not keep the state forever.
        std::fs::write(&marker, format!("{}\n/data\n", i32::MAX)).unwrap();
        assert!(live_database_lock(&dir).is_none());

        // A marker with no pid in it, such as RocksDB's, cannot be disproved.
        let lock = dir.join("data/crdb/LOCK");
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        std::fs::write(&lock, "").unwrap();
        assert_eq!(live_database_lock(&dir).as_deref(), Some(lock.as_path()));
    }

    #[test]
    #[cfg(unix)]
    fn an_ancestor_that_cannot_be_read_is_treated_as_a_mount_point() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let mount = tmp.path().join("mnt");
        std::fs::create_dir(&mount).unwrap();
        write_state(&base, "unreadable", &mount.join("project"), &[]);
        // Searchable but not listable, which is what a mount point owned by
        // somebody else looks like: the path below it still answers "not
        // found", while its contents cannot be counted.
        let mut perms = std::fs::metadata(&mount).unwrap().permissions();
        perms.set_mode(0o111);
        std::fs::set_permissions(&mount, perms).unwrap();

        let entries = orphans(&base).unwrap();
        let ambiguity = entries.first().map(|entry| entry.ambiguity());

        let mut perms = std::fs::metadata(&mount).unwrap().permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(&mount, perms).unwrap();

        // Root reads it regardless and finds it empty, which reaches the same
        // answer by the other route.
        let ambiguity =
            ambiguity.expect("the root itself is still missing, so this is a candidate");
        let why = ambiguity.expect("an ancestor that cannot be listed must reach a person");
        assert!(why.contains("unmounted volume"), "{why}");
    }

    /// An exit status that means failure, built the way each platform builds
    /// one: `from_raw` takes a wait status on Unix and an exit code on Windows.
    fn failure() -> std::process::ExitStatus {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(256)
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(1)
        }
    }

    #[test]
    fn pitchfork_forgetting_something_it_never_knew_is_not_a_failure() {
        let args = vec!["stop".to_string(), "ns/db".to_string()];
        let failed = |stderr: &str| {
            Ok(std::process::Output {
                status: failure(),
                stdout: vec![],
                stderr: stderr.as_bytes().to_vec(),
            })
        };
        assert!(tolerate_unknown(failed("daemon ns/db not found"), &args).is_ok());
        // The status check asks for more: the message has to be about the
        // daemon in hand, since the last thing after it is a recursive delete.
        assert!(names_this_as_unknown("daemon ns/db not found", "ns/db"));
        assert!(names_this_as_unknown("no such daemon: db", "ns/db"));
        assert!(!names_this_as_unknown("", "ns/db"));
        assert!(!names_this_as_unknown(
            "No such file or directory (os error 2)",
            "ns/db"
        ));
        assert!(!names_this_as_unknown("daemon ns/other not found", "ns/db"));
        assert!(tolerate_unknown(failed("no such config"), &args).is_ok());
        assert!(tolerate_unknown(failed("permission denied"), &args).is_err());
        // Phrases that turn up in real failures are not tolerated: treating one
        // of these as "nothing to do" would delete a running database's data.
        assert!(tolerate_unknown(failed("unknown error from supervisor"), &args).is_err());
        assert!(tolerate_unknown(failed("daemon is not responding"), &args).is_err());
    }

    #[test]
    fn a_root_that_cannot_be_confirmed_missing_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        // A path that exists but is not a directory: `is_dir()` answers false
        // for it exactly as it does for an unreadable one, and neither is the
        // definite absence that makes state safe to delete.
        let file_root = tmp.path().join("root-is-a-file");
        std::fs::write(&file_root, "").unwrap();
        write_state(&base, "file-root", &file_root, &[]);
        assert!(orphans(&base).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_root_that_reappears_before_the_lock_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let root = tmp.path().join("restored");
        let dir = write_state(&base, "restored", &root, &[]);
        let entry = orphans(&base).unwrap().remove(0);

        // Selection and the confirmation prompt both ran while the project was
        // gone; a restored worktree between then and now must not be deleted.
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(remove(&entry, None).await.unwrap(), Outcome::Kept);
        assert!(dir.join("state.json").exists());
    }
}
