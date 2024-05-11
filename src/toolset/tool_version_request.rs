use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use eyre::Result;

use crate::cli::args::ForgeArg;
use crate::forge::Forge;
use crate::toolset::{ToolVersion, ToolVersionOptions};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ToolVersionRequest {
    Version {
        forge: ForgeArg,
        version: String,
        options: ToolVersionOptions,
    },
    Prefix {
        forge: ForgeArg,
        prefix: String,
        options: ToolVersionOptions,
    },
    Ref {
        forge: ForgeArg,
        ref_: String,
        options: ToolVersionOptions,
    },
    Sub {
        forge: ForgeArg,
        sub: String,
        orig_version: String,
    },
    Path(ForgeArg, PathBuf),
    System(ForgeArg),
}

impl ToolVersionRequest {
    pub fn new(forge: ForgeArg, s: &str) -> Self {
        let s = match s.split_once('-') {
            Some(("ref", r)) => format!("ref:{}", r),
            _ => s.to_string(),
        };
        match s.split_once(':') {
            Some(("ref", r)) => Self::Ref {
                forge,
                ref_: r.to_string(),
                options: Default::default(),
            },
            Some(("prefix", p)) => Self::Prefix {
                forge,
                prefix: p.to_string(),
                options: Default::default(),
            },
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
                    Self::Version {
                        forge,
                        version: s,
                        options: Default::default(),
                    }
                }
            }
            _ => panic!("invalid tool version request: {s}"),
        }
    }
    pub fn new_opts(forge: ForgeArg, s: &str, options: ToolVersionOptions) -> Self {
        let mut tvr = Self::new(forge, s);
        match &mut tvr {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. } => *o = options,
            _ => Default::default(),
        }
        tvr
    }
    pub fn forge(&self) -> &ForgeArg {
        match self {
            Self::Version { forge: f, .. }
            | Self::Prefix { forge: f, .. }
            | Self::Ref { forge: f, .. }
            | Self::Path(f, _)
            | Self::Sub { forge: f, .. }
            | Self::System(f) => f,
        }
    }
    pub fn version(&self) -> String {
        match self {
            Self::Version { version: v, .. } => v.clone(),
            Self::Prefix { prefix: p, .. } => format!("prefix:{p}"),
            Self::Ref { ref_: r, .. } => format!("ref:{r}"),
            Self::Path(_, p) => format!("path:{}", p.display()),
            Self::Sub {
                sub, orig_version, ..
            } => format!("sub-{}:{}", sub, orig_version),
            Self::System(_) => "system".to_string(),
        }
    }

    pub fn options(&self) -> ToolVersionOptions {
        match self {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. } => o.clone(),
            _ => Default::default(),
        }
    }

    pub fn resolve(&self, plugin: &dyn Forge, latest_versions: bool) -> Result<ToolVersion> {
        ToolVersion::resolve(plugin, self.clone(), latest_versions)
    }
}

impl Display for ToolVersionRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.forge(), self.version())
    }
}
