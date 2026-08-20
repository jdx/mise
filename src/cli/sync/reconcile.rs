use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eyre::Result;

use crate::cli::args::BackendArg;
use crate::file;
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::install_state;

pub(super) struct LinkOwnership {
    root: PathBuf,
}

impl LinkOwnership {
    /// Own links whose immediate target is under the provider namespace.
    ///
    /// This intentionally includes dangling terminal entries and does not
    /// resolve a provider entry that itself redirects elsewhere.
    pub(super) fn in_namespace(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn owns(&self, link: &Path) -> Result<bool> {
        file::is_symlink_target_within(link, &self.root)
    }
}

pub(super) struct ProviderLinks {
    ownership: LinkOwnership,
    links: Vec<(String, PathBuf)>,
}

impl ProviderLinks {
    pub(super) fn new(ownership: LinkOwnership, links: Vec<(String, PathBuf)>) -> Self {
        Self { ownership, links }
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
    ownership: LinkOwnership,
    links: Vec<(String, PathBuf)>,
) -> Result<BTreeSet<String>> {
    Ok(
        reconcile_all(tool, vec![ProviderLinks::new(ownership, links)])?
            .pop()
            .unwrap_or_default(),
    )
}

/// Reconciles multiple selected providers as one operation.
///
/// Earlier providers take precedence when multiple sources expose the same
/// version. Ownership from every selected provider is considered before any
/// link is removed or replaced, so a stale link from a later provider cannot
/// block an earlier provider's desired version.
pub(super) fn reconcile_all(
    tool: &BackendArg,
    providers: Vec<ProviderLinks>,
) -> Result<Vec<BTreeSet<String>>> {
    let mut desired = BTreeMap::new();
    for (provider_index, provider) in providers.iter().enumerate() {
        for (version, target) in &provider.links {
            // Preserve the first source entry for a version, matching the
            // previous create-if-missing provider precedence.
            desired
                .entry(version.clone())
                .or_insert_with(|| (provider_index, target.clone()));
        }
    }
    let installs_path = &tool.installs_path;
    let mut versions = desired.keys().cloned().collect::<BTreeSet<_>>();

    if installs_path.exists() {
        for entry in installs_path.read_dir()? {
            let path = entry?.path();
            if !is_runtime_symlink(&path)
                && providers_own(&providers, &path)?
                && let Some(version) = path.file_name().and_then(|v| v.to_str())
            {
                versions.insert(version.to_string());
            }
        }
    }

    file::create_dir_all(installs_path)?;
    let mut changed = vec![BTreeSet::new(); providers.len()];
    for version in versions {
        let _state_lock = install_state::lock_tool_version(&tool.short, &version)?;
        let link = installs_path.join(&version);
        let runtime_link = is_runtime_symlink(&link);
        let source_link = !runtime_link && providers_own(&providers, &link)?;
        let Some((provider_index, target)) = desired.get(&version) else {
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

        file::make_symlink(target, &link)?;
        if target.exists() {
            install_state::clear_incomplete_marker(&tool.short, &version)?;
        }
        changed[*provider_index].insert(version);
    }
    Ok(changed)
}

fn providers_own(providers: &[ProviderLinks], link: &Path) -> Result<bool> {
    for provider in providers {
        if provider.ownership.owns(link)? {
            return Ok(true);
        }
    }
    Ok(false)
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

        reconcile(&tool, LinkOwnership::in_namespace(&source_root), vec![]).unwrap();

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

        reconcile(&tool, LinkOwnership::in_namespace(&direct_root), vec![]).unwrap();

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

        reconcile(&tool, LinkOwnership::in_namespace(&direct_root), vec![]).unwrap();

        assert!(file::is_symlink_to(
            &installs_path.join("22"),
            &storage_target
        ));
    }

    #[test]
    #[cfg(unix)]
    fn later_provider_stale_link_does_not_block_earlier_provider() {
        let dir = tempfile::tempdir().unwrap();
        let earlier_root = dir.path().join("earlier");
        let later_root = dir.path().join("later");
        let installs_path = dir.path().join("installs");
        let desired_target = earlier_root.join("1.0.0");
        let stale_target = later_root.join("1.0.0");
        file::create_dir_all(&desired_target).unwrap();
        file::create_dir_all(&later_root).unwrap();
        file::create_dir_all(&installs_path).unwrap();
        file::make_symlink(&stale_target, &installs_path.join("1.0.0")).unwrap();

        let mut tool = BackendArg::from("node");
        tool.installs_path = installs_path.clone();
        let providers = vec![
            ProviderLinks::new(
                LinkOwnership::in_namespace(&earlier_root),
                vec![("1.0.0".to_string(), desired_target.clone())],
            ),
            ProviderLinks::new(LinkOwnership::in_namespace(&later_root), vec![]),
        ];

        let changed = reconcile_all(&tool, providers).unwrap();

        assert_eq!(changed[0], BTreeSet::from(["1.0.0".to_string()]));
        assert!(changed[1].is_empty());
        assert!(file::is_symlink_to(
            &installs_path.join("1.0.0"),
            &desired_target
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
                LinkOwnership::in_namespace(&source_root),
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
