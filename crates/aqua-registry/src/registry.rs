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

/// Baked merged registry YAML (compiled into binary).
pub static AQUA_STANDARD_REGISTRY: &str =
    include!(concat!(env!("OUT_DIR"), "/aqua_standard_registry.rs"));

#[derive(Debug)]
struct BakedRegistryIndex {
    packages: HashMap<String, AquaPackage>,
    aliases: HashMap<String, String>,
}

static BAKED_REGISTRY_INDEX: LazyLock<BakedRegistryIndex> = LazyLock::new(|| {
    BakedRegistryIndex::from_yaml(AQUA_STANDARD_REGISTRY)
        .expect("Failed to parse baked aqua registry")
});

impl BakedRegistryIndex {
    fn from_yaml(content: &str) -> Result<Self> {
        let registry: RegistryYaml = serde_yaml::from_str(content)?;
        let mut packages = HashMap::new();
        let mut pending_aliases = Vec::new();

        for package in registry.packages {
            let Some(id) = canonical_package_id(&package) else {
                continue;
            };
            for alias in &package.aliases {
                if alias.name != id {
                    pending_aliases.push((alias.name.clone(), id.clone()));
                }
            }
            packages.insert(id, package);
        }

        let aliases = pending_aliases
            .into_iter()
            .filter(|(alias, _)| !packages.contains_key(alias))
            .collect();

        Ok(Self { packages, aliases })
    }

    fn package(&self, package_id: &str) -> Option<AquaPackage> {
        if let Some(package) = self.packages.get(package_id) {
            return Some(package.clone());
        }

        self.aliases
            .get(package_id)
            .and_then(|canonical| self.packages.get(canonical))
            .cloned()
    }

    fn package_ids(&self) -> Vec<String> {
        self.packages.keys().cloned().collect()
    }
}

fn canonical_package_id(package: &AquaPackage) -> Option<String> {
    package.name.clone().or_else(|| {
        (!package.repo_owner.is_empty() && !package.repo_name.is_empty())
            .then(|| format!("{}/{}", package.repo_owner, package.repo_name))
    })
}

/// Returns all package IDs from the baked-in aqua registry.
pub fn package_ids() -> Vec<String> {
    BAKED_REGISTRY_INDEX.package_ids()
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
            && let Some(package) = BAKED_REGISTRY_INDEX.package(package_id)
        {
            log::trace!("reading baked-in aqua-registry for {package_id}");
            return Ok(RegistryYaml {
                packages: vec![package],
            });
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
    fn test_baked_registry_index_package_lookup() {
        let index = BakedRegistryIndex::from_yaml(
            r#"
packages:
  - type: github_release
    repo_owner: example
    repo_name: canonical
  - type: github_release
    name: example/named
    repo_owner: example
    repo_name: renamed
"#,
        )
        .unwrap();

        assert!(index.package("example/canonical").is_some());
        assert!(index.package("example/named").is_some());
        assert!(index.package("example/renamed").is_none());
    }

    #[test]
    fn test_baked_registry_index_alias_lookup() {
        let index = BakedRegistryIndex::from_yaml(
            r#"
packages:
  - type: github_release
    name: example/canonical
    repo_owner: example
    repo_name: canonical
    aliases:
      - name: example/alias
      - name: example/self
  - type: github_release
    name: example/self
    repo_owner: example
    repo_name: self
"#,
        )
        .unwrap();

        let alias_package = index.package("example/alias").unwrap();
        assert_eq!(alias_package.name.as_deref(), Some("example/canonical"));

        let canonical_package = index.package("example/self").unwrap();
        assert_eq!(canonical_package.name.as_deref(), Some("example/self"));
    }
}
