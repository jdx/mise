//! What this machine shares right now: every captured file whose policy
//! allows sharing, mapped to its setup-branch path, with the saved version
//! (the newest checkpoint's snapshot, which already carries manual-save
//! entries forward from the promotion chain). Ours is always the saved
//! version, never the live file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::Result;

use super::layout::Roots;
use crate::file::display_path;
use crate::system::history::checkpoint::Store;
use crate::system::history::shadow::HistoryRepo;
use crate::system::history::tracked::{EntryKind, TrackedSet, display_to_tree_path};

#[derive(Clone, Debug)]
pub(crate) struct SharedFile {
    pub local: PathBuf,
    pub mode: String,
    pub oid: String,
}

/// Why a captured file is not shared.
#[derive(Clone, Debug)]
pub(crate) struct Unshared {
    pub local: PathBuf,
    pub reason: String,
    /// Where the file would land in the setup branch, so a copy committed
    /// there counts as private content (`None` when it has no place there).
    pub branch_path: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct ShareReport {
    /// By setup-branch path.
    pub files: BTreeMap<String, SharedFile>,
    pub unshared: Vec<Unshared>,
    /// Private-by-default files a per-file declaration shares anyway.
    pub overrides: Vec<PathBuf>,
    /// The checkpoint the saved versions come from.
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

/// Computes the shareable set from the newest checkpoint.
pub(crate) fn current(
    repo: &HistoryRepo,
    store: &Store,
    tracked: &TrackedSet,
) -> Result<ShareReport> {
    let mut report = ShareReport::default();
    let Some(latest) = store.list()?.into_iter().last() else {
        return Ok(report);
    };
    let Some(snapshot) = latest.checkpoint.tree.snapshot.clone() else {
        return Ok(report);
    };
    report.checkpoint = Some(latest.checkpoint.uuid.clone());
    let walk = tracked.walk()?;
    let roots = Roots::current();
    let private: BTreeMap<PathBuf, String> = walk
        .private
        .iter()
        .map(|file| (file.path.clone(), file.reason.clone()))
        .collect();
    for (path, (owner, policy)) in &walk.files {
        let entry = &walk.entries[*owner];
        if entry.kind == EntryKind::Output {
            continue;
        }
        if !policy.share {
            let reason = private
                .get(path)
                .map(|reason| format!("{reason} (private unless explicitly overridden)"))
                .or_else(|| entry.note.clone())
                .unwrap_or_else(|| "share = false".to_string());
            report.unshared.push(Unshared {
                local: path.clone(),
                reason,
                branch_path: roots.branch_path(entry.kind, path, entry.variant.as_deref()),
            });
            continue;
        }
        if let Some(note) = &entry.note {
            report.unshared.push(Unshared {
                local: path.clone(),
                reason: note.clone(),
                branch_path: roots.branch_path(entry.kind, path, entry.variant.as_deref()),
            });
            continue;
        }
        let Some(branch_path) = roots.branch_path(entry.kind, path, entry.variant.as_deref())
        else {
            let reason = if path.starts_with(&roots.config_dir) {
                "not shared: repository metadata stays in git, and filenames must be portable (no backslashes, colons, control characters, or invalid UTF-8)"
            } else {
                "not shared: outside portable roots or an unsupported filename (no backslashes, colons, control characters, or invalid UTF-8)"
            };
            report.unshared.push(Unshared {
                local: path.clone(),
                reason: reason.to_string(),
                branch_path: None,
            });
            continue;
        };
        let tree_path = display_to_tree_path(&display_path(path));
        let Some((mode, oid)) = repo.object_at(&snapshot, &tree_path)? else {
            continue;
        };
        if entry.kind == EntryKind::Track
            && entry.path == *path
            && super::privacy::is_private_branch_path(&branch_path)
        {
            report.overrides.push(path.clone());
        }
        report.files.insert(
            branch_path,
            SharedFile {
                local: path.clone(),
                mode,
                oid,
            },
        );
    }
    Ok(report)
}
