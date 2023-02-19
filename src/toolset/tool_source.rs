use crate::file::display_path;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

/// where a tool version came from (e.g.: .tool-versions)
#[derive(Debug, Clone)]
pub enum ToolSource {
    ToolVersions(PathBuf),
    // RtxRc(PathBuf),
    LegacyVersionFile(PathBuf),
    Argument,
    Environment(String, String),
}

impl Display for ToolSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            ToolSource::ToolVersions(path) => write!(f, "{}", display_path(path)),
            // ToolSource::RtxRc(path) => write!(f, "{}", display_path(path)),
            ToolSource::LegacyVersionFile(path) => write!(f, "{}", display_path(path)),
            ToolSource::Argument => write!(f, "--runtime"),
            ToolSource::Environment(k, v) => write!(f, "{k}={v}"),
        }
    }
}
