use crate::dirs;
use std::path::{Path, PathBuf};

pub fn normalize_env_path(config_root: &Path, p: PathBuf) -> PathBuf {
    let p = p.strip_prefix("./").unwrap_or(&p);
    match p.strip_prefix("~/") {
        Ok(p) => dirs::HOME.join(p),
        _ if p.is_relative() => config_root.join(p),
        _ => p.to_path_buf(),
    }
}
