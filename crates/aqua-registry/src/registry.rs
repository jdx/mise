use crate::types::{AquaPackage, RegistryIndex};
use crate::RegistryBuilder;
use eyre::Result;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

static GLOBAL_REGISTRY: OnceCell<Arc<AquaRegistryManager>> = OnceCell::new();

pub struct AquaRegistryManager {
    index: RegistryIndex,
    cache: Mutex<HashMap<String, AquaPackage>>,
}

impl AquaRegistryManager {
    pub fn get_global() -> &'static Arc<AquaRegistryManager> {
        GLOBAL_REGISTRY.get_or_init(|| {
            Arc::new(Self::new().unwrap_or_else(|err| {
                eprintln!("Failed to initialize aqua registry: {err:?}");
                Self::empty()
            }))
        })
    }

    fn new() -> Result<Self> {
        let builder = RegistryBuilder::with_baked();

        // TODO: Add custom registries from settings
        // TODO: Add git repo support from settings

        let index = builder.build()?;

        Ok(Self {
            index,
            cache: Mutex::new(HashMap::new()),
        })
    }

    fn empty() -> Self {
        Self {
            index: RegistryIndex {
                packages_by_name: indexmap::IndexMap::new(),
                aliases: indexmap::IndexMap::new(),
            },
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub async fn package(&self, id: &str) -> Result<AquaPackage> {
        // Check cache first
        if let Some(pkg) = self.cache.lock().await.get(id) {
            return Ok(pkg.clone());
        }

        // Get from index
        if let Some(pkg) = self.index.get(id) {
            let pkg = pkg.clone();

            // Cache it
            self.cache.lock().await.insert(id.to_string(), pkg.clone());

            Ok(pkg)
        } else {
            eyre::bail!("Package not found: {}", id)
        }
    }

    pub async fn package_with_version(&self, id: &str, versions: &[&str]) -> Result<AquaPackage> {
        let mut pkg = self.package(id).await?;

        // Apply version-specific overrides
        pkg = pkg.with_version(versions);

        Ok(pkg)
    }
}
