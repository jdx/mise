use crate::types::{AquaPackage, RegistryIndex, RegistryYaml};
use eyre::{Result, WrapErr};
use indexmap::IndexMap;
use std::path::Path;

pub struct RegistryBuilder {
    packages: IndexMap<String, AquaPackage>,
    aliases: IndexMap<String, String>,
}

impl RegistryBuilder {
    pub fn new() -> Self {
        Self {
            packages: IndexMap::new(),
            aliases: IndexMap::new(),
        }
    }

    pub fn with_baked() -> Self {
        let mut builder = Self::new();
        #[cfg(feature = "baked")]
        {
            builder = builder.merge_baked();
        }
        builder
    }

    #[cfg(feature = "baked")]
    fn merge_baked(mut self) -> Self {
        // This will be populated by build.rs with the baked registry data
        let baked_index: RegistryIndex = super::baked::get_baked_registry();
        self.packages.extend(baked_index.packages_by_name);
        self.aliases.extend(baked_index.aliases);
        self
    }

    pub fn with_registry_file<P: AsRef<Path>>(self, path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .wrap_err_with(|| format!("Failed to read registry file: {:?}", path.as_ref()))?;
        self.with_registry_yaml(&content)
    }

    pub fn try_add_registry_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let content = std::fs::read_to_string(path.as_ref())
            .wrap_err_with(|| format!("Failed to read registry file: {:?}", path.as_ref()))?;
        self.try_add_registry_yaml(&content)
    }

    pub fn try_add_registry_yaml(&mut self, yaml_content: &str) -> Result<()> {
        let registry: RegistryYaml =
            serde_yaml::from_str(yaml_content).wrap_err("Failed to parse registry YAML")?;

        self.merge_registry(registry);
        Ok(())
    }

    pub fn with_registry_yaml(mut self, yaml_content: &str) -> Result<Self> {
        let registry: RegistryYaml =
            serde_yaml::from_str(yaml_content).wrap_err("Failed to parse registry YAML")?;

        self.merge_registry(registry);
        Ok(self)
    }

    pub fn with_git_repo<P: AsRef<Path>>(self, url: &str, cache_dir: P) -> Result<Self> {
        // For now, this is a stub - actual Git implementation would be added here
        // The implementation would:
        // 1. Clone or update the git repository to cache_dir
        // 2. Look for registry.yaml or pkgs/**/registry.yaml files
        // 3. Load them with with_registry_file
        //
        // Since the aqua-registry crate should remain independent of mise's git module,
        // this would need to be implemented by the caller or through dependency injection
        Err(eyre::eyre!(
            "Git repository support for '{}' not yet implemented in aqua-registry crate. Cache dir: {:?}",
            url,
            cache_dir.as_ref()
        ))
    }

    fn merge_registry(&mut self, registry: RegistryYaml) {
        // Merge packages - last wins for collisions
        for package in registry.packages {
            let canonical_name = if !package.repo_owner.is_empty() && !package.repo_name.is_empty()
            {
                format!("{}/{}", package.repo_owner, package.repo_name)
            } else if let Some(name) = &package.name {
                name.clone()
            } else {
                continue; // Skip packages without identifiable names
            };

            self.packages.insert(canonical_name, package);
        }

        // Merge aliases
        if let Some(aliases) = registry.aliases {
            for alias in aliases {
                self.aliases.insert(alias.name, alias.package);
            }
        }
    }

    pub fn build(self) -> Result<RegistryIndex> {
        Ok(RegistryIndex {
            packages_by_name: self.packages,
            aliases: self.aliases,
        })
    }
}

impl Default for RegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}
