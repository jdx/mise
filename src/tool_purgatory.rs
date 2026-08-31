use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::file::display_path;
use crate::toolset::{ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;

const STATE_SCHEMA_VERSION: u8 = 1;

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

pub(crate) fn scheduled_removals() -> Result<BTreeMap<PathBuf, u64>> {
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

pub(crate) fn schedule(tv: &ToolVersion, after: Duration) -> Result<()> {
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

pub(crate) fn forget_path(path: &Path) -> Result<()> {
    if !state_path().exists() {
        return Ok(());
    }
    let _lock = crate::lock_file::get(state_path(), false)?;
    let mut state = load_state()?;
    state.entries.remove(&entry_key(path));
    save_state(&state)
}

pub(crate) async fn auto_prune() -> Result<()> {
    if !state_path().exists() {
        return Ok(());
    }
    let now = now_epoch_seconds()?;
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
    let prunable = crate::cli::prune::prunable_tools(&config, Vec::<&BackendArg>::new()).await?;
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
        } else {
            warn!(
                "keeping unrecognized tool purgatory entry {}",
                display_path(install_path)
            );
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
    if !entries_to_remove.is_empty() {
        let _lock = crate::lock_file::get(state_path(), false)?;
        let mut state = load_state()?;
        for (key, entry) in entries_to_remove {
            if state.entries.get(&key) == Some(&entry) {
                state.entries.remove(&key);
            }
        }
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
}
