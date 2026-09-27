use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use crate::args::BackendArg;
use crate::config::Config;
use crate::file::display_path;
use crate::toolset::{ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;

const STATE_SCHEMA_VERSION: u8 = 1;

/// How long a due receipt waits before it is evaluated again after pruning
/// kept it. Deciding whether a version is still in use loads every tracked
/// config and scans installed versions, which is far too slow to repeat on
/// every command while a tracked config keeps referencing the version.
const RECHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Deserialize, Serialize)]
struct PurgatoryState {
    schema_version: u8,
    entries: BTreeMap<String, PurgatoryEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PurgatoryEntry {
    install_path: PathBuf,
    display: String,
    remove_after: u64,
}

impl PurgatoryState {
    fn empty() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            entries: BTreeMap::new(),
        }
    }

    fn has_due(&self, now: u64) -> bool {
        self.entries.values().any(|entry| entry.remove_after <= now)
    }

    /// Applies the outcome of a pruning pass. Entries that changed since the
    /// pass read them (for example, a newer upgrade rescheduled the same path)
    /// are left alone so the newer receipt wins.
    fn apply_prune_outcome(
        &mut self,
        removed: Vec<(String, PurgatoryEntry)>,
        kept: Vec<(String, PurgatoryEntry)>,
        recheck_after: u64,
    ) {
        for (key, entry) in removed {
            if self.entries.get(&key) == Some(&entry) {
                self.entries.remove(&key);
            }
        }
        for (key, entry) in kept {
            if let Some(current) = self.entries.get_mut(&key)
                && *current == entry
            {
                current.remove_after = recheck_after;
            }
        }
    }
}

fn state_path() -> &'static Path {
    &crate::dirs::TOOL_PURGATORY
}

fn now_epoch_seconds() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .wrap_err("system clock is before the Unix epoch")?
        .as_secs())
}

fn entry_key(path: &Path) -> String {
    crate::hash::hash_sha256_to_str(&path.to_string_lossy())
}

fn load_state() -> Result<PurgatoryState> {
    let path = state_path();
    if !path.exists() {
        return Ok(PurgatoryState::empty());
    }
    let state: PurgatoryState = serde_json::from_str(&crate::file::read_to_string(path)?)
        .wrap_err_with(|| format!("failed to read tool purgatory state {}", display_path(path)))?;
    if state.schema_version != STATE_SCHEMA_VERSION {
        bail!(
            "unsupported tool purgatory state version {} in {}",
            state.schema_version,
            display_path(path)
        );
    }
    Ok(state)
}

fn save_state(state: &PurgatoryState) -> Result<()> {
    let path = state_path();
    if state.entries.is_empty() {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        return Ok(());
    }
    crate::file::create_dir_all(path.parent().expect("tool purgatory state parent"))?;
    let mut contents = serde_json::to_vec_pretty(state)?;
    contents.push(b'\n');
    crate::file::write_atomic(path, contents)
}

pub fn scheduled_removals() -> Result<BTreeMap<PathBuf, u64>> {
    if !state_path().exists() {
        return Ok(BTreeMap::new());
    }
    let _lock = crate::lock_file::get(state_path(), false)?;
    Ok(load_state()?
        .entries
        .into_values()
        .map(|entry| (entry.install_path, entry.remove_after))
        .collect())
}

pub fn schedule(tv: &ToolVersion, after: Duration) -> Result<()> {
    let _lock = crate::lock_file::get(state_path(), false)?;
    let mut state = load_state()?;
    let install_path = tv.install_path();
    state.entries.insert(
        entry_key(&install_path),
        PurgatoryEntry {
            install_path,
            display: tv.to_string(),
            remove_after: now_epoch_seconds()?.saturating_add(after.as_secs()),
        },
    );
    save_state(&state)
}

pub fn forget_path(path: &Path) -> Result<()> {
    if !state_path().exists() {
        return Ok(());
    }
    let _lock = crate::lock_file::get(state_path(), false)?;
    let mut state = load_state()?;
    state.entries.remove(&entry_key(path));
    save_state(&state)
}

pub async fn auto_prune() -> Result<()> {
    if !state_path().exists() {
        return Ok(());
    }
    // This runs on nearly every command, so check for due receipts without
    // taking either lock. Saves are atomic renames, so an unlocked read sees
    // a complete state. A failed read falls through to the locked path.
    let now = now_epoch_seconds()?;
    if load_state().is_ok_and(|state| !state.has_due(now)) {
        return Ok(());
    }
    // Config resolution below may execute trusted templates. If one of those
    // templates invokes mise, the child reaches auto-prune before the parent
    // has removed the due receipt. Serialize the whole cleanup separately
    // from the short state-file lock so nested and concurrent invocations can
    // skip it without blocking commands that legitimately update the state.
    let cleanup_lock_path = state_path().with_extension("auto-prune");
    let Some(_cleanup_lock) = crate::lock_file::LockFile::new(&cleanup_lock_path).try_lock()?
    else {
        debug!("skipping deferred pruning because another invocation is handling it");
        return Ok(());
    };
    let due = {
        let _lock = crate::lock_file::get(state_path(), false)?;
        let state = load_state()?;
        state
            .entries
            .iter()
            .filter(|(_, entry)| entry.remove_after <= now)
            .map(|(key, entry)| (key.clone(), entry.clone()))
            .collect::<Vec<_>>()
    };
    if due.is_empty() {
        return Ok(());
    }

    let config = Config::get().await?;
    let prunable = crate::toolset::prunable_tools(&config, Vec::<&BackendArg>::new()).await?;
    let prunable_by_path = prunable
        .into_iter()
        .map(|(backend, tv)| (tv.install_path(), (backend, tv)))
        .collect::<BTreeMap<_, _>>();
    let installed_paths = ToolsetBuilder::new()
        .build(&config)
        .await?
        .list_installed_versions(&config)
        .await?
        .into_iter()
        .map(|(_, tv)| tv.install_path())
        .collect::<HashSet<_>>();

    let mpr = MultiProgressReport::get();
    let mut install_state_changed = false;
    let mut entries_awaiting_reconciliation = vec![];
    let mut entries_to_remove = vec![];
    let mut entries_to_recheck = vec![];
    for (key, entry) in due {
        let install_path = &entry.install_path;
        let display = &entry.display;
        let Some(installs_dir) = install_path
            .parent()
            .filter(|parent| parent.starts_with(*crate::dirs::INSTALLS))
            .filter(|parent| *parent != *crate::dirs::INSTALLS)
        else {
            warn!(
                "ignoring tool purgatory entry outside the user installs directory: {}",
                display_path(install_path)
            );
            entries_to_remove.push((key, entry));
            continue;
        };
        if let Some((backend, tv)) = prunable_by_path.get(install_path) {
            let pr = mpr.add(&format!("uninstall {display}"));
            match backend
                .uninstall_version(&config, tv, pr.as_ref(), false)
                .await
            {
                Ok(()) => {
                    pr.finish();
                    install_state_changed = true;
                    match crate::runtime_symlinks::remove_missing_symlinks_in_dir(installs_dir) {
                        Ok(()) => entries_awaiting_reconciliation.push((key, entry)),
                        Err(err) => {
                            warn!(
                                "failed to remove missing runtime symlinks for {display}: {err:#}"
                            );
                        }
                    }
                }
                Err(err) => warn!("failed to prune deferred {display}: {err:#}"),
            }
        } else if !install_path.exists() {
            // A missing version is already gone, but backend discovery may no
            // longer find its install directory. Retry that cleanup directly
            // from the receipt before allowing reconciliation to clear it.
            install_state_changed = true;
            match crate::runtime_symlinks::remove_missing_symlinks_in_dir(installs_dir) {
                Ok(()) => entries_awaiting_reconciliation.push((key, entry)),
                Err(err) => {
                    warn!("failed to remove missing runtime symlinks for {display}: {err:#}");
                }
            }
        } else if installed_paths.contains(install_path) {
            // Keep the receipt while a tracked config or tool stub needs this
            // version. It may become prunable again after that reference goes
            // away, without another upgrade to create a fresh receipt.
            debug!("keeping deferred {display} because it is still in use");
            entries_to_recheck.push((key, entry));
        } else {
            warn!(
                "keeping unrecognized tool purgatory entry {}",
                display_path(install_path)
            );
            entries_to_recheck.push((key, entry));
        }
    }
    mpr.finish_progress();
    if install_state_changed {
        let reconcile = async {
            let config = Config::reset().await?;
            let ts = config.get_toolset().await?;
            crate::config::rebuild_shims_and_runtime_symlinks(
                &config,
                ts,
                &[],
                crate::lockfile::LockfileUpdateMode::Normal,
            )
            .await
        }
        .await;
        match reconcile {
            Ok(()) => {
                entries_to_remove.extend(entries_awaiting_reconciliation);
            }
            Err(err) => {
                // Keep the due receipts so a later invocation retries
                // reconciliation. The versions are already missing, so the
                // retry will not attempt to uninstall them again.
                warn!(
                    "failed to reconcile runtime symlinks and shims after deferred pruning: {err:#}"
                );
            }
        }
    }
    if !entries_to_remove.is_empty() || !entries_to_recheck.is_empty() {
        let recheck_after = now_epoch_seconds()?.saturating_add(RECHECK_INTERVAL.as_secs());
        let _lock = crate::lock_file::get(state_path(), false)?;
        let mut state = load_state()?;
        state.apply_prune_outcome(entries_to_remove, entries_to_recheck, recheck_after);
        save_state(&state)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_keys_are_stable_and_path_specific() {
        assert_eq!(entry_key(Path::new("/a")), entry_key(Path::new("/a")));
        assert_ne!(entry_key(Path::new("/a")), entry_key(Path::new("/b")));
    }

    fn entry(path: &str, remove_after: u64) -> (String, PurgatoryEntry) {
        (
            entry_key(Path::new(path)),
            PurgatoryEntry {
                install_path: PathBuf::from(path),
                display: path.to_string(),
                remove_after,
            },
        )
    }

    #[test]
    fn kept_receipts_back_off_until_the_recheck_interval() {
        let (removed_key, removed) = entry("/installs/a/1", 100);
        let (kept_key, kept) = entry("/installs/b/1", 100);
        let (future_key, future) = entry("/installs/c/1", u64::MAX);
        let mut state = PurgatoryState::empty();
        for (key, entry) in [
            (removed_key.clone(), removed.clone()),
            (kept_key.clone(), kept.clone()),
            (future_key.clone(), future.clone()),
        ] {
            state.entries.insert(key, entry);
        }
        assert!(state.has_due(100));

        let recheck_after = 100 + RECHECK_INTERVAL.as_secs();
        state.apply_prune_outcome(
            vec![(removed_key.clone(), removed)],
            vec![(kept_key.clone(), kept)],
            recheck_after,
        );

        assert!(!state.entries.contains_key(&removed_key));
        // The kept receipt stays, so the version becomes prunable again once
        // nothing references it, but it is not due again until the interval
        // passes. Receipts that were not evaluated are untouched.
        assert_eq!(state.entries[&kept_key].remove_after, recheck_after);
        assert_eq!(state.entries[&future_key], future);
        assert!(!state.has_due(recheck_after - 1));
        assert!(state.has_due(recheck_after));
    }

    #[test]
    fn prune_outcome_skips_receipts_rescheduled_during_the_pass() {
        let (key, stale) = entry("/installs/a/1", 100);
        let (_, rescheduled) = entry("/installs/a/1", 200);
        let mut state = PurgatoryState::empty();
        state.entries.insert(key.clone(), rescheduled.clone());
        state.apply_prune_outcome(vec![], vec![(key.clone(), stale.clone())], 9_999);
        assert_eq!(state.entries[&key], rescheduled);
        state.apply_prune_outcome(vec![(key.clone(), stale)], vec![], 9_999);
        assert_eq!(state.entries[&key], rescheduled);
    }
}
