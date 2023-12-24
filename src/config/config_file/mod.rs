use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};
use confique::Partial;

use tool_versions::ToolVersions;

use crate::cli::args::tool::ToolArg;
use crate::config::config_file::rtx_toml::RtxToml;
use crate::config::settings::SettingsPartial;
use crate::config::{global_config_files, system_config_files, AliasMap, Config, Settings};
use crate::file::{display_path, replace_path};
use crate::hash::hash_to_str;

use crate::plugins::PluginName;
use crate::task::Task;
use crate::toolset::{ToolVersionList, Toolset};
use crate::{dirs, env, file};

pub mod legacy_version;
pub mod rtx_toml;
pub mod toml;
pub mod tool_versions;

#[derive(Debug, PartialEq)]
pub enum ConfigFileType {
    RtxToml,
    ToolVersions,
    LegacyVersion,
}

pub trait ConfigFile: Debug + Send + Sync {
    fn get_type(&self) -> ConfigFileType;
    fn get_path(&self) -> &Path;
    /// gets the project directory for the config
    /// if it's a global/system config, returns None
    /// files like ~/src/foo/.rtx/config.toml will return ~/src/foo
    /// and ~/src/foo/.rtx.config.toml will return None
    fn project_root(&self) -> Option<&Path> {
        let p = self.get_path();
        if env::RTX_CONFIG_FILE.as_ref().is_some_and(|f| f == p) {
            return None;
        }
        match p.parent() {
            Some(dir) => match dir {
                dir if dir.starts_with(*dirs::CONFIG) => None,
                dir if dir.starts_with(*dirs::SYSTEM) => None,
                dir if dir == *dirs::HOME => None,
                dir => Some(dir),
            },
            None => None,
        }
    }
    fn plugins(&self) -> HashMap<PluginName, String> {
        Default::default()
    }
    fn env(&self) -> HashMap<String, String> {
        Default::default()
    }
    fn env_remove(&self) -> Vec<String> {
        Default::default()
    }
    fn env_path(&self) -> Vec<PathBuf> {
        Default::default()
    }
    fn tasks(&self) -> Vec<&Task> {
        Default::default()
    }
    fn remove_plugin(&mut self, _plugin_name: &PluginName);
    fn replace_versions(&mut self, plugin_name: &PluginName, versions: &[String]);
    fn save(&self) -> Result<()>;
    fn dump(&self) -> String;
    fn to_toolset(&self) -> &Toolset;
    fn settings(&self) -> Result<SettingsPartial> {
        Ok(SettingsPartial::empty())
    }
    fn aliases(&self) -> AliasMap {
        Default::default()
    }
    fn watch_files(&self) -> Vec<PathBuf> {
        vec![self.get_path().to_path_buf()]
    }
    fn is_global(&self) -> bool {
        global_config_files()
            .iter()
            .chain(system_config_files().iter())
            .any(|p| p == self.get_path())
    }
}

impl dyn ConfigFile {
    pub fn add_runtimes(&mut self, config: &Config, runtimes: &[ToolArg], pin: bool) -> Result<()> {
        // TODO: this has become a complete mess and could probably be greatly simplified
        let mut ts = self.to_toolset().to_owned();
        ts.resolve(config);
        let mut plugins_to_update = HashMap::new();
        for runtime in runtimes {
            if let Some(tv) = &runtime.tvr {
                plugins_to_update
                    .entry(runtime.plugin.clone())
                    .or_insert_with(Vec::new)
                    .push(tv);
            }
        }
        for (plugin, versions) in &plugins_to_update {
            let mut tvl =
                ToolVersionList::new(plugin.to_string(), ts.source.as_ref().unwrap().clone());
            for tv in versions {
                tvl.requests.push(((*tv).clone(), Default::default()));
            }
            ts.versions.insert(plugin.clone(), tvl);
        }
        ts.resolve(config);
        for (plugin, versions) in plugins_to_update {
            let versions = versions
                .into_iter()
                .map(|tvr| {
                    if pin {
                        let plugin = config.get_or_create_plugin(&plugin);
                        let tv = tvr.resolve(plugin.as_ref(), Default::default(), false)?;
                        Ok(tv.version)
                    } else {
                        Ok(tvr.version())
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            self.replace_versions(&plugin, &versions);
        }

        Ok(())
    }

    /// this is for `rtx local|global TOOL` which will display the version instead of setting it
    /// it's only valid to use a single tool in this case
    /// returns "true" if the tool was displayed which means the CLI should exit
    pub fn display_runtime(&self, runtimes: &[ToolArg]) -> Result<bool> {
        // in this situation we just print the current version in the config file
        if runtimes.len() == 1 && runtimes[0].tvr.is_none() {
            let plugin = &runtimes[0].plugin;
            let tvl = self
                .to_toolset()
                .versions
                .get(plugin)
                .ok_or_else(|| {
                    eyre!(
                        "no version set for {} in {}",
                        plugin.to_string(),
                        display_path(self.get_path())
                    )
                })?
                .requests
                .iter()
                .map(|(tvr, _)| tvr.version())
                .collect::<Vec<_>>();
            rtxprintln!("{}", tvl.join(" "));
            return Ok(true);
        }
        // check for something like `rtx local node python@latest` which is invalid
        if runtimes.iter().any(|r| r.tvr.is_none()) {
            return Err(eyre!("invalid input, specify a version for each tool. Or just specify one tool to print the current version"));
        }
        Ok(false)
    }
}

fn init(path: &Path) -> Box<dyn ConfigFile> {
    match detect_config_file_type(path) {
        Some(ConfigFileType::RtxToml) => Box::new(RtxToml::init(path)),
        Some(ConfigFileType::ToolVersions) => Box::new(ToolVersions::init(path)),
        _ => panic!("Unknown config file type: {}", path.display()),
    }
}

pub fn parse_or_init(path: &Path) -> Result<Box<dyn ConfigFile>> {
    let cf = match path.exists() {
        true => parse(path)?,
        false => init(path),
    };
    Ok(cf)
}

pub fn parse(path: &Path) -> Result<Box<dyn ConfigFile>> {
    match detect_config_file_type(path) {
        Some(ConfigFileType::RtxToml) => Ok(Box::new(RtxToml::from_file(path)?)),
        Some(ConfigFileType::ToolVersions) => Ok(Box::new(ToolVersions::from_file(path)?)),
        #[allow(clippy::box_default)]
        _ => Ok(Box::new(RtxToml::default())),
    }
}

pub fn is_trusted(path: &Path) -> bool {
    let settings = Settings::get();
    if settings
        .trusted_config_paths
        .iter()
        .any(|p| path.starts_with(replace_path(p)))
    {
        return true;
    }
    match path.canonicalize() {
        Ok(path) => trust_path(&path).exists(),
        Err(_) => false,
    }
}

pub fn trust(path: &Path) -> Result<()> {
    let path = path.canonicalize()?;
    let hashed_path = trust_path(&path);
    if !hashed_path.exists() {
        file::create_dir_all(hashed_path.parent().unwrap())?;
        file::make_symlink(&path, &hashed_path)?;
    }
    Ok(())
}

pub fn untrust(path: &Path) -> Result<()> {
    let path = path.canonicalize()?;
    let hashed_path = trust_path(&path);
    if hashed_path.exists() {
        file::remove_file(hashed_path)?;
    }
    Ok(())
}

fn trust_path(path: &Path) -> PathBuf {
    dirs::TRUSTED_CONFIGS.join(hash_to_str(&path))
}

fn detect_config_file_type(path: &Path) -> Option<ConfigFileType> {
    match path.file_name().unwrap().to_str().unwrap() {
        f if f.ends_with(".toml") => Some(ConfigFileType::RtxToml),
        f if env::RTX_DEFAULT_CONFIG_FILENAME.as_str() == f => Some(ConfigFileType::RtxToml),
        f if env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str() == f => {
            Some(ConfigFileType::ToolVersions)
        }
        _ => None,
    }
}

impl Display for dyn ConfigFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let toolset = self.to_toolset().to_string();
        write!(f, "{}: {toolset}", &display_path(self.get_path()))
    }
}

impl PartialEq for dyn ConfigFile {
    fn eq(&self, other: &Self) -> bool {
        self.get_path() == other.get_path()
    }
}
impl Eq for dyn ConfigFile {}
impl Hash for dyn ConfigFile {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.get_path().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_config_file_type() {
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.test-tool-versions")),
            Some(ConfigFileType::ToolVersions)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.tool-versions.toml")),
            Some(ConfigFileType::RtxToml)
        );
    }
}
