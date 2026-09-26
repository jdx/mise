//! Locating and reading the global mise configuration file that commands
//! edit in place (`mise settings set`, `mise dot track`, history sync).

use std::path::{Path, PathBuf};

use eyre::Result;
use toml_edit::DocumentMut;

use crate::file::{self, display_path};

/// `config.toml`, or `config.local.toml` next to it for machine-only
/// declarations.
pub fn declaration_file(local: bool) -> Result<PathBuf> {
    let global = crate::config::global_shared_config_path();
    if !local {
        return Ok(global);
    }
    let dir = global.parent().unwrap_or(Path::new("."));
    Ok(dir.join("config.local.toml"))
}

pub fn read_document(path: &Path) -> Result<DocumentMut> {
    if path.exists() {
        let text = file::read_to_string(path)?;
        Ok(text
            .parse::<DocumentMut>()
            .map_err(|err| eyre::eyre!("parsing {}: {err}", display_path(path)))?)
    } else {
        Ok(DocumentMut::new())
    }
}
