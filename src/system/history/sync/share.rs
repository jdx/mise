//! Saved active file versions for live comparison. Publication always uses
//! the complete ordinary branch, not this machine-specific view.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::Result;

use super::layout::{Located, Roots};
use crate::system::history::shadow::HistoryRepo;
use crate::system::history::tracked::TrackedSet;

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
    if tracked
        .invalid
        .iter()
        .any(|entry| entry.reason.contains("encrypt"))
    {
        eyre::bail!("invalid dotfile declarations; correct them before publishing");
    }
    let mut report = ShareReport::default();
    let Some(head) = repo.ref_oid(HistoryRepo::HISTORY_REF)? else {
        return Ok(report);
    };
    report.checkpoint = Some(head.clone());
    let roots = Roots::current();
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
        let Some((mode, oid)) = repo.restored_object_at(&head, &file.path)? else {
            continue;
        };
        report
            .files
            .insert(file.path, SharedFile { local, mode, oid });
    }
    Ok(report)
}
