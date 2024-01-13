use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use eyre::Result;

use crate::cli::args::ForgeArg;
use crate::forge::Forge;
use crate::toolset::{ToolVersion, ToolVersionOptions};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ToolVersionRequest {
    Version(ForgeArg, String),
    Prefix(ForgeArg, String),
    Ref(ForgeArg, String),
    Path(ForgeArg, PathBuf),
    Sub {
        forge: ForgeArg,
        sub: String,
        orig_version: String,
    },
    System(ForgeArg),
}

impl ToolVersionRequest {
    pub fn new(forge: ForgeArg, s: &str) -> Self {
        let s = match s.split_once('-') {
            Some(("ref", r)) => format!("ref:{}", r),
            _ => s.to_string(),
        };
        match s.split_once(':') {
            Some(("ref", r)) => Self::Ref(forge, r.to_string()),
            Some(("prefix", p)) => Self::Prefix(forge, p.to_string()),
            Some(("path", p)) => Self::Path(forge, PathBuf::from(p)),
            Some((p, v)) if p.starts_with("sub-") => Self::Sub {
                forge,
                sub: p.split_once('-').unwrap().1.to_string(),
                orig_version: v.to_string(),
            },
            None => {
                if s == "system" {
                    Self::System(forge)
                } else {
                    Self::Version(forge, s.to_string())
                }
            }
            _ => panic!("invalid tool version request: {s}"),
        }
    }

    pub fn forge(&self) -> &ForgeArg {
        match self {
            Self::Version(f, _)
            | Self::Prefix(f, _)
            | Self::Ref(f, _)
            | Self::Path(f, _)
            | Self::Sub { forge: f, .. }
            | Self::System(f) => f,
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
        plugin: &dyn Forge,
        opts: ToolVersionOptions,
        latest_versions: bool,
    ) -> Result<ToolVersion> {
        ToolVersion::resolve(plugin, self.clone(), opts, latest_versions)
    }
}

impl Display for ToolVersionRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.forge(), self.version())
    }
}
