use crate::backend::aqua::{arch, os};
use crate::config::Settings;
use crate::git::{CloneOptions, Git};
use crate::{dirs, duration::WEEKLY, file};
use aqua_registry::{AquaRegistry, AquaRegistryConfig};
use eyre::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;
use tokio::sync::Mutex;

static AQUA_REGISTRY_PATH: Lazy<PathBuf> = Lazy::new(|| dirs::CACHE.join("aqua-registry"));
static AQUA_DEFAULT_REGISTRY_URL: &str = "https://github.com/aquaproj/aqua-registry";

pub static AQUA_REGISTRY: Lazy<MiseAquaRegistry> = Lazy::new(|| {
    MiseAquaRegistry::standard().unwrap_or_else(|err| {
        warn!("failed to initialize aqua registry: {err:?}");
        MiseAquaRegistry::default()
    })
});

/// Wrapper around the aqua-registry crate that provides mise-specific functionality
#[derive(Debug)]
pub struct MiseAquaRegistry {
    inner: AquaRegistry,
    #[allow(dead_code)]
    path: PathBuf,
    #[allow(dead_code)]
    repo_exists: bool,
}

impl Default for MiseAquaRegistry {
    fn default() -> Self {
        let config = AquaRegistryConfig::default();
        let inner = AquaRegistry::new(config.clone());
        Self {
            inner,
            path: config.cache_dir,
            repo_exists: false,
        }
    }
}

impl MiseAquaRegistry {
    pub fn standard() -> Result<Self> {
        let path = AQUA_REGISTRY_PATH.clone();
        let repo = Git::new(&path);
        let settings = Settings::get();
        let registry_url =
            settings
                .aqua
                .registry_url
                .as_deref()
                .or(if settings.aqua.baked_registry {
                    None
                } else {
                    Some(AQUA_DEFAULT_REGISTRY_URL)
                });

        if let Some(registry_url) = registry_url {
            if repo.exists() {
                fetch_latest_repo(&repo)?;
            } else {
                info!("cloning aqua registry from {registry_url} to {path:?}");
                repo.clone(registry_url, CloneOptions::default())?;
            }
        }

        let config = AquaRegistryConfig {
            cache_dir: path.clone(),
            registry_url: registry_url.map(|s| s.to_string()),
            use_baked_registry: settings.aqua.baked_registry,
            prefer_offline: settings.prefer_offline(),
        };

        let inner = AquaRegistry::new(config);

        Ok(Self {
            inner,
            path,
            repo_exists: repo.exists(),
        })
    }

    pub async fn package(&self, id: &str) -> Result<AquaPackage> {
        static CACHE: Lazy<Mutex<HashMap<String, AquaPackage>>> =
            Lazy::new(|| Mutex::new(HashMap::new()));

        if let Some(pkg) = CACHE.lock().await.get(id) {
            return Ok(pkg.clone());
        }

        let pkg = self.inner.package(id).await?;
        CACHE.lock().await.insert(id.to_string(), pkg.clone());
        Ok(pkg)
    }

    pub async fn package_with_version(&self, id: &str, versions: &[&str]) -> Result<AquaPackage> {
        let pkg = self.package(id).await?;
        Ok(pkg.with_version(versions, os(), arch()))
    }
}

fn fetch_latest_repo(repo: &Git) -> Result<()> {
    if file::modified_duration(&repo.dir)? < WEEKLY {
        return Ok(());
    }

    if Settings::get().prefer_offline() {
        trace!("skipping aqua registry update due to prefer-offline mode");
        return Ok(());
    }

    info!("updating aqua registry repo");
    repo.update(None)?;
    Ok(())
}

struct AquaSuggestionsCache {
    name_to_ids: HashMap<&'static str, Vec<&'static str>>,
    names: Vec<&'static str>,
}

static AQUA_SUGGESTIONS_CACHE: Lazy<AquaSuggestionsCache> = Lazy::new(|| {
    let ids = aqua_registry::package_ids();
    let mut name_to_ids: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
    for id in ids {
        if let Some((_, name)) = id.rsplit_once('/') {
            name_to_ids.entry(name).or_default().push(id);
        }
    }
    let names = name_to_ids.keys().copied().collect();
    AquaSuggestionsCache { name_to_ids, names }
});

/// Search aqua packages by tool name, returning "owner/name" IDs
/// where the name part is similar to the query.
pub fn aqua_suggest(query: &str) -> Vec<String> {
    let cache = &*AQUA_SUGGESTIONS_CACHE;

    // Use a higher threshold (0.8) to avoid noisy suggestions
    let similar_names = xx::suggest::similar_n_with_threshold(query, &cache.names, 5, 0.8);

    // Map back to full IDs
    let mut results = Vec::new();
    for matched_name in &similar_names {
        if let Some(full_ids) = cache.name_to_ids.get(matched_name.as_str()) {
            for full_id in full_ids {
                results.push(full_id.to_string());
                if results.len() >= 5 {
                    return results;
                }
            }
        }
    }
    results
}

// Re-export types and static for compatibility
pub use aqua_registry::{
    AquaChecksum, AquaChecksumType, AquaMinisignType, AquaPackage, AquaPackageType,
};
