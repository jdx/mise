use std::env::{join_paths, split_paths};
use std::path::PathBuf;

use crate::env;

#[cfg(windows)]
pub fn setup() -> color_eyre::Result<PathBuf> {
    let path = env::MISE_DATA_DIR.join(".fake-asdf");
    Ok(path)
}

pub fn get_path_with_fake_asdf() -> String {
    let mut path = split_paths(&env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    match setup() {
        Ok(fake_asdf_path) => {
            path.insert(0, fake_asdf_path);
        }
        Err(e) => {
            warn!("Failed to setup fake asdf: {:#}", e);
        }
    };
    join_paths(path).unwrap().to_string_lossy().to_string()
}
