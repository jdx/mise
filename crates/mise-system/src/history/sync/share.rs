//! Saved active file versions for live comparison. Publication always uses
//! the complete ordinary branch, not this machine-specific view.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::Result;

use super::layout::{Located, Roots};
use crate::history::shadow::HistoryRepo;
use crate::history::tracked::TrackedSet;

#[derive(Clone, Debug)]
pub(crate) struct SharedFile {
    pub local: PathBuf,
    pub mode: String,
    pub oid: String,
}

#[derive(Debug, Default)]
pub(crate) struct ShareReport {
    pub files: BTreeMap<String, SharedFile>,
    pub checkpoint: Option<String>,
    /// Paths of selected streams whose saved version this machine does not
    /// hold (see [`crate::history::held`]), each with the version
    /// it last held, if any: their `files` are the live ones, and that
    /// version, not the shared one, is their baseline.
    pub unheld: BTreeMap<String, Option<(String, String)>>,
}

impl ShareReport {
    pub(crate) fn objects(&self) -> BTreeMap<String, (String, String)> {
        self.files
            .iter()
            .map(|(path, file)| (path.clone(), (file.mode.clone(), file.oid.clone())))
            .collect()
    }
}

/// Read committed files, including manually saved files missing on disk.
/// Live directory enumeration is never the authority for a saved version.
pub(crate) fn current(repo: &HistoryRepo, tracked: &TrackedSet) -> Result<ShareReport> {
    if !tracked.invalid.is_empty() {
        eyre::bail!("invalid dotfile declarations; correct them before publishing");
    }
    let mut report = ShareReport::default();
    let Some(head) = repo.ref_oid(HistoryRepo::HISTORY_REF)? else {
        return Ok(report);
    };
    report.checkpoint = Some(head.clone());
    let roots = Roots::current();
    let record = crate::history::held::Held::load(repo)?;
    let mut holds: BTreeMap<String, bool> = BTreeMap::new();
    for file in repo.ls_tree(&head)? {
        let (local, variant) = match roots.locate(&file.path) {
            Located::Config(path) => (path, None),
            Located::Tracked { path, variant } => (path, variant),
            Located::Marker | Located::Unmapped => continue,
        };
        let Some(entry) = tracked.entry_for(&local) else {
            continue;
        };
        if entry.variant != variant {
            continue;
        }
        let stream = entry.tree_path(&entry.path)?;
        let held = match holds.get(&stream) {
            Some(held) => *held,
            None => {
                let held = record.always_holds(&stream)
                    || record.holds(&stream, repo.object_at(&head, &stream)?.as_ref());
                holds.insert(stream.clone(), held);
                held
            }
        };
        // another machine's version is not this one's to compare against:
        // the live file is what this machine has
        let object = if held {
            repo.restored_object_at(&head, &file.path)?
        } else if let Some(live) = usable_live(repo, &local)? {
            // a file missing here while its stream was not selected is not
            // a deletion this machine made: without a base, the pull
            // applies the saved version
            let base = match &live {
                Some(_) => held_base(repo, &head, &record, &stream, &file.path)?,
                None => None,
            };
            report.unheld.insert(file.path.clone(), base);
            live
        } else {
            // a directory or unreadable path where the file belongs is that
            // path's own conflict, decided when the pull plans it, not a
            // reason to stop every other file
            repo.restored_object_at(&head, &file.path)?
        };
        let Some((mode, oid)) = object else {
            continue;
        };
        report
            .files
            .insert(file.path, SharedFile { local, mode, oid });
    }
    Ok(report)
}

/// The version of `path` this machine last held, from its stream's frozen
/// version, to compare the live file with. An encrypted version is
/// compared by what it decrypts to, like the live file; one that cannot be
/// decrypted here gives no baseline, so the pull asks rather than guesses.
fn held_base(
    repo: &HistoryRepo,
    head: &str,
    held: &crate::history::held::Held,
    stream: &str,
    path: &str,
) -> Result<Option<(String, String)>> {
    let Some(frozen) = held.frozen(stream) else {
        return Ok(None);
    };
    let object = if path == stream {
        Some(frozen.clone())
    } else {
        match path
            .strip_prefix(stream)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            Some(relative) if frozen.0 == "040000" => repo.object_at(&frozen.1, relative)?,
            _ => None,
        }
    };
    let Some(object) = object else {
        return Ok(None);
    };
    let encrypted = crate::history::manifest::Manifest::read(repo, head)?.is_some_and(|manifest| {
        manifest.encrypted_paths().iter().any(|prefix| {
            path == prefix
                || path
                    .strip_prefix(prefix.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    });
    if !encrypted {
        return Ok(Some(object));
    }
    Ok(super::files::decrypt(repo, path, &object, console::user_attended_stderr()).ok())
}

/// The live object at `path`, or `None` when the path itself cannot stand
/// in for a file (see [`super::apply::UnusableLive`]).
pub(super) fn usable_live(
    repo: &HistoryRepo,
    path: &std::path::Path,
) -> Result<Option<Option<(String, String)>>> {
    match super::apply::live_object(repo, path) {
        Ok(live) => Ok(Some(live)),
        Err(err) if err.downcast_ref::<super::apply::UnusableLive>().is_some() => Ok(None),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_enrollment_blocks_publication_independently_of_wording() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::history::checkpoint::Store::open_in(temp.path()).unwrap();
        let Some(repo) = store.repo() else { return };
        let tracked = TrackedSet {
            invalid: vec![crate::history::store::PathReason {
                path: "unrepresentable".into(),
                reason: "not a portable location".into(),
            }],
            ..Default::default()
        };
        assert!(current(repo, &tracked).is_err());
    }
}
