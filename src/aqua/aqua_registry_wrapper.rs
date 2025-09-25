use crate::backend::aqua::{arch, os};
use crate::config::Settings;
use crate::git::{CloneOptions, Git};
use crate::{dirs, duration::WEEKLY, env, file};
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
            prefer_offline: env::PREFER_OFFLINE.load(std::sync::atomic::Ordering::Relaxed),
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

    if env::PREFER_OFFLINE.load(std::sync::atomic::Ordering::Relaxed) {
        trace!("skipping aqua registry update due to PREFER_OFFLINE");
        return Ok(());
    }

    info!("updating aqua registry repo");
    repo.update(None)?;
    Ok(())
}

// Re-export types and static for compatibility
pub use aqua_registry::{AquaChecksumType, AquaMinisignType, AquaPackage, AquaPackageType};
