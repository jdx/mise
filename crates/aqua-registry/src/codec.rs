use crate::types::AquaPackage;
use crate::{AquaRegistryError, Result};
use rkyv::rancor::Error as RkyvError;

pub fn encode_package_rkyv(package: &AquaPackage) -> Result<Vec<u8>> {
    rkyv::to_bytes::<RkyvError>(package)
        .map(|bytes| bytes.to_vec())
        .map_err(|err| {
            AquaRegistryError::RegistryNotAvailable(format!(
                "failed to encode aqua package as rkyv: {err}"
            ))
        })
}

pub fn decode_package_rkyv(package_id: &str, bytes: &[u8]) -> Result<AquaPackage> {
    rkyv::from_bytes::<AquaPackage, RkyvError>(bytes).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to decode aqua package {package_id} from rkyv: {err}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AquaVar;

    #[test]
    fn test_rkyv_package_roundtrip_preserves_var_default() {
        let mut package = AquaPackage::default();
        package.repo_owner = "owner".into();
        package.repo_name = "repo".into();
        package.crate_name = Some("example-crate".into());
        package.private = true;
        package.vars = vec![AquaVar {
            name: "channel".into(),
            default: Some("beta".into()),
            required: false,
        }];

        let bytes = encode_package_rkyv(&package).unwrap();
        let decoded = decode_package_rkyv("owner/repo", &bytes).unwrap();

        assert_eq!(decoded.repo_owner, "owner");
        assert_eq!(decoded.repo_name, "repo");
        assert_eq!(decoded.crate_name.as_deref(), Some("example-crate"));
        assert!(decoded.private);
        assert_eq!(decoded.vars[0].default.as_deref(), Some("beta"));
    }

    // The baked registry is a build script output that mbx and Cargo key caches
    // on, so the same package must always encode to the same bytes.
    #[test]
    fn test_rkyv_encoding_does_not_depend_on_map_insertion_order() {
        let encode = |keys: Vec<usize>| {
            let mut package = AquaPackage::default();
            package.repo_owner = "owner".into();
            package.repo_name = "repo".into();
            for key in keys {
                package
                    .replacements
                    .insert(format!("os-{key}"), format!("value-{key}"));
            }
            encode_package_rkyv(&package).unwrap()
        };

        let forward = encode((0..64).collect());
        for _ in 0..8 {
            assert_eq!(forward, encode((0..64).rev().collect()));
            assert_eq!(forward, encode((0..64).collect()));
        }
    }

    // `checksum.replacements` is a separate map from the package's own, so the
    // test above does not cover it.
    #[test]
    fn test_rkyv_encoding_of_checksum_replacements_does_not_depend_on_key_order() {
        let encode = |keys: Vec<usize>| {
            let entries = keys
                .iter()
                .map(|key| format!("        os-{key}: value-{key}\n"))
                .collect::<String>();
            let yaml = format!(
                "packages:\n  - type: github_release\n    repo_owner: owner\n    repo_name: repo\n    checksum:\n      type: github_release\n      asset: checksums.txt\n      algorithm: sha256\n      replacements:\n{entries}"
            );
            let registry = crate::types::RegistryYaml::from_yaml(&yaml).unwrap();
            encode_package_rkyv(&registry.packages[0].package).unwrap()
        };

        let forward = encode((0..64).collect());
        // The map must reach the encoding at all, or the check below is vacuous.
        assert_ne!(forward, encode((0..63).collect()));
        for _ in 0..8 {
            assert_eq!(forward, encode((0..64).rev().collect()));
            assert_eq!(forward, encode((0..64).collect()));
        }
    }
}
