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
        package.vars = vec![AquaVar {
            name: "channel".into(),
            default: Some("beta".into()),
            required: false,
        }];

        let bytes = encode_package_rkyv(&package).unwrap();
        let decoded = decode_package_rkyv("owner/repo", &bytes).unwrap();

        assert_eq!(decoded.repo_owner, "owner");
        assert_eq!(decoded.repo_name, "repo");
        assert_eq!(decoded.vars[0].default.as_deref(), Some("beta"));
    }
}
