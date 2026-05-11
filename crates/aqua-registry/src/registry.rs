use crate::types::AquaPackage;
use crate::{AquaRegistryConfig, AquaRegistryError, CacheStore, RegistryFetcher, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;
use tokio::sync::Mutex;

/// The main Aqua registry implementation
#[derive(Debug)]
pub struct AquaRegistry<F = DefaultRegistryFetcher, C = NoOpCacheStore>
where
    F: RegistryFetcher,
    C: CacheStore,
{
    #[allow(dead_code)]
    config: AquaRegistryConfig,
    fetcher: F,
    #[allow(dead_code)]
    cache_store: C,
}

/// Default implementation of RegistryFetcher
#[derive(Debug, Clone)]
pub struct DefaultRegistryFetcher;

/// No-op implementation of CacheStore
#[derive(Debug, Clone, Default)]
pub struct NoOpCacheStore;

/// File-based cache store implementation
#[derive(Debug, Clone)]
pub struct FileCacheStore {
    cache_dir: PathBuf,
}

impl AquaRegistry {
    /// Create a new AquaRegistry with the given configuration
    pub fn new(config: AquaRegistryConfig) -> Self {
        let fetcher = DefaultRegistryFetcher;
        Self {
            config,
            fetcher,
            cache_store: NoOpCacheStore,
        }
    }

    /// Create a new AquaRegistry with custom fetcher and cache store
    pub fn with_fetcher_and_cache<F, C>(
        config: AquaRegistryConfig,
        fetcher: F,
        cache_store: C,
    ) -> AquaRegistry<F, C>
    where
        F: RegistryFetcher,
        C: CacheStore,
    {
        AquaRegistry {
            config,
            fetcher,
            cache_store,
        }
    }
}

impl<F, C> AquaRegistry<F, C>
where
    F: RegistryFetcher,
    C: CacheStore,
{
    /// Get a package definition by ID
    pub async fn package(&self, id: &str) -> Result<AquaPackage> {
        static CACHE: LazyLock<Mutex<HashMap<String, AquaPackage>>> =
            LazyLock::new(|| Mutex::new(HashMap::new()));

        if let Some(pkg) = CACHE.lock().await.get(id) {
            return Ok(pkg.clone());
        }

        let mut pkg = self.fetcher.fetch_package(id).await?;

        pkg.setup_version_filter()?;
        CACHE.lock().await.insert(id.to_string(), pkg.clone());
        Ok(pkg)
    }
}

impl RegistryFetcher for DefaultRegistryFetcher {
    async fn fetch_package(&self, package_id: &str) -> Result<AquaPackage> {
        Err(AquaRegistryError::RegistryNotAvailable(format!(
            "no aqua-registry found for {package_id}"
        )))
    }
}

impl CacheStore for NoOpCacheStore {
    fn is_fresh(&self, _key: &str) -> bool {
        false
    }

    fn store(&self, _key: &str, _data: &[u8]) -> std::io::Result<()> {
        Ok(())
    }

    fn retrieve(&self, _key: &str) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

impl FileCacheStore {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }
}

impl CacheStore for FileCacheStore {
    fn is_fresh(&self, key: &str) -> bool {
        // Check if cache entry exists and is less than a week old
        #[allow(clippy::collapsible_if)]
        if let Ok(metadata) = std::fs::metadata(self.cache_dir.join(key)) {
            if let Ok(modified) = metadata.modified() {
                let age = std::time::SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default();
                return age < std::time::Duration::from_secs(7 * 24 * 60 * 60); // 1 week
            }
        }
        false
    }

    fn store(&self, key: &str, data: &[u8]) -> std::io::Result<()> {
        let path = self.cache_dir.join(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, data)
    }

    fn retrieve(&self, key: &str) -> std::io::Result<Option<Vec<u8>>> {
        let path = self.cache_dir.join(key);
        match std::fs::read(path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_creation() {
        let config = AquaRegistryConfig::default();
        let registry = AquaRegistry::new(config);

        // This should not panic - registry should be created successfully
        drop(registry);
    }

    #[test]
    fn test_cache_store() {
        let cache = NoOpCacheStore;
        assert!(!cache.is_fresh("test"));
        assert!(cache.store("test", b"data").is_ok());
        assert!(cache.retrieve("test").unwrap().is_none());
    }
}
