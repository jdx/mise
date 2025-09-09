use aqua_registry::AquaRegistryManager;
use eyre::Result;
use std::sync::LazyLock as Lazy;

pub static AQUA_REGISTRY: Lazy<&AquaRegistryManager> =
    Lazy::new(|| AquaRegistryManager::get_global());

pub struct AquaRegistry;

impl AquaRegistry {
    pub async fn package(&self, id: &str) -> Result<aqua_registry::AquaPackage> {
        AQUA_REGISTRY.package(id).await
    }

    pub async fn package_with_version(
        &self,
        id: &str,
        versions: &[&str],
    ) -> Result<aqua_registry::AquaPackage> {
        AQUA_REGISTRY.package_with_version(id, versions).await
    }
}

// Re-export types for backwards compatibility
pub use aqua_registry::{AquaChecksumType, AquaMinisignType, AquaPackageType};

// Re-export AquaPackage as needed by backend
pub use aqua_registry::AquaPackage;

// Compatibility stub for the doctor
use std::collections::HashMap;

pub static AQUA_STANDARD_REGISTRY_FILES: Lazy<HashMap<&'static str, &'static str>> =
    Lazy::new(|| {
        // For now, return an empty map - this will be replaced with actual baked data
        HashMap::new()
    });
