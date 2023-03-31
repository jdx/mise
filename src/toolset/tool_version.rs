use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use indexmap::IndexMap;
use versions::{Chunk, Version};

use crate::config::Config;
use crate::dirs;

use crate::plugins::{Plugin, Plugins};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::runtimes::RuntimeVersion;
use crate::ui::progress_report::ProgressReport;

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersion {
    pub plugin_name: String,
    pub r#type: ToolVersionType,
    pub rtv: Option<RuntimeVersion>,
    pub options: IndexMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolVersionType {
    Version(String),
    Prefix(String),
    Ref(String),
    Path(PathBuf),
    System,
}

impl ToolVersion {
    pub fn new(plugin_name: String, r#type: ToolVersionType) -> Self {
        Self {
            plugin_name,
            r#type,
            rtv: None,
            options: Default::default(),
        }
    }

    pub fn resolve(
        &mut self,
        config: &Config,
        plugin: Arc<Plugins>,
        latest_versions: bool,
    ) -> Result<()> {
        if self.rtv.is_none() {
            self.rtv = Some(match &self.r#type {
                ToolVersionType::Version(v) => {
                    self.resolve_version(config, plugin, v, latest_versions)?
                }
                ToolVersionType::Prefix(v) => self.resolve_prefix(config, plugin, v)?,
                ToolVersionType::Ref(r) => self.resolve_ref(config, plugin, r)?,
                ToolVersionType::Path(path) => self.resolve_path(config, plugin, path)?,
                ToolVersionType::System => self.resolve_system(config, plugin),
            });
        }
        Ok(())
    }

    fn resolve_version(
        &self,
        config: &Config,
        plugin: Arc<Plugins>,
        v: &str,
        latest_versions: bool,
    ) -> Result<RuntimeVersion> {
        let v = config.resolve_alias(plugin.name(), v)?;
        match v.split_once(':') {
            Some(("ref", r)) => {
                return self.resolve_ref(config, plugin, r);
            }
            Some(("path", p)) => {
                return self.resolve_path(config, plugin, &PathBuf::from(p));
            }
            Some(("prefix", p)) => {
                return self.resolve_prefix(config, plugin, p);
            }
            _ => (),
        }

        let existing_path = dirs::INSTALLS.join(plugin.name()).join(&v);
        if existing_path.exists() && !is_runtime_symlink(&existing_path) {
            // if the version is already installed, no need to fetch all the remote versions
            return Ok(self.build_rtv(config, plugin, &v));
        }

        if v == "latest" {
            if !latest_versions {
                if let Some(v) = plugin.latest_installed_version()? {
                    return Ok(self.build_rtv(config, plugin, &v));
                }
            }
            if let Some(v) = plugin.latest_version(&config.settings, None)? {
                return Ok(self.build_rtv(config, plugin, &v));
            }
        }
        if !latest_versions {
            let matches = plugin.list_installed_versions_matching(&v)?;
            if matches.contains(&v) {
                return Ok(self.build_rtv(config, plugin, &v));
            }
        }
        let matches = plugin.list_versions_matching(&config.settings, &v)?;
        if matches.contains(&v) {
            return Ok(self.build_rtv(config, plugin, &v));
        }
        if v.contains("!-") {
            if let Some(rtv) = self.resolve_bang(config, plugin.clone(), &v)? {
                return Ok(rtv);
            }
        }
        self.resolve_prefix(config, plugin, &v)
    }

    /// resolve a version like `12.0.0!-1` which becomes `11.0.0`, `12.1.0!-0.1` becomes `12.0.0`
    fn resolve_bang(
        &self,
        config: &Config,
        plugin: Arc<Plugins>,
        v: &str,
    ) -> Result<Option<RuntimeVersion>> {
        let (wanted, minus) = v.split_once("!-").unwrap();
        let wanted = match wanted {
            "latest" => plugin.latest_version(&config.settings, None)?.unwrap(),
            _ => config.resolve_alias(plugin.name(), wanted)?,
        };
        let wanted = version_sub(&wanted, minus);
        match plugin.latest_version(&config.settings, Some(wanted))? {
            Some(v) => Ok(Some(self.build_rtv(config, plugin, &v))),
            None => Ok(None),
        }
    }

    fn resolve_prefix(
        &self,
        config: &Config,
        plugin: Arc<Plugins>,
        prefix: &str,
    ) -> Result<RuntimeVersion> {
        let matches = plugin.list_versions_matching(&config.settings, prefix)?;
        let v = match matches.last() {
            Some(v) => v,
            None => prefix,
            // None => Err(VersionNotFound(plugin.name.clone(), prefix.to_string()))?,
        };
        Ok(self.build_rtv(config, plugin, v))
    }

    fn resolve_ref(
        &self,
        config: &Config,
        plugin: Arc<Plugins>,
        r: &str,
    ) -> Result<RuntimeVersion> {
        Ok(self.build_rtv(config, plugin, &format!("ref-{}", r)))
    }

    fn resolve_path(
        &self,
        config: &Config,
        plugin: Arc<Plugins>,
        path: &PathBuf,
    ) -> Result<RuntimeVersion> {
        let path = fs::canonicalize(path)?;
        Ok(self.build_rtv(config, plugin, &path.display().to_string()))
    }

    pub fn resolve_system(&self, config: &Config, plugin: Arc<Plugins>) -> RuntimeVersion {
        self.build_rtv(config, plugin, "system")
    }

    pub fn is_missing(&self) -> bool {
        match self.rtv {
            Some(ref rtv) if rtv.version == "system" => false,
            Some(ref rtv) => !rtv.is_installed(),
            None => true,
        }
    }

    pub fn install(&self, config: &Config, pr: &mut ProgressReport, force: bool) -> Result<()> {
        match self.r#type {
            ToolVersionType::Version(_) | ToolVersionType::Prefix(_) | ToolVersionType::Ref(_) => {
                self.rtv.as_ref().unwrap().install(config, pr, force)
            }
            _ => Ok(()),
        }
    }

    fn build_rtv(&self, config: &Config, plugin: Arc<Plugins>, v: &str) -> RuntimeVersion {
        RuntimeVersion::new(config, plugin, v.to_string(), self.clone())
    }
}

impl Display for ToolVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.plugin_name, &self.r#type)
    }
}

impl Display for ToolVersionType {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            ToolVersionType::Version(v) => write!(f, "{v}"),
            ToolVersionType::Prefix(p) => write!(f, "prefix:{p}"),
            ToolVersionType::Ref(r) => write!(f, "ref:{r}"),
            ToolVersionType::Path(p) => write!(f, "path:{}", p.display()),
            ToolVersionType::System => write!(f, "system"),
        }
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
    fn test_tool_version_display() {
        let foo = "foo".to_string();
        let tv = ToolVersion::new(foo.clone(), ToolVersionType::Version("1.2.3".to_string()));
        assert_str_eq!(tv.to_string(), "foo@1.2.3");
        let tv = ToolVersion::new(foo.clone(), ToolVersionType::Prefix("1.2.3".to_string()));
        assert_str_eq!(tv.to_string(), "foo@prefix:1.2.3");
        let tv = ToolVersion::new(foo.clone(), ToolVersionType::Ref("master".to_string()));
        assert_str_eq!(tv.to_string(), "foo@ref:master");
        let tv = ToolVersion::new(foo.clone(), ToolVersionType::Path(PathBuf::from("~")));
        assert_str_eq!(tv.to_string(), "foo@path:~");
        let tv = ToolVersion::new(foo, ToolVersionType::System);
        assert_str_eq!(tv.to_string(), "foo@system");
    }

    #[test]
    fn test_version_sub() {
        assert_str_eq!(version_sub("18.2.3", "2"), "16");
        assert_str_eq!(version_sub("18.2.3", "0.1"), "18.1");
    }
}
