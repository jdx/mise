use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use miette::Result;

use crate::plugins::Plugin;
use crate::toolset::{ToolVersion, ToolVersionOptions};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ToolVersionRequest {
    Version(String, String),
    Prefix(String, String),
    Ref(String, String),
    Path(String, PathBuf),
    Sub {
        plugin_name: String,
        sub: String,
        orig_version: String,
    },
    System(String),
}

impl ToolVersionRequest {
    pub fn new(plugin_name: String, s: &str) -> Self {
        let s = match s.split_once('-') {
            Some(("ref", r)) => format!("ref:{}", r),
            _ => s.to_string(),
        };
        match s.split_once(':') {
            Some(("ref", r)) => Self::Ref(plugin_name, r.to_string()),
            Some(("prefix", p)) => Self::Prefix(plugin_name, p.to_string()),
            Some(("path", p)) => Self::Path(plugin_name, PathBuf::from(p)),
            Some((p, v)) if p.starts_with("sub-") => Self::Sub {
                plugin_name,
                sub: p.split_once('-').unwrap().1.to_string(),
                orig_version: v.to_string(),
            },
            None => {
                if s == "system" {
                    Self::System(plugin_name)
                } else {
                    Self::Version(plugin_name, s.to_string())
                }
            }
            _ => panic!("invalid tool version request: {s}"),
        }
    }

    pub fn plugin_name(&self) -> &String {
        match self {
            Self::Version(p, _) => p,
            Self::Prefix(p, _) => p,
            Self::Ref(p, _) => p,
            Self::Path(p, _) => p,
            Self::Sub { plugin_name, .. } => plugin_name,
            Self::System(p) => p,
        }
    }

    pub fn version(&self) -> String {
        match self {
            Self::Version(_, v) => v.clone(),
            Self::Prefix(_, p) => format!("prefix:{p}"),
            Self::Ref(_, r) => format!("ref:{r}"),
            Self::Path(_, p) => format!("path:{}", p.display()),
            Self::Sub {
                sub, orig_version, ..
            } => format!("sub-{}:{}", sub, orig_version),
            Self::System(_) => "system".to_string(),
        }
    }

    pub fn resolve(
        &self,
        plugin: &dyn Plugin,
        opts: ToolVersionOptions,
        latest_versions: bool,
    ) -> Result<ToolVersion> {
        ToolVersion::resolve(plugin, self.clone(), opts, latest_versions)
    }
}

impl Display for ToolVersionRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", self.plugin_name(), self.version())
    }
}
