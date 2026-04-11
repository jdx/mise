use crate::types::{AquaPackage, RegistryYaml};
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
    #[allow(dead_code)]
    repo_exists: bool,
}

/// Default implementation of RegistryFetcher
#[derive(Debug, Clone)]
pub struct DefaultRegistryFetcher {
    config: AquaRegistryConfig,
}

/// No-op implementation of CacheStore
#[derive(Debug, Clone, Default)]
pub struct NoOpCacheStore;

/// File-based cache store implementation
#[derive(Debug, Clone)]
pub struct FileCacheStore {
    cache_dir: PathBuf,
}

/// Baked canonical registry files (compiled into binary).
pub static AQUA_STANDARD_REGISTRY_FILES: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| include!(concat!(env!("OUT_DIR"), "/aqua_standard_registry_files.rs")));

/// Baked alias-to-canonical package ID map (compiled into binary).
static AQUA_STANDARD_REGISTRY_ALIASES: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| {
        include!(concat!(
            env!("OUT_DIR"),
            "/aqua_standard_registry_aliases.rs"
        ))
    });

/// Returns all package IDs from the baked-in aqua registry.
pub fn package_ids() -> Vec<&'static str> {
    AQUA_STANDARD_REGISTRY_FILES.keys().copied().collect()
}

fn baked_registry_file(package_id: &str) -> Option<&'static str> {
    if let Some(content) = AQUA_STANDARD_REGISTRY_FILES.get(package_id) {
        return Some(*content);
    }

    AQUA_STANDARD_REGISTRY_ALIASES
        .get(package_id)
        .and_then(|canonical| AQUA_STANDARD_REGISTRY_FILES.get(*canonical))
        .copied()
}

impl AquaRegistry {
    /// Create a new AquaRegistry with the given configuration
    pub fn new(config: AquaRegistryConfig) -> Self {
        let repo_exists = Self::check_repo_exists(&config.cache_dir);
        let fetcher = DefaultRegistryFetcher {
            config: config.clone(),
        };
        Self {
            config,
            fetcher,
            cache_store: NoOpCacheStore,
            repo_exists,
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
        let repo_exists = Self::check_repo_exists(&config.cache_dir);
        AquaRegistry {
            config,
            fetcher,
            cache_store,
            repo_exists,
        }
    }

    fn check_repo_exists(cache_dir: &std::path::Path) -> bool {
        cache_dir.join(".git").exists()
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

        let registry = self.fetcher.fetch_registry(id).await?;
        let mut pkg = registry
            .packages
            .into_iter()
            .next()
            .ok_or_else(|| AquaRegistryError::PackageNotFound(id.to_string()))?;

        pkg.setup_version_filter()?;
        CACHE.lock().await.insert(id.to_string(), pkg.clone());
        Ok(pkg)
    }

    /// Get a package definition configured for specific versions
    pub async fn package_with_version(
        &self,
        id: &str,
        versions: &[&str],
        os: &str,
        arch: &str,
    ) -> Result<AquaPackage> {
        Ok(self.package(id).await?.with_version(versions, os, arch))
    }
}

impl RegistryFetcher for DefaultRegistryFetcher {
    async fn fetch_registry(&self, package_id: &str) -> Result<RegistryYaml> {
        let path_id = package_id
            .split('/')
            .collect::<Vec<_>>()
            .join(std::path::MAIN_SEPARATOR_STR);
        let path = self
            .config
            .cache_dir
            .join("pkgs")
            .join(&path_id)
            .join("registry.yaml");

        // Try to read from local repository first
        if self.config.cache_dir.join(".git").exists() && path.exists() {
            log::trace!("reading aqua-registry for {package_id} from repo at {path:?}");
            let contents = std::fs::read_to_string(&path)?;
            return Ok(serde_yaml::from_str(&contents)?);
        }

        // Fall back to baked registry if enabled
        if self.config.use_baked_registry
            && let Some(content) = baked_registry_file(package_id)
        {
            log::trace!("reading baked-in aqua-registry for {package_id}");
            return Ok(serde_yaml::from_str(content)?);
        }

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

    #[test]
    fn test_baked_registry_package_lookup() {
        let registry = baked_registry_file("01mf02/jaq").unwrap();
        let registry = serde_yaml::from_str::<RegistryYaml>(registry).unwrap();

        let package = registry.packages.into_iter().next().unwrap();
        assert_eq!(package.repo_owner, "01mf02");
        assert_eq!(package.repo_name, "jaq");
    }

    #[test]
    fn test_baked_registry_alias_lookup() {
        let alias = "elijah-potter/harper/harper-ls";

        assert!(!AQUA_STANDARD_REGISTRY_FILES.contains_key(alias));
        assert_eq!(
            AQUA_STANDARD_REGISTRY_ALIASES.get(alias).copied(),
            Some("Automattic/harper/harper-ls")
        );

        let registry = baked_registry_file(alias).unwrap();
        let registry = serde_yaml::from_str::<RegistryYaml>(registry).unwrap();

        let package = registry.packages.into_iter().next().unwrap();
        assert_eq!(package.name.as_deref(), Some("Automattic/harper/harper-ls"));
    }
}
