use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use blake3::Hasher;
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, AeadCore, OsRng},
};
use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::config::Settings;
use crate::dirs;
use crate::file;

fn get_encryption_key() -> Option<[u8; 32]> {
    std::env::var("__MISE_ENV_CACHE_KEY").ok().and_then(|s| {
        let bytes = BASE64_STANDARD.decode(&s).ok()?;
        bytes.try_into().ok()
    })
}

fn encrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| eyre::eyre!("failed to create cipher: {}", e))?;
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, data)
        .map_err(|e| eyre::eyre!("encryption failed: {}", e))?;

    // Format: nonce || ciphertext
    let mut result = nonce.to_vec();
    result.extend(ciphertext);
    Ok(result)
}

fn decrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    if data.len() < 12 {
        bail!("data too short to contain nonce");
    }

    let nonce = Nonce::from_slice(&data[..12]);
    let ciphertext = &data[12..];

    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| eyre::eyre!("failed to create cipher: {}", e))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| eyre::eyre!("decryption failed: {}", e))?;
    Ok(plaintext)
}

fn validate_watch_files(watch_files: &[PathBuf], expected_mtimes: &[u64]) -> Result<()> {
    if watch_files.len() != expected_mtimes.len() {
        bail!("watch file count mismatch");
    }
    for (path, expected_mtime) in watch_files.iter().zip(expected_mtimes.iter()) {
        if !path.exists() {
            // mtime=0 means file didn't exist when cached - skip if still doesn't exist
            if *expected_mtime == 0 {
                continue;
            }
            bail!("watch file no longer exists: {}", path.display());
        }
        if let Some(current_mtime) = get_file_mtime(path) {
            if current_mtime != *expected_mtime {
                bail!(
                    "watch file mtime changed: {} (expected: {}, current: {})",
                    path.display(),
                    expected_mtime,
                    current_mtime
                );
            }
        } else {
            bail!("could not get mtime for watch file: {}", path.display());
        }
    }
    Ok(())
}

/// Represents the cached environment data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedEnv {
    /// Cached environment variables
    pub env: BTreeMap<String, String>,
    /// User-configured paths from env._.path directives
    pub user_paths: Vec<PathBuf>,
    /// Tool paths from installations
    pub tool_paths: Vec<PathBuf>,
    /// Time when the cache was created
    pub created_at: u64,
    /// Files to watch for changes (from modules and _.source directives)
    pub watch_files: Vec<PathBuf>,
    /// mtimes of watch files at cache creation time
    pub watch_file_mtimes: Vec<u64>,
    /// mise version when cache was created
    pub mise_version: String,
    /// SHA256 of the original cache key inputs (for debugging)
    pub cache_key_debug: String,
}

/// Cached environment data for non-tool env results (config-time env resolution)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedNonToolEnv {
    /// Environment variables resolved from non-tool directives
    pub env: IndexMap<String, (String, PathBuf)>,
    /// Variables removed by env directives
    pub env_remove: BTreeSet<String>,
    /// dotenv files used
    pub env_files: Vec<PathBuf>,
    /// paths added by env directives
    pub env_paths: Vec<PathBuf>,
    /// scripts sourced by env directives
    pub env_scripts: Vec<PathBuf>,
    /// redaction patterns contributed by env directives
    pub redactions: Vec<String>,
    /// Files to watch for changes (from modules and _.source directives)
    pub watch_files: Vec<PathBuf>,
    /// mtimes of watch files at cache creation time
    pub watch_file_mtimes: Vec<u64>,
    /// Time when the cache was created
    pub created_at: u64,
    /// mise version when cache was created
    pub mise_version: String,
    /// SHA256 of the original cache key inputs (for debugging)
    pub cache_key_debug: String,
}

impl CachedEnv {
    /// Returns the directory where env cache files are stored
    pub fn cache_dir() -> PathBuf {
        dirs::STATE.join("env-cache")
    }

    /// Computes the cache key based on config files, settings, tool versions, etc.
    pub fn compute_cache_key(
        config_files: &[(PathBuf, u64)],    // (path, mtime)
        tool_versions: &[(String, String)], // (tool, version)
        settings_hash: &str,
        base_path: &str,
    ) -> String {
        let mut hasher = Hasher::new();

        // mise version
        hasher.update(env!("CARGO_PKG_VERSION").as_bytes());

        // config files and their mtimes
        for (path, mtime) in config_files {
            hasher.update(path.to_string_lossy().as_bytes());
            hasher.update(&mtime.to_le_bytes());
        }

        // tool versions
        for (tool, version) in tool_versions {
            hasher.update(tool.as_bytes());
            hasher.update(version.as_bytes());
        }

        // settings hash
        hasher.update(settings_hash.as_bytes());

        // base PATH
        hasher.update(base_path.as_bytes());

        let hash = hasher.finalize();
        hex::encode(hash.as_bytes())
    }

    /// Generates a new encryption key and returns it as a base64 string
    pub fn generate_encryption_key() -> String {
        let key = ChaCha20Poly1305::generate_key(&mut OsRng);
        BASE64_STANDARD.encode(key)
    }

    /// Ensures an encryption key exists, returns one if not set
    pub fn ensure_encryption_key() -> String {
        std::env::var("__MISE_ENV_CACHE_KEY").unwrap_or_else(|_| Self::generate_encryption_key())
    }

    /// Loads a cached environment from disk
    pub fn load(cache_key: &str) -> Result<Option<Self>> {
        let key = match get_encryption_key() {
            Some(k) => k,
            None => {
                trace!("env_cache: no encryption key set, skipping cache load");
                return Ok(None);
            }
        };

        let cache_file = Self::cache_dir().join(cache_key);
        if !cache_file.exists() {
            trace!(
                "env_cache: cache file does not exist: {}",
                cache_file.display()
            );
            return Ok(None);
        }

        let encrypted_data = file::read(&cache_file)?;
        let decrypted_data = match decrypt_data(&encrypted_data, &key) {
            Ok(data) => data,
            Err(e) => {
                debug!("env_cache: decryption failed (key changed?): {}", e);
                // Remove stale cache file
                let _ = file::remove_file(&cache_file);
                return Ok(None);
            }
        };

        let cached: CachedEnv = match rmp_serde::from_slice(&decrypted_data) {
            Ok(c) => c,
            Err(e) => {
                debug!("env_cache: deserialization failed: {}", e);
                let _ = file::remove_file(&cache_file);
                return Ok(None);
            }
        };

        // Validate mise version
        if cached.mise_version != env!("CARGO_PKG_VERSION") {
            debug!(
                "env_cache: mise version mismatch (cached: {}, current: {})",
                cached.mise_version,
                env!("CARGO_PKG_VERSION")
            );
            let _ = file::remove_file(&cache_file);
            return Ok(None);
        }

        // Check TTL
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let ttl = Settings::get().env_cache_ttl().as_secs();
        let age = now.saturating_sub(cached.created_at);
        if age > ttl {
            debug!("env_cache: cache expired (age: {}s, ttl: {}s)", age, ttl);
            let _ = file::remove_file(&cache_file);
            return Ok(None);
        }

        // Check watch files mtimes
        if let Err(e) = validate_watch_files(&cached.watch_files, &cached.watch_file_mtimes) {
            debug!("env_cache: watch file validation failed: {}", e);
            let _ = file::remove_file(&cache_file);
            return Ok(None);
        }

        trace!("env_cache: loaded cache for key {}", cache_key);
        Ok(Some(cached))
    }

    /// Saves a cached environment to disk
    pub fn save(&self, cache_key: &str) -> Result<()> {
        let key = match get_encryption_key() {
            Some(k) => k,
            None => {
                trace!("env_cache: no encryption key set, skipping cache save");
                return Ok(());
            }
        };

        let cache_dir = Self::cache_dir();
        file::create_dir_all(&cache_dir)?;

        let serialized = rmp_serde::to_vec(self)?;
        let encrypted = encrypt_data(&serialized, &key)?;

        let cache_file = cache_dir.join(cache_key);
        file::write(&cache_file, &encrypted)?;

        trace!("env_cache: saved cache for key {}", cache_key);
        Ok(())
    }

    /// Returns true if env caching is enabled and we have an encryption key
    pub fn is_enabled() -> bool {
        Settings::get().env_cache && get_encryption_key().is_some()
    }

    /// Clears all env cache files
    pub fn clear() -> Result<()> {
        let cache_dir = Self::cache_dir();
        if cache_dir.exists() {
            file::remove_all(&cache_dir)?;
        }
        Ok(())
    }
}

impl CachedNonToolEnv {
    /// Returns the directory where env cache files are stored
    pub fn cache_dir() -> PathBuf {
        CachedEnv::cache_dir()
    }

    /// Computes the cache key based on config files and settings
    pub fn compute_cache_key(
        config_files: &[(PathBuf, u64)], // (path, mtime)
        settings_hash: &str,
        base_path: &str,
    ) -> String {
        let mut hasher = Hasher::new();

        // scope to non-tool env cache
        hasher.update(b"non-tool-env");

        // mise version
        hasher.update(env!("CARGO_PKG_VERSION").as_bytes());

        // config files and their mtimes
        for (path, mtime) in config_files {
            hasher.update(path.to_string_lossy().as_bytes());
            hasher.update(&mtime.to_le_bytes());
        }

        // settings hash
        hasher.update(settings_hash.as_bytes());

        // base PATH
        hasher.update(base_path.as_bytes());

        let hash = hasher.finalize();
        hex::encode(hash.as_bytes())
    }

    /// Loads cached non-tool env data if it exists and is valid
    pub fn load(cache_key: &str) -> Result<Option<Self>> {
        let key = match get_encryption_key() {
            Some(k) => k,
            None => {
                trace!("env_cache: no encryption key set, skipping cache load");
                return Ok(None);
            }
        };

        let cache_file = Self::cache_dir().join(cache_key);
        if !cache_file.exists() {
            trace!(
                "env_cache: cache file does not exist: {}",
                cache_file.display()
            );
            return Ok(None);
        }

        let encrypted_data = file::read(&cache_file)?;
        let decrypted_data = match decrypt_data(&encrypted_data, &key) {
            Ok(data) => data,
            Err(e) => {
                debug!("env_cache: decryption failed: {}", e);
                let _ = file::remove_file(&cache_file);
                return Ok(None);
            }
        };

        let cached: CachedNonToolEnv = match rmp_serde::from_slice(&decrypted_data) {
            Ok(c) => c,
            Err(e) => {
                debug!("env_cache: deserialization failed: {}", e);
                let _ = file::remove_file(&cache_file);
                return Ok(None);
            }
        };

        // Validate mise version
        if cached.mise_version != env!("CARGO_PKG_VERSION") {
            debug!(
                "env_cache: mise version mismatch (cached: {}, current: {})",
                cached.mise_version,
                env!("CARGO_PKG_VERSION")
            );
            let _ = file::remove_file(&cache_file);
            return Ok(None);
        }

        // Check TTL
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let ttl = Settings::get().env_cache_ttl().as_secs();
        let age = now.saturating_sub(cached.created_at);
        if age > ttl {
            debug!("env_cache: cache expired (age: {}s, ttl: {}s)", age, ttl);
            let _ = file::remove_file(&cache_file);
            return Ok(None);
        }

        // Check watch files mtimes
        if let Err(e) = validate_watch_files(&cached.watch_files, &cached.watch_file_mtimes) {
            debug!("env_cache: watch file validation failed: {}", e);
            let _ = file::remove_file(&cache_file);
            return Ok(None);
        }

        trace!("env_cache: loaded non-tool env cache for key {}", cache_key);
        Ok(Some(cached))
    }

    /// Saves cached non-tool env data to disk
    pub fn save(&self, cache_key: &str) -> Result<()> {
        let key = match get_encryption_key() {
            Some(k) => k,
            None => {
                trace!("env_cache: no encryption key set, skipping cache save");
                return Ok(());
            }
        };

        let cache_dir = Self::cache_dir();
        file::create_dir_all(&cache_dir)?;

        let serialized = rmp_serde::to_vec(self)?;
        let encrypted = encrypt_data(&serialized, &key)?;

        let cache_file = cache_dir.join(cache_key);
        file::write(&cache_file, &encrypted)?;

        trace!("env_cache: saved non-tool env cache for key {}", cache_key);
        Ok(())
    }

    /// Returns true if env caching is enabled and we have an encryption key
    pub fn is_enabled() -> bool {
        Settings::get().env_cache && get_encryption_key().is_some()
    }
}

/// Helper to get the mtime of a file as seconds since UNIX epoch
pub fn get_file_mtime(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// Computes a hash of the current settings that affect env computation
pub fn compute_settings_hash() -> String {
    let settings = Settings::get();
    let mut hasher = Hasher::new();

    // Add settings that affect env computation
    hasher.update(settings.experimental.to_string().as_bytes());
    hasher.update(settings.all_compile.to_string().as_bytes());

    // Add any other relevant settings
    if let Some(env_file) = &settings.env_file {
        hasher.update(env_file.to_string_lossy().as_bytes());
    }

    let hash = hasher.finalize();
    hex::encode(&hash.as_bytes()[..8]) // Short hash for debugging
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_computation() {
        let config_files = vec![(PathBuf::from("/home/user/project/mise.toml"), 1234567890u64)];
        let tool_versions = vec![("node".to_string(), "20.0.0".to_string())];
        let settings_hash = "abc123";
        let base_path = "/usr/bin:/bin";

        let key1 =
            CachedEnv::compute_cache_key(&config_files, &tool_versions, settings_hash, base_path);

        // Same inputs should produce same key
        let key2 =
            CachedEnv::compute_cache_key(&config_files, &tool_versions, settings_hash, base_path);
        assert_eq!(key1, key2);

        // Different mtime should produce different key
        let config_files_changed =
            vec![(PathBuf::from("/home/user/project/mise.toml"), 1234567891u64)];
        let key3 = CachedEnv::compute_cache_key(
            &config_files_changed,
            &tool_versions,
            settings_hash,
            base_path,
        );
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_encryption_roundtrip() {
        let key: [u8; 32] = rand::random();
        let data = b"hello world";

        let encrypted = encrypt_data(data, &key).unwrap();
        let decrypted = decrypt_data(&encrypted, &key).unwrap();

        assert_eq!(data.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_generate_encryption_key() {
        let key1 = CachedEnv::generate_encryption_key();
        let key2 = CachedEnv::generate_encryption_key();

        // Keys should be different
        assert_ne!(key1, key2);

        // Keys should be valid base64
        assert!(BASE64_STANDARD.decode(&key1).is_ok());
        assert!(BASE64_STANDARD.decode(&key2).is_ok());

        // Decoded keys should be 32 bytes
        assert_eq!(BASE64_STANDARD.decode(&key1).unwrap().len(), 32);
    }
}
