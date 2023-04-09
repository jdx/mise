use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use versions::{Chunk, Version};

use crate::config::Config;
use crate::dirs;
use crate::hash::hash_to_str;
use crate::plugins::PluginName;
use crate::runtime_symlinks::is_runtime_symlink;
use crate::tool::Tool;
use crate::toolset::{ToolVersionOptions, ToolVersionRequest};

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersion {
    pub request: ToolVersionRequest,
    pub plugin_name: PluginName,
    pub version: String,
    pub opts: ToolVersionOptions,
}

impl ToolVersion {
    pub fn new(
        tool: &Tool,
        request: ToolVersionRequest,
        opts: ToolVersionOptions,
        version: String,
    ) -> Self {
        ToolVersion {
            plugin_name: tool.name.to_string(),
            version,
            request,
            opts,
        }
    }

    pub fn resolve(
        config: &Config,
        tool: &Tool,
        request: ToolVersionRequest,
        opts: ToolVersionOptions,
        latest_versions: bool,
    ) -> Result<Self> {
        let tv = match request.clone() {
            ToolVersionRequest::Version(_, v) => {
                Self::resolve_version(config, tool, request, latest_versions, &v, opts)?
            }
            ToolVersionRequest::Prefix(_, prefix) => {
                Self::resolve_prefix(config, tool, request, &prefix, opts)?
            }
            _ => {
                let version = request.version();
                Self::new(tool, request, opts, version)
            }
        };
        Ok(tv)
    }

    pub fn install_path(&self) -> PathBuf {
        let pathname = match &self.request {
            ToolVersionRequest::Path(_, p) => p.to_string_lossy().to_string(),
            _ => self.tv_pathname(),
        };
        dirs::INSTALLS.join(&self.plugin_name).join(pathname)
    }
    pub fn cache_path(&self) -> PathBuf {
        dirs::CACHE.join(&self.plugin_name).join(self.tv_pathname())
    }
    pub fn download_path(&self) -> PathBuf {
        dirs::DOWNLOADS
            .join(&self.plugin_name)
            .join(self.tv_pathname())
    }
    fn tv_pathname(&self) -> String {
        match &self.request {
            ToolVersionRequest::Version(_, _) => self.version.to_string(),
            ToolVersionRequest::Prefix(_, _) => self.version.to_string(),
            ToolVersionRequest::Ref(_, r) => format!("ref-{}", r),
            ToolVersionRequest::Path(_, p) => format!("path-{}", hash_to_str(p)),
            ToolVersionRequest::System(_) => "system".to_string(),
        }
    }

    fn resolve_version(
        config: &Config,
        tool: &Tool,
        request: ToolVersionRequest,
        latest_versions: bool,
        v: &str,
        opts: ToolVersionOptions,
    ) -> Result<ToolVersion> {
        let v = config.resolve_alias(&tool.name, v)?;
        match v.split_once(':') {
            Some(("ref", r)) => {
                return Ok(Self::resolve_ref(tool, r.to_string(), opts));
            }
            Some(("path", p)) => {
                return Self::resolve_path(tool, PathBuf::from(p), opts);
            }
            Some(("prefix", p)) => {
                return Self::resolve_prefix(config, tool, request, p, opts);
            }
            _ => (),
        }

        let build = |v| Ok(Self::new(tool, request.clone(), opts.clone(), v));

        let existing_path = dirs::INSTALLS.join(&tool.name).join(&v);
        if existing_path.exists() && !is_runtime_symlink(&existing_path) {
            // if the version is already installed, no need to fetch all the remote versions
            return build(v);
        }
        if !tool.is_installed() {
            return build(v);
        }

        if v == "latest" {
            if !latest_versions {
                if let Some(v) = tool.latest_installed_version()? {
                    return build(v);
                }
            }
            if let Some(v) = tool.latest_version(&config.settings, None)? {
                return build(v);
            }
        }
        if !latest_versions {
            let matches = tool.list_installed_versions_matching(&v)?;
            if matches.contains(&v) {
                return build(v);
            }
        }
        let matches = tool.list_versions_matching(&config.settings, &v)?;
        if matches.contains(&v) {
            return build(v);
        }
        if v.contains("!-") {
            if let Some(tv) = Self::resolve_bang(config, tool, request.clone(), &v, &opts)? {
                return Ok(tv);
            }
        }
        Self::resolve_prefix(config, tool, request, &v, opts)
    }

    /// resolve a version like `12.0.0!-1` which becomes `11.0.0`, `12.1.0!-0.1` becomes `12.0.0`
    fn resolve_bang(
        config: &Config,
        tool: &Tool,
        request: ToolVersionRequest,
        v: &str,
        opts: &ToolVersionOptions,
    ) -> Result<Option<Self>> {
        let (wanted, minus) = v.split_once("!-").unwrap();
        let wanted = match wanted {
            "latest" => tool.latest_version(&config.settings, None)?.unwrap(),
            _ => config.resolve_alias(&tool.name, wanted)?,
        };
        let wanted = version_sub(&wanted, minus);
        let tv = tool
            .latest_version(&config.settings, Some(wanted))?
            .map(|v| Self::new(tool, request, opts.clone(), v));
        Ok(tv)
    }

    fn resolve_prefix(
        config: &Config,
        tool: &Tool,
        request: ToolVersionRequest,
        prefix: &str,
        opts: ToolVersionOptions,
    ) -> Result<Self> {
        let matches = tool.list_versions_matching(&config.settings, prefix)?;
        let v = match matches.last() {
            Some(v) => v,
            None => prefix,
            // None => Err(VersionNotFound(plugin.name.clone(), prefix.to_string()))?,
        };
        Ok(Self::new(tool, request, opts, v.to_string()))
    }

    fn resolve_ref(tool: &Tool, r: String, opts: ToolVersionOptions) -> Self {
        let request = ToolVersionRequest::Ref(tool.name.clone(), r);
        let version = request.version();
        Self::new(tool, request, opts, version)
    }

    fn resolve_path(tool: &Tool, path: PathBuf, opts: ToolVersionOptions) -> Result<ToolVersion> {
        let path = fs::canonicalize(path)?;
        let request = ToolVersionRequest::Path(tool.name.clone(), path);
        let version = request.version();
        Ok(Self::new(tool, request, opts, version))
    }
}

impl Display for ToolVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.plugin_name, &self.version)
    }
}

/// subtracts sub from orig and removes suffix
/// e.g. version_sub("18.2.3", "2") -> "16"
/// e.g. version_sub("18.2.3", "0.1") -> "18.1"
fn version_sub(orig: &str, sub: &str) -> String {
    let mut orig = Version::new(orig).unwrap();
    let sub = Version::new(sub).unwrap();
    while orig.chunks.0.len() > sub.chunks.0.len() {
        orig.chunks.0.pop();
    }
    for (i, orig_chunk) in orig.clone().chunks.0.iter().enumerate() {
        let m = sub.nth(i).unwrap();
        orig.chunks.0[i] = Chunk::Numeric(orig_chunk.single_digit().unwrap() - m);
    }
    orig.to_string()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use super::*;

    #[test]
    fn test_version_sub() {
        assert_str_eq!(version_sub("18.2.3", "2"), "16");
        assert_str_eq!(version_sub("18.2.3", "0.1"), "18.1");
    }
}
