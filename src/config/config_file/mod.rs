use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::path::Path;

use color_eyre::eyre::{eyre, Result};
use indexmap::IndexMap;

use rtxrc::RTXFile;
use tool_versions::ToolVersions;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgVersion};
use crate::config::settings::SettingsBuilder;
use crate::config::{AliasMap, Config};
use crate::env;

use crate::file::display_path;
use crate::output::Output;
use crate::plugins::PluginName;

use crate::toolset::{Toolset, ToolsetBuilder};

pub mod legacy_version;
pub mod rtx_toml;
pub mod rtxrc;
pub mod tool_versions;

#[derive(Debug, PartialEq)]
pub enum ConfigFileType {
    RtxRc,
    RtxToml,
    ToolVersions,
    LegacyVersion,
}

pub trait ConfigFile: Debug + Display + Send + Sync {
    fn get_type(&self) -> ConfigFileType;
    fn get_path(&self) -> &Path;
    fn plugins(&self) -> IndexMap<PluginName, Vec<String>>;
    fn env(&self) -> HashMap<String, String>;
    fn remove_plugin(&mut self, plugin_name: &PluginName);
    fn add_version(&mut self, plugin_name: &PluginName, version: &str);
    fn replace_versions(&mut self, plugin_name: &PluginName, versions: &[String]);
    fn save(&self) -> Result<()>;
    fn dump(&self) -> String;
    fn to_toolset(&self) -> &Toolset;
    fn settings(&self) -> SettingsBuilder;
    fn aliases(&self) -> AliasMap;
}

impl dyn ConfigFile {
    pub fn add_runtimes(
        &mut self,
        config: &Config,
        runtimes: &[RuntimeArg],
        pin: bool,
    ) -> Result<()> {
        let mut runtime_map: HashMap<PluginName, Vec<String>> = HashMap::new();
        let ts = ToolsetBuilder::new()
            .with_install_missing()
            .with_args(runtimes)
            .build(config);

        for runtime in runtimes {
            if let Some(rtv) = ts.resolve_runtime_arg(runtime) {
                runtime_map
                    .entry(rtv.plugin.name.clone())
                    .or_default()
                    .push(if pin {
                        rtv.version.to_string()
                    } else {
                        match &runtime.version {
                            RuntimeArgVersion::Version(ref v) => v.to_string(),
                            RuntimeArgVersion::Path(p) => format!("path:{}", p.display()),
                            RuntimeArgVersion::Ref(r) => format!("ref:{r}"),
                            RuntimeArgVersion::Prefix(p) => format!("prefix:{p}"),
                            _ => "latest".to_string(),
                        }
                    });
            }
        }
        for (plugin, versions) in runtime_map {
            self.replace_versions(&plugin, &versions);
        }

        Ok(())
    }

    /// this is for `rtx local|global RUNTIME` which will display the version instead of setting it
    /// it's only valid to use a single runtime in this case
    /// returns "true" if the runtime was displayed which means the CLI should exit
    pub fn display_runtime(&self, out: &mut Output, runtimes: &[RuntimeArg]) -> Result<bool> {
        // in this situation we just print the current version in the config file
        if runtimes.len() == 1 && runtimes[0].version == RuntimeArgVersion::None {
            let plugin = &runtimes[0].plugin;
            let plugins = self.plugins();
            let version = plugins.get(plugin).ok_or_else(|| {
                eyre!(
                    "no version set for {} in {}",
                    plugin.to_string(),
                    display_path(self.get_path())
                )
            })?;
            rtxprintln!(out, "{}", version.join(" "));
            return Ok(true);
        }
        // check for something like `rtx local nodejs python@latest` which is invalid
        if runtimes
            .iter()
            .any(|r| r.version == RuntimeArgVersion::None)
        {
            return Err(eyre!("invalid input, specify a version for each runtime. Or just specify one runtime to print the current version"));
        }
        Ok(false)
    }
}

pub fn init(path: &Path) -> Box<dyn ConfigFile> {
    if path.ends_with(".rtxrc") || path.ends_with(".rtxrc.toml") {
        return Box::new(RTXFile::init(path));
    } else if path.ends_with(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str()) {
        return Box::new(ToolVersions::init(path));
    }

    panic!("Unknown config file type: {}", path.display());
}

pub fn parse(path: &Path) -> Result<Box<dyn ConfigFile>> {
    match detect_config_file_type(path) {
        Some(ConfigFileType::RtxToml) => Ok(Box::new(rtx_toml::RtxToml::from_file(path)?)),
        Some(ConfigFileType::RtxRc) => Ok(Box::new(RTXFile::from_file(path)?)),
        Some(ConfigFileType::ToolVersions) => Ok(Box::new(ToolVersions::from_file(path)?)),
        #[allow(clippy::box_default)]
        _ => Ok(Box::new(RTXFile::default())),
    }
}

fn detect_config_file_type(path: &Path) -> Option<ConfigFileType> {
    match path.file_name().unwrap().to_str().unwrap() {
        ".rtxrc" | ".rtxrc.toml" | "config.toml" => Some(ConfigFileType::RtxRc),
        f if env::RTX_DEFAULT_CONFIG_FILENAME.as_str() == f => Some(ConfigFileType::RtxToml),
        f if env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str() == f => {
            Some(ConfigFileType::ToolVersions)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_config_file_type() {
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.rtxrc")),
            Some(ConfigFileType::RtxRc)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.rtxrc.toml")),
            Some(ConfigFileType::RtxRc)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.test-tool-versions")),
            Some(ConfigFileType::ToolVersions)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.tool-versions.toml")),
            None
        );
    }
}
