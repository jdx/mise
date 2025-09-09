pub mod builder;
pub mod implementations;
pub mod registry;
pub mod types;

#[cfg(feature = "baked")]
pub mod baked {
    use super::types::RegistryIndex;
    use once_cell::sync::Lazy;

    static BAKED_REGISTRY: Lazy<RegistryIndex> =
        Lazy::new(|| include!(concat!(env!("OUT_DIR"), "/aqua_baked.rs")));

    pub fn get_baked_registry() -> RegistryIndex {
        // Clone is needed here because we can't return a reference to the static
        // In a real implementation, we might use Arc or implement a more efficient clone
        BAKED_REGISTRY.clone()
    }
}

pub use builder::RegistryBuilder;
pub use registry::AquaRegistryManager;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;

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
}
