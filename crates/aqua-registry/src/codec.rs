use crate::{AquaRegistryError, Result};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::io::{Read, Write};

use crate::types::AquaPackage;

pub fn encode_package_msgpack_z(package: &AquaPackage) -> Result<Vec<u8>> {
    let packed = rmp_serde::to_vec_named(package).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to encode aqua package as MessagePack: {err}"
        ))
    })?;
    let mut zlib = ZlibEncoder::new(Vec::new(), Compression::best());
    zlib.write_all(&packed)?;
    Ok(zlib.finish()?)
}

pub fn decode_package_msgpack_z(package_id: &str, bytes: &[u8]) -> Result<AquaPackage> {
    let mut zlib = ZlibDecoder::new(bytes);
    let mut packed = Vec::new();
    zlib.read_to_end(&mut packed).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to decompress aqua package {package_id}: {err}"
        ))
    })?;
    rmp_serde::from_slice(&packed).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to decode aqua package {package_id}: {err}"
        ))
    })
}
