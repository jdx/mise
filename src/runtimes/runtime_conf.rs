use serde_derive::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct RuntimeConf {
    pub bin_paths: Vec<String>,
}

impl RuntimeConf {
    pub fn parse(path: &Path) -> color_eyre::Result<Self> {
        Ok(rmp_serde::from_read(File::open(path)?)?)
        // let contents = std::fs::read_to_string(path)
        //     .wrap_err_with(|| format!("failed to read {}", path.to_string_lossy()))?;
        // let conf: Self = toml::from_str(&contents)
        //     .wrap_err_with(|| format!("failed to from_file {}", path.to_string_lossy()))?;

        // Ok(conf)
    }

    pub fn write(&self, path: &Path) -> color_eyre::Result<()> {
        let bytes = rmp_serde::to_vec_named(self)?;
        File::create(path)?.write_all(&bytes)?;
        Ok(())
    }
}
