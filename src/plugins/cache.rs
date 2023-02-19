use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde_derive::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PluginCache {
    pub versions: Vec<String>,
    pub legacy_filenames: Vec<String>,
    pub aliases: Vec<(String, String)>,
}

impl PluginCache {
    pub fn parse(path: &Path) -> color_eyre::Result<Self> {
        trace!("reading plugin cache from {}", path.to_string_lossy());
        let mut gz = ZlibDecoder::new(File::open(path)?);
        let mut bytes = Vec::new();
        gz.read_to_end(&mut bytes)?;
        Ok(rmp_serde::from_slice(&bytes)?)
    }

    pub fn write(&self, path: &Path) -> color_eyre::Result<()> {
        trace!("writing plugin cache to {}", path.to_string_lossy());
        let mut gz = ZlibEncoder::new(File::create(path)?, Compression::fast());
        gz.write_all(&rmp_serde::to_vec_named(self)?[..])?;

        Ok(())
    }
}
