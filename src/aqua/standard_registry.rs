use aqua_registry::{AquaPackage, Result, decode_package_rkyv};

/// Metadata for the baked aqua registry snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AquaRegistryMetadata {
    pub repository: &'static str,
    pub tag: &'static str,
}

/// Baked canonical registry packages (compiled into the mise binary).
pub static AQUA_STANDARD_REGISTRY_FILES: phf::Map<&'static str, &'static [u8]> =
    include!(concat!(env!("OUT_DIR"), "/aqua_standard_registry_files.rs"));

/// Baked aqua registry snapshot metadata (compiled into the mise binary).
pub static AQUA_STANDARD_REGISTRY_METADATA: AquaRegistryMetadata = include!(concat!(
    env!("OUT_DIR"),
    "/aqua_standard_registry_metadata.rs"
));

/// Baked alias-to-canonical package ID map (compiled into the mise binary).
static AQUA_STANDARD_REGISTRY_ALIASES: phf::Map<&'static str, &'static str> = include!(concat!(
    env!("OUT_DIR"),
    "/aqua_standard_registry_aliases.rs"
));

/// Returns all package IDs from the baked-in aqua registry.
pub fn package_ids() -> Vec<&'static str> {
    AQUA_STANDARD_REGISTRY_FILES.keys().copied().collect()
}

pub fn package(package_id: &str) -> Option<Result<AquaPackage>> {
    baked_registry_file(package_id).map(|content| decode_package_rkyv(package_id, content))
}

fn baked_registry_file(package_id: &str) -> Option<&'static [u8]> {
    if let Some(content) = AQUA_STANDARD_REGISTRY_FILES.get(package_id) {
        return Some(*content);
    }

    AQUA_STANDARD_REGISTRY_ALIASES
        .get(package_id)
        .and_then(|canonical| AQUA_STANDARD_REGISTRY_FILES.get(*canonical))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baked_registry_package_lookup() {
        let package = package("01mf02/jaq").unwrap().unwrap();

        assert_eq!(package.repo_owner, "01mf02");
        assert_eq!(package.repo_name, "jaq");
    }

    #[test]
    fn test_baked_registry_path_only_package_lookup() {
        let package = package("golang.org/x/perf/cmd/benchstat").unwrap().unwrap();

        assert_eq!(
            package.path.as_deref(),
            Some("golang.org/x/perf/cmd/benchstat")
        );
    }

    #[test]
    fn test_baked_registry_metadata() {
        assert_eq!(
            AQUA_STANDARD_REGISTRY_METADATA.repository,
            "aquaproj/aqua-registry"
        );
        assert!(!AQUA_STANDARD_REGISTRY_METADATA.tag.is_empty());
        assert!(AQUA_STANDARD_REGISTRY_METADATA.tag.starts_with('v'));
    }

    #[test]
    fn test_baked_registry_alias_lookup() {
        let alias = "elijah-potter/harper/harper-ls";

        assert!(!AQUA_STANDARD_REGISTRY_FILES.contains_key(alias));
        assert_eq!(
            AQUA_STANDARD_REGISTRY_ALIASES.get(alias).copied(),
            Some("Automattic/harper/harper-ls")
        );

        let alias_package = package(alias).unwrap().unwrap();
        let canonical_package = package("Automattic/harper/harper-ls").unwrap().unwrap();

        assert_eq!(
            alias_package.name.as_deref(),
            Some("Automattic/harper/harper-ls")
        );
        assert_eq!(
            alias_package.name.as_deref(),
            canonical_package.name.as_deref()
        );
        assert_eq!(alias_package.repo_owner, canonical_package.repo_owner);
        assert_eq!(alias_package.repo_name, canonical_package.repo_name);
    }

    #[test]
    fn test_baked_registry_numeric_replacement_keys() {
        let package = package("sharkdp/hyperfine").unwrap().unwrap();

        assert_eq!(package.replacements.get("386"), Some(&"i686".to_string()));
    }
}
