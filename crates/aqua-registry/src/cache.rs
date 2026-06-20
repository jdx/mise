use crate::{AquaRegistryError, CompiledRegistry, ParsedRegistry, Result};
use blake3::Hasher as Blake3Hasher;
use siphasher::sip::SipHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const COMPILED_REGISTRY_CACHE_VERSION: &str = "v5";

#[derive(Debug, Clone)]
pub struct RegistryCache {
    root: PathBuf,
}

impl RegistryCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn source_path(&self, registry_url: &str) -> PathBuf {
        self.root
            .join("sources")
            .join(format!("{}.yaml", registry_url_hash(registry_url)))
    }

    pub fn read_source(&self, registry_url: &str) -> Result<Option<String>> {
        let path = self.source_path(registry_url);
        read_optional_to_string(&path)
    }

    pub fn read_fresh_source(
        &self,
        registry_url: &str,
        max_age: Duration,
    ) -> Result<Option<String>> {
        let path = self.source_path(registry_url);
        if !path_is_fresh(&path, max_age)? {
            return Ok(None);
        }
        read_optional_to_string(&path)
    }

    pub fn write_source(&self, registry_url: &str, source: &str) -> Result<()> {
        let path = self.source_path(registry_url);
        let Some(parent) = path.parent() else {
            return Err(AquaRegistryError::RegistryNotAvailable(format!(
                "cached aqua registry source path has no parent: {}",
                path.display()
            )));
        };
        fs::create_dir_all(parent)?;

        let mut tmp = tempfile::NamedTempFile::with_prefix_in("registry-source.", parent)?;
        tmp.write_all(source.as_bytes())?;
        tmp.persist(&path).map_err(|err| err.error)?;
        Ok(())
    }

    pub fn source_hash(source: &str) -> String {
        source_hash(source)
    }

    pub fn compiled_dir(&self, registry_url: &str, source_hash: &str) -> PathBuf {
        self.root
            .join("compiled")
            .join(registry_url_hash(registry_url))
            .join(COMPILED_REGISTRY_CACHE_VERSION)
            .join(source_hash)
    }

    pub fn load_compiled(&self, registry_url: &str, source_hash: &str) -> Result<CompiledRegistry> {
        CompiledRegistry::load(self.compiled_dir(registry_url, source_hash))
    }

    pub fn write_compiled(
        &self,
        registry_url: &str,
        source_hash: &str,
        registry: &ParsedRegistry,
    ) -> Result<CompiledRegistry> {
        let compiled_dir = self.compiled_dir(registry_url, source_hash);
        if let Ok(existing) = CompiledRegistry::load(&compiled_dir) {
            self.prune_stale_compiled(registry_url, source_hash);
            return Ok(existing);
        }

        let Some(parent) = compiled_dir.parent() else {
            return Err(AquaRegistryError::RegistryNotAvailable(format!(
                "compiled aqua registry cache path has no parent: {}",
                compiled_dir.display()
            )));
        };
        fs::create_dir_all(parent)?;

        let tmp_dir = tempfile::Builder::new()
            .prefix(&format!("{source_hash}.tmp-"))
            .tempdir_in(parent)?;
        let tmp_path = tmp_dir.path().to_path_buf();

        registry.write_compiled_cache(&tmp_path)?;
        let tmp_path = tmp_dir.keep();

        if let Ok(existing) = CompiledRegistry::load(&compiled_dir) {
            cleanup_tmp_dir_for_existing_compiled_cache(&tmp_path, &compiled_dir)?;
            self.prune_stale_compiled(registry_url, source_hash);
            return Ok(existing);
        }

        if compiled_dir.exists() {
            remove_dir_all_if_exists(&compiled_dir)?;
        }

        if let Err(err) = fs::rename(&tmp_path, &compiled_dir) {
            if let Ok(existing) = CompiledRegistry::load(&compiled_dir) {
                cleanup_tmp_dir_for_existing_compiled_cache(&tmp_path, &compiled_dir)?;
                self.prune_stale_compiled(registry_url, source_hash);
                return Ok(existing);
            }
            let _ = remove_dir_all_if_exists(&tmp_path);
            return Err(err.into());
        }

        let compiled = CompiledRegistry::load(&compiled_dir)?;
        self.prune_stale_compiled(registry_url, source_hash);
        Ok(compiled)
    }

    pub fn prune_stale_compiled(&self, registry_url: &str, source_hash: &str) {
        let current_dir = self.compiled_dir(registry_url, source_hash);
        prune_stale_compiled_registries(&current_dir);
    }
}

fn registry_url_hash(registry_url: &str) -> String {
    hash_to_str(&registry_url)
}

fn source_hash(source: &str) -> String {
    let mut hasher = Blake3Hasher::new();
    hasher.update(source.as_bytes());
    hasher.finalize().to_hex().to_string()
}

fn hash_to_str<T: Hash>(t: &T) -> String {
    let mut s = SipHasher::new();
    t.hash(&mut s);
    format!("{:x}", s.finish())
}

fn read_optional_to_string(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn path_is_fresh(path: &Path, max_age: Duration) -> Result<bool> {
    let Some(age) = path_age(path)? else {
        return Ok(false);
    };
    Ok(age < max_age)
}

fn path_age(path: &Path) -> Result<Option<Duration>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let modified = metadata.modified()?;
    Ok(Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default(),
    ))
}

fn prune_stale_compiled_registries(current_dir: &Path) {
    let Some(parent) = current_dir.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path == current_dir {
            continue;
        }
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            && is_compiled_source_hash_dir(&path)
            && let Err(err) = fs::remove_dir_all(&path)
        {
            log::debug!(
                "failed to prune stale compiled aqua registry cache {}: {err}",
                path.display()
            );
        }
    }
}

fn is_compiled_source_hash_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn cleanup_tmp_dir_for_existing_compiled_cache(tmp_dir: &Path, compiled_dir: &Path) -> Result<()> {
    match fs::remove_dir_all(tmp_dir) {
        Ok(()) => Ok(()),
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                && CompiledRegistry::load(compiled_dir).is_ok() =>
        {
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}

fn remove_dir_all_if_exists(path: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_source(package_id: &str) -> String {
        format!("packages:\n  - name: {package_id}\n    url: https://example.com/tool\n")
    }

    #[test]
    fn source_cache_reads_fresh_sources_and_skips_stale_sources() {
        let temp = tempfile::tempdir().unwrap();
        let cache = RegistryCache::new(temp.path());
        let registry_url = "https://example.com/aqua-registry";

        cache.write_source(registry_url, "packages: []").unwrap();

        assert_eq!(
            cache
                .read_fresh_source(registry_url, Duration::from_secs(60))
                .unwrap()
                .as_deref(),
            Some("packages: []")
        );
        assert!(
            cache
                .read_fresh_source(registry_url, Duration::ZERO)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn source_cache_writes_atomically_and_overwrites_existing_source() {
        let temp = tempfile::tempdir().unwrap();
        let cache = RegistryCache::new(temp.path());
        let registry_url = "https://example.com/aqua-registry";

        cache.write_source(registry_url, "first").unwrap();
        cache.write_source(registry_url, "second").unwrap();

        assert_eq!(
            cache.read_source(registry_url).unwrap().as_deref(),
            Some("second")
        );
        assert!(cache.source_path(registry_url).is_file());
    }

    #[test]
    fn compiled_cache_is_scoped_by_registry_url() {
        let cache = RegistryCache::new("/cache");
        let source_hash = RegistryCache::source_hash("packages: []");
        let first = cache.compiled_dir("https://example.com/one", &source_hash);
        let second = cache.compiled_dir("https://example.com/two", &source_hash);

        assert_ne!(first.parent(), second.parent());
        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some(source_hash.as_str())
        );
    }

    #[test]
    fn compiled_cache_writes_loads_and_prunes_stale_source_hash_siblings() {
        let temp = tempfile::tempdir().unwrap();
        let cache = RegistryCache::new(temp.path());
        let registry_url = "https://example.com/aqua-registry";
        let first_source = registry_source("example/first");
        let second_source = registry_source("example/second");
        let first_hash = RegistryCache::source_hash(&first_source);
        let second_hash = RegistryCache::source_hash(&second_source);
        let first_registry = ParsedRegistry::parse_yaml(&first_source).unwrap();
        let second_registry = ParsedRegistry::parse_yaml(&second_source).unwrap();

        cache
            .write_compiled(registry_url, &first_hash, &first_registry)
            .unwrap();
        let first_dir = cache.compiled_dir(registry_url, &first_hash);
        assert!(first_dir.is_dir());

        cache
            .write_compiled(registry_url, &second_hash, &second_registry)
            .unwrap();
        let second_dir = cache.compiled_dir(registry_url, &second_hash);
        let loaded = cache.load_compiled(registry_url, &second_hash).unwrap();

        assert!(second_dir.is_dir());
        assert!(!first_dir.exists());
        assert!(loaded.package("example/second").is_ok());
    }

    #[test]
    fn compiled_cache_prune_skips_temp_directories() {
        let temp = tempfile::tempdir().unwrap();
        let cache = RegistryCache::new(temp.path());
        let registry_url = "https://example.com/aqua-registry";
        let current_hash = RegistryCache::source_hash(&registry_source("example/current"));
        let stale_hash = RegistryCache::source_hash(&registry_source("example/stale"));
        let current_dir = cache.compiled_dir(registry_url, &current_hash);
        let stale_dir = cache.compiled_dir(registry_url, &stale_hash);
        let tmp_dir = current_dir
            .parent()
            .unwrap()
            .join(format!("{current_hash}.tmp-in-progress"));

        fs::create_dir_all(&current_dir).unwrap();
        fs::create_dir_all(&stale_dir).unwrap();
        fs::create_dir_all(&tmp_dir).unwrap();

        cache.prune_stale_compiled(registry_url, &current_hash);

        assert!(current_dir.is_dir());
        assert!(!stale_dir.exists());
        assert!(tmp_dir.is_dir());
    }

    #[test]
    fn compiled_temp_cleanup_treats_missing_temp_as_success_when_final_cache_exists() {
        let temp = tempfile::tempdir().unwrap();
        let cache = RegistryCache::new(temp.path());
        let registry_url = "https://example.com/aqua-registry";
        let source = registry_source("example/tool");
        let source_hash = RegistryCache::source_hash(&source);
        let registry = ParsedRegistry::parse_yaml(&source).unwrap();
        let compiled_dir = cache.compiled_dir(registry_url, &source_hash);
        let missing_tmp_dir = compiled_dir.with_file_name(format!("{source_hash}.tmp-missing"));

        registry.write_compiled_cache(&compiled_dir).unwrap();

        cleanup_tmp_dir_for_existing_compiled_cache(&missing_tmp_dir, &compiled_dir).unwrap();
    }

    #[test]
    fn registry_url_hash_matches_existing_cache_layout() {
        assert_eq!(registry_url_hash("foo"), "e1b19adfb2e348a2");
    }
}
