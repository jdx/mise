use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::Result;

use crate::file;
use crate::hash::file_hash_blake3;

/// Persistent state for prepare freshness checking.
///
/// Stores blake3 content hashes of source files keyed by provider ID.
/// Persisted to `.mise/prepare-state.toml`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PrepareState {
    /// provider_id → (relative_path → blake3_hex)
    #[serde(default)]
    pub providers: BTreeMap<String, BTreeMap<String, String>>,
}

impl PrepareState {
    /// Load state from `.mise/prepare-state.toml`, returning default if not found.
    pub fn load(project_root: &Path) -> Self {
        let path = state_path(project_root);
        if !path.exists() {
            return Self::default();
        }
        match file::read_to_string(&path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(state) => state,
                Err(e) => {
                    warn!("failed to parse {}: {e}", path.display());
                    Self::default()
                }
            },
            Err(e) => {
                warn!("failed to read {}: {e}", path.display());
                Self::default()
            }
        }
    }

    /// Save state to `.mise/prepare-state.toml`.
    pub fn save(&self, project_root: &Path) -> Result<()> {
        let path = state_path(project_root);
        file::create_dir_all(path.parent().unwrap())?;
        let contents = toml::to_string_pretty(self)?;
        file::write(&path, contents)?;
        Ok(())
    }

    /// Get stored hashes for a provider, or None if not previously recorded.
    pub fn get_hashes(&self, provider_id: &str) -> Option<&BTreeMap<String, String>> {
        self.providers.get(provider_id)
    }

    /// Update stored hashes for a provider.
    pub fn set_hashes(&mut self, provider_id: &str, hashes: BTreeMap<String, String>) {
        self.providers.insert(provider_id.to_string(), hashes);
    }
}

/// Compute blake3 hashes for a list of source files.
///
/// Returns a map of relative_path → blake3_hex. Directories are skipped
/// (only regular files are hashed). Non-existent files are omitted.
pub fn hash_sources(sources: &[PathBuf], project_root: &Path) -> Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();

    for source in sources {
        if !source.exists() {
            continue;
        }

        if source.is_dir() {
            // For directories, hash all files within (up to 3 levels deep)
            hash_dir_files(&mut hashes, source, project_root, 3)?;
        } else {
            let hash = file_hash_blake3(source, None)?;
            let rel = source
                .strip_prefix(project_root)
                .unwrap_or(source)
                .to_string_lossy()
                .to_string();
            hashes.insert(rel, hash);
        }
    }

    Ok(hashes)
}

/// Recursively hash files in a directory up to max_depth levels.
fn hash_dir_files(
    hashes: &mut BTreeMap<String, String>,
    dir: &Path,
    project_root: &Path,
    max_depth: usize,
) -> Result<()> {
    if max_depth == 0 {
        return Ok(());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                hash_dir_files(hashes, &path, project_root, max_depth - 1)?;
            } else {
                let hash = file_hash_blake3(&path, None)?;
                let rel = path
                    .strip_prefix(project_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                hashes.insert(rel, hash);
            }
        }
    }
    Ok(())
}

/// Path to the state file for a given project root.
fn state_path(project_root: &Path) -> PathBuf {
    project_root.join(".mise").join("prepare-state.toml")
}
