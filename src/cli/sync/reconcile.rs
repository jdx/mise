use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eyre::Result;

use crate::cli::args::BackendArg;
use crate::file;
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::install_state;

pub(super) struct LinkOwnership<'a> {
    direct_root: Option<&'a Path>,
    resolved_root: Option<&'a Path>,
}

impl<'a> LinkOwnership<'a> {
    pub(super) fn resolved(root: &'a Path) -> Self {
        Self {
            direct_root: None,
            resolved_root: Some(root),
        }
    }

    pub(super) fn direct(root: &'a Path) -> Self {
        Self {
            direct_root: Some(root),
            resolved_root: None,
        }
    }

    fn owns(&self, link: &Path) -> Result<bool> {
        if let Some(root) = self.direct_root
            && file::is_symlink_target_directly_within(link, root)?
        {
            return Ok(true);
        }
        match self.resolved_root {
            Some(root) => file::is_symlink_target_within(link, root),
            None => Ok(false),
        }
    }
}

/// Reconciles links from one external source under a tool's installs path.
///
/// `ownership` defines which existing links this operation owns. Managed
/// installs, runtime aliases, and links from other sources are preserved.
///
/// If multiple entries in `links` map to the same version, the first entry
/// wins, so callers must supply entries in a deterministic order.
///
/// Returns versions whose links were created or changed.
pub(super) fn reconcile(
    tool: &BackendArg,
    ownership: LinkOwnership<'_>,
    links: Vec<(String, PathBuf)>,
) -> Result<BTreeSet<String>> {
    let mut desired = BTreeMap::new();
    for (version, target) in links {
        // Preserve the first source entry for a version, matching the previous
        // create-if-missing loop when a source exposes duplicates.
        desired.entry(version).or_insert(target);
    }
    let installs_path = &tool.installs_path;
    let mut versions = desired.keys().cloned().collect::<BTreeSet<_>>();

    if installs_path.exists() {
        for entry in installs_path.read_dir()? {
            let path = entry?.path();
            if !is_runtime_symlink(&path)
                && ownership.owns(&path)?
                && let Some(version) = path.file_name().and_then(|v| v.to_str())
            {
                versions.insert(version.to_string());
            }
        }
    }

    file::create_dir_all(installs_path)?;
    let mut changed = BTreeSet::new();
    for version in versions {
        let _state_lock = install_state::lock_tool_version(&tool.short, &version)?;
        let link = installs_path.join(&version);
        let runtime_link = is_runtime_symlink(&link);
        let source_link = !runtime_link && ownership.owns(&link)?;
        let Some(target) = desired.get(&version) else {
            if source_link {
                file::remove_symlink_or_junction(&link)?;
            }
            continue;
        };

        if !runtime_link && target.exists() && file::is_symlink_to(&link, target) {
            install_state::clear_incomplete_marker(&tool.short, &version)?;
            continue;
        }

        let entry_exists = std::fs::symlink_metadata(&link).is_ok();
        if entry_exists && !source_link {
            // Never overwrite a managed install, runtime alias, or link from
            // another external source.
            continue;
        }

        if entry_exists {
            file::make_symlink(target, &link)?;
            if target.exists() {
                install_state::clear_incomplete_marker(&tool.short, &version)?;
            }
            changed.insert(version);
        } else {
            file::make_symlink(target, &link)?;
            if target.exists() {
                install_state::clear_incomplete_marker(&tool.short, &version)?;
            }
            changed.insert(version);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    #[cfg(unix)]
    fn removes_only_stale_links_from_the_current_source() {
        let dir = tempfile::tempdir().unwrap();
        let source_root = dir.path().join("source");
        let other_root = dir.path().join("other");
        let installs_path = dir.path().join("installs");
        file::create_dir_all(&source_root).unwrap();
        file::create_dir_all(&other_root).unwrap();
        file::create_dir_all(installs_path.join("3.0.0")).unwrap();

        file::make_symlink(&source_root.join("1.0.0"), &installs_path.join("1.0.0")).unwrap();
        file::make_symlink(&other_root.join("2.0.0"), &installs_path.join("2.0.0")).unwrap();
        file::make_symlink_or_file(Path::new("./3.0.0"), &installs_path.join("latest")).unwrap();

        let mut tool = BackendArg::from("node");
        tool.installs_path = installs_path.clone();

        reconcile(&tool, LinkOwnership::resolved(&source_root), vec![]).unwrap();

        assert!(std::fs::symlink_metadata(installs_path.join("1.0.0")).is_err());
        assert!(file::is_symlink_or_junction(&installs_path.join("2.0.0")));
        assert!(installs_path.join("3.0.0").is_dir());
        assert!(is_runtime_symlink(&installs_path.join("latest")));
    }

    #[test]
    #[cfg(unix)]
    fn removes_stale_links_after_a_direct_source_entry_disappears() {
        let dir = tempfile::tempdir().unwrap();
        let direct_root = dir.path().join("opt");
        let resolved_target = dir.path().join("Cellar/node@22/22.0.0");
        let direct_target = direct_root.join("node@22");
        let installs_path = dir.path().join("installs");
        file::create_dir_all(&resolved_target).unwrap();
        file::create_dir_all(&direct_root).unwrap();
        file::create_dir_all(&installs_path).unwrap();
        file::make_symlink(&resolved_target, &direct_target).unwrap();
        file::make_symlink(&direct_target, &installs_path.join("22")).unwrap();

        let mut tool = BackendArg::from("node");
        tool.installs_path = installs_path.clone();
        std::fs::remove_file(&direct_target).unwrap();

        reconcile(&tool, LinkOwnership::direct(&direct_root), vec![]).unwrap();

        assert!(std::fs::symlink_metadata(installs_path.join("22")).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn preserves_links_that_resolve_within_storage_but_bypass_the_direct_source() {
        let dir = tempfile::tempdir().unwrap();
        let direct_root = dir.path().join("opt");
        let storage_target = dir.path().join("Cellar/node@22/22.0.0");
        let installs_path = dir.path().join("installs");
        file::create_dir_all(&direct_root).unwrap();
        file::create_dir_all(&storage_target).unwrap();
        file::create_dir_all(&installs_path).unwrap();
        file::make_symlink(&storage_target, &installs_path.join("22")).unwrap();

        let mut tool = BackendArg::from("node");
        tool.installs_path = installs_path.clone();

        reconcile(&tool, LinkOwnership::direct(&direct_root), vec![]).unwrap();

        assert!(file::is_symlink_to(
            &installs_path.join("22"),
            &storage_target
        ));
    }

    #[test]
    fn waits_for_the_version_lock_before_reconciling() {
        let dir = tempfile::tempdir().unwrap();
        let source_root = dir.path().join("source");
        let target = source_root.join("1.0.0");
        let mut tool = BackendArg::from("node");
        tool.short = format!(
            "sync-lock-test-{}",
            dir.path().file_name().unwrap().to_string_lossy()
        );
        tool.installs_path = dir.path().join("installs");
        file::create_dir_all(&target).unwrap();

        let held_lock = install_state::lock_tool_version(&tool.short, "1.0.0").unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let result = reconcile(
                &tool,
                LinkOwnership::resolved(&source_root),
                vec![("1.0.0".to_string(), target)],
            );
            done_tx.send(result).unwrap();
        });

        ready_rx.recv().unwrap();
        assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(held_lock);
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        handle.join().unwrap();
    }
}
