use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use versions::{Chunk, Version};

use crate::config::Config;
use crate::dirs;
use crate::hash::hash_to_str;
use crate::plugins::PluginName;
use crate::tool::Tool;
use crate::toolset::{ToolVersionOptions, ToolVersionRequest};

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
            ToolVersionRequest::Sub {
                sub, orig_version, ..
            } => Self::resolve_sub(
                config,
                tool,
                request,
                latest_versions,
                &sub,
                &orig_version,
                opts,
            )?,
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
    pub fn latest_version(&self, config: &Config, tool: &Tool) -> Result<String> {
        let tv = self
            .request
            .resolve(config, tool, self.opts.clone(), true)?;
        Ok(tv.version)
    }
    fn tv_pathname(&self) -> String {
        match &self.request {
            ToolVersionRequest::Version(_, _) => self.version.to_string(),
            ToolVersionRequest::Prefix(_, _) => self.version.to_string(),
            ToolVersionRequest::Sub { .. } => self.version.to_string(),
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
            Some((part, v)) if part.starts_with("sub-") => {
                let sub = part.split_once('-').unwrap().1;
                return Self::resolve_sub(config, tool, request, latest_versions, sub, v, opts);
            }
            _ => (),
        }

        let build = |v| Ok(Self::new(tool, request.clone(), opts.clone(), v));

        if !tool.is_installed() {
            return build(v);
        }
        let existing = build(v.clone())?;
        if tool.is_version_installed(&existing) {
            // if the version is already installed, no need to fetch all the remote versions
            return Ok(existing);
        }

        if v == "latest" {
            if !latest_versions {
                if let Some(v) = tool.latest_installed_version(None)? {
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
            if let Some(v) = matches.last() {
                return build(v.clone());
            }
        }
        let matches = tool.list_versions_matching(&config.settings, &v)?;
        if matches.contains(&v) {
            return build(v);
        }
        // TODO: remove for calver release
        if let Some((v, sub)) = v.split_once("!-") {
            return Self::resolve_sub(config, tool, request.clone(), latest_versions, sub, v, opts);
        }
        Self::resolve_prefix(config, tool, request, &v, opts)
    }

    /// resolve a version like `sub-1:12.0.0` which becomes `11.0.0`, `sub-0.1:12.1.0` becomes `12.0.0`
    fn resolve_sub(
        config: &Config,
        tool: &Tool,
        request: ToolVersionRequest,
        latest_versions: bool,
        sub: &str,
        v: &str,
        opts: ToolVersionOptions,
    ) -> Result<Self> {
        let v = match v {
            "latest" => tool.latest_version(&config.settings, None)?.unwrap(),
            _ => config.resolve_alias(&tool.name, v)?,
        };
        let v = version_sub(&v, sub);
        Self::resolve_version(config, tool, request, latest_versions, &v, opts)
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
