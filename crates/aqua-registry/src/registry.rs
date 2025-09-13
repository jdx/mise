use crate::git;
use crate::types::{AquaPackage, RegistryIndex};
use crate::RegistryBuilder;
use dashmap::DashMap;
use eyre::Result;
use once_cell::sync::OnceCell;
use std::path::PathBuf;
use std::sync::Arc;

static GLOBAL_REGISTRY: OnceCell<Arc<AquaRegistryManager>> = OnceCell::new();

pub struct AquaRegistryManager {
    index: RegistryIndex,
    cache: DashMap<String, AquaPackage>,
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
        Self::with_settings(None, &[], true)
    }

    pub fn with_settings(
        registry_url: Option<&str>,
        additional_registries: &[String],
        use_baked_registry: bool,
    ) -> Result<Self> {
        let mut builder = if use_baked_registry {
            RegistryBuilder::with_baked()
        } else {
            RegistryBuilder::new()
        };

        let mut failed_registries = Vec::new();

        // Collect all successful registries to load
        let mut registries_to_load = Vec::new();

        // Add default registry if provided
        if let Some(url) = registry_url {
            if url.starts_with("http://") || url.starts_with("https://") {
                // Handle git repositories
                let cache_dir = std::env::temp_dir()
                    .join("mise-aqua-registry-cache")
                    .join("default");
                match git::clone_or_update_registry(url, &cache_dir) {
                    Ok(registry_files) => {
                        for registry_file in registry_files {
                            if let Err(err) = builder.try_add_registry_file(&registry_file) {
                                eprintln!(
                                    "Failed to load registry file {:?}: {err:?}",
                                    registry_file
                                );
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to clone/update git registry from {}: {err:?}", url);
                        failed_registries.push(url.to_string());
                    }
                }
            } else {
                registries_to_load.push(url);
            }
        }

        // Add additional registries
        for (idx, registry) in additional_registries.iter().enumerate() {
            if registry.starts_with("http://") || registry.starts_with("https://") {
                // Handle git repositories
                let cache_dir = std::env::temp_dir()
                    .join("mise-aqua-registry-cache")
                    .join(format!("additional-{}", idx));
                match git::clone_or_update_registry(registry, &cache_dir) {
                    Ok(registry_files) => {
                        for registry_file in registry_files {
                            if let Err(err) = builder.try_add_registry_file(&registry_file) {
                                eprintln!(
                                    "Failed to load registry file {:?}: {err:?}",
                                    registry_file
                                );
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!(
                            "Failed to clone/update git registry from {}: {err:?}",
                            registry
                        );
                        failed_registries.push(registry.clone());
                    }
                }
            } else {
                registries_to_load.push(registry);
            }
        }

        // Load registries one by one, continuing on failures
        for registry in registries_to_load {
            if let Err(err) = builder.try_add_registry_file(registry) {
                eprintln!("Failed to load registry from {}: {err:?}", registry);
                failed_registries.push(registry.to_string());
                // Continue with other registries even if this one fails
            }
        }

        // Always try to build the index, even if some registries failed
        let index = builder.build()?;

        // Log summary of failures but don't fail entirely
        if !failed_registries.is_empty() {
            eprintln!(
                "Warning: {} registry sources failed to load: {}",
                failed_registries.len(),
                failed_registries.join(", ")
            );
        }

        Ok(Self {
            index,
            cache: DashMap::new(),
        })
    }

    fn empty() -> Self {
        Self {
            index: RegistryIndex {
                packages_by_name: indexmap::IndexMap::new(),
                aliases: indexmap::IndexMap::new(),
            },
            cache: DashMap::new(),
        }
    }

    pub async fn package(&self, id: &str) -> Result<AquaPackage> {
        // Check cache first
        if let Some(pkg) = self.cache.get(id) {
            return Ok(pkg.clone());
        }

        // Get from index
        if let Some(pkg) = self.index.get(id) {
            let pkg = pkg.clone();

            // Cache it
            self.cache.insert(id.to_string(), pkg.clone());

            Ok(pkg)
        } else {
            // Provide more helpful error message
            let available_count = self.index.packages_by_name.len();
            let alias_count = self.index.aliases.len();
            eyre::bail!(
                "Package '{}' not found in registry (searched {} packages and {} aliases)",
                id,
                available_count,
                alias_count
            )
        }
    }

    pub async fn package_with_version(&self, id: &str, versions: &[&str]) -> Result<AquaPackage> {
        let mut pkg = self.package(id).await?;

        // Apply version-specific overrides
        pkg = pkg.with_version(versions);

        Ok(pkg)
    }

    /// Create a new registry manager with Git repository support
    /// This method is designed to be called from the main mise crate with Git functionality
    pub fn with_git_support(
        registry_url: Option<&str>,
        additional_registries: &[String],
        use_baked_registry: bool,
        git_clone_fn: Option<Box<dyn Fn(&str, &PathBuf) -> Result<Vec<PathBuf>>>>,
    ) -> Result<Self> {
        let mut builder = if use_baked_registry {
            RegistryBuilder::with_baked()
        } else {
            RegistryBuilder::new()
        };

        let mut failed_registries = Vec::new();
        let cache_base = std::env::temp_dir().join("mise-aqua-registry-cache");

        // Handle default registry
        if let Some(url) = registry_url {
            if url.starts_with("http://") || url.starts_with("https://") {
                if let Some(ref git_fn) = git_clone_fn {
                    let cache_dir = cache_base.join("default");
                    match git_fn(url, &cache_dir) {
                        Ok(registry_files) => {
                            for registry_file in registry_files {
                                if let Err(err) = builder.try_add_registry_file(&registry_file) {
                                    eprintln!(
                                        "Failed to load registry file {:?}: {err:?}",
                                        registry_file
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            eprintln!("Failed to clone Git registry from {}: {err:?}", url);
                            failed_registries.push(url.to_string());
                        }
                    }
                } else {
                    eprintln!("Git support not available for URL: {}", url);
                    failed_registries.push(url.to_string());
                }
            } else {
                // Handle as file path
                if let Err(err) = builder.try_add_registry_file(url) {
                    eprintln!("Failed to load registry from {}: {err:?}", url);
                    failed_registries.push(url.to_string());
                }
            }
        }

        // Handle additional registries
        for (idx, registry) in additional_registries.iter().enumerate() {
            if registry.starts_with("http://") || registry.starts_with("https://") {
                if let Some(ref git_fn) = git_clone_fn {
                    let cache_dir = cache_base.join(format!("additional-{}", idx));
                    match git_fn(registry, &cache_dir) {
                        Ok(registry_files) => {
                            for registry_file in registry_files {
                                if let Err(err) = builder.try_add_registry_file(&registry_file) {
                                    eprintln!(
                                        "Failed to load registry file {:?}: {err:?}",
                                        registry_file
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            eprintln!("Failed to clone Git registry from {}: {err:?}", registry);
                            failed_registries.push(registry.clone());
                        }
                    }
                } else {
                    eprintln!("Git support not available for URL: {}", registry);
                    failed_registries.push(registry.clone());
                }
            } else {
                // Handle as file path
                if let Err(err) = builder.try_add_registry_file(registry) {
                    eprintln!("Failed to load registry from {}: {err:?}", registry);
                    failed_registries.push(registry.clone());
                }
            }
        }

        let index = builder.build()?;

        if !failed_registries.is_empty() {
            eprintln!(
                "Warning: {} registry sources failed to load: {}",
                failed_registries.len(),
                failed_registries.join(", ")
            );
        }

        Ok(Self {
            index,
            cache: DashMap::new(),
        })
    }
}
