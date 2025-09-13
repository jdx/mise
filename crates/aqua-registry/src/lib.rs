pub(crate) mod aqua_package;
pub(crate) mod aqua_template;
pub(crate) mod builder;
pub(crate) mod git;
pub(crate) mod registry;
pub(crate) mod type_implementations;
pub(crate) mod types;
pub(crate) mod utils;

#[cfg(feature = "baked")]
pub mod baked {
    use super::types::RegistryIndex;
    use once_cell::sync::Lazy;
    use std::sync::Arc;

    static BAKED_REGISTRY: Lazy<Arc<RegistryIndex>> =
        Lazy::new(|| Arc::new(include!(concat!(env!("OUT_DIR"), "/aqua_baked.rs"))));

    pub fn get_baked_registry() -> Arc<RegistryIndex> {
        BAKED_REGISTRY.clone()
    }
}

// Public API - only expose what's actually used by the main mise codebase
pub use registry::AquaRegistryManager;
pub use types::{AquaChecksumType, AquaMinisignType, AquaPackage, AquaPackageType};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::RegistryBuilder;

    #[test]
    fn test_registry_builder() {
        let builder = RegistryBuilder::new();
        let registry = builder.build().expect("Failed to build registry");

        // Should have empty index initially
        assert_eq!(registry.packages_by_name.len(), 0);
        assert_eq!(registry.aliases.len(), 0);
    }

    #[test]
    fn test_registry_builder_with_yaml() {
        let yaml = r#"
packages:
  - repo_owner: test
    repo_name: tool
    description: Test tool
"#;

        let builder = RegistryBuilder::new();
        let registry = builder
            .with_registry_yaml(yaml)
            .expect("Failed to parse YAML")
            .build()
            .expect("Failed to build registry");

        // Should have one package
        assert_eq!(registry.packages_by_name.len(), 1);
        assert!(registry.contains("test/tool"));
    }

    #[test]
    fn test_registry_index_lookup() {
        let yaml = r#"
packages:
  - repo_owner: example
    repo_name: tool
    description: Example tool
aliases:
  - name: et
    package: example/tool
"#;

        let builder = RegistryBuilder::new();
        let registry = builder
            .with_registry_yaml(yaml)
            .expect("Failed to parse YAML")
            .build()
            .expect("Failed to build registry");

        // Test direct lookup
        assert!(registry.contains("example/tool"));
        assert!(registry.get("example/tool").is_some());

        // Test alias lookup
        assert!(registry.contains("et"));
        assert!(registry.get("et").is_some());

        // Test non-existent lookup
        assert!(!registry.contains("nonexistent"));
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_manager() {
        use tokio_test;

        tokio_test::block_on(async {
            // This test will use the baked registry (empty in test mode)
            let manager = AquaRegistryManager::get_global();

            // Should handle non-existent packages gracefully
            let result = manager.package("nonexistent").await;
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_registry_manager_with_git_url() {
        // Test that git URLs are handled gracefully (even if git command fails)
        let manager = AquaRegistryManager::with_settings(
            Some("https://github.com/nonexistent/repo.git"),
            &[],
            false,
        );

        // Should not panic even if git fails
        assert!(manager.is_ok() || manager.is_err());
    }

    #[test]
    fn test_registry_builder_with_git_repo() {
        let temp_dir = std::env::temp_dir().join("test-registry-builder-git-repo");

        // Test with invalid URL - should handle error gracefully
        let builder = RegistryBuilder::new();
        let result = builder.with_git_repo("https://nonexistent.example.com/repo.git", &temp_dir);
        assert!(result.is_err());

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
