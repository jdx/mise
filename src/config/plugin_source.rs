use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use crate::cli::args::runtime::RuntimeArg;
use crate::file::display_path;

#[derive(Debug, Clone)]
pub enum PluginSource {
    ToolVersions(PathBuf),
    RtxRc(PathBuf),
    LegacyVersionFile(PathBuf),
    Argument(RuntimeArg),
    Environment(String, String),
}

impl Display for PluginSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            PluginSource::ToolVersions(path) => write!(f, "{}", display_path(path)),
            PluginSource::RtxRc(path) => write!(f, "{}", display_path(path)),
            PluginSource::LegacyVersionFile(path) => write!(f, "{}", display_path(path)),
            PluginSource::Argument(arg) => write!(f, "--runtime {arg}"),
            PluginSource::Environment(k, v) => write!(f, "{k}={v}"),
        }
    }
}
