use color_eyre::eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use rayon::prelude::*;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgVersion};
use crate::config::config_file::legacy_version::LegacyVersionFile;
use crate::config::config_file::ConfigFile;
use crate::config::{config_file, Config};

use crate::toolset::tool_version::ToolVersionType;
use crate::toolset::{ToolSource, ToolVersion, Toolset};
use crate::{env, file};

#[derive(Debug)]
pub struct ToolsetBuilder {
    args: Vec<RuntimeArg>,
    install_missing: bool,
}

impl ToolsetBuilder {
    pub fn new() -> Self {
        Self {
            args: Vec::new(),
            install_missing: false,
        }
    }

    pub fn with_args(mut self, args: &[RuntimeArg]) -> Self {
        self.args = args.to_vec();
        self
    }

    pub fn with_install_missing(mut self) -> Self {
        self.install_missing = true;
        self
    }

    pub fn build(self, config: &Config) -> Toolset {
        let mut toolset = Toolset::default().with_plugins(config.plugins.clone());
        load_config_files(config, &mut toolset);
        load_runtime_env(&mut toolset, env::vars().collect());
        load_runtime_args(&mut toolset, &self.args);
        toolset.resolve(config);

        if self.install_missing {
            if let Err(e) = toolset.install_missing(config) {
                warn!("Error installing runtimes: {}", e);
            };
        }

        debug!("{}", toolset);
        toolset
    }
}

fn load_config_files(config: &Config, ts: &mut Toolset) {
    let toolsets: Vec<_> = config
        .config_files
        .par_iter()
        .rev()
        .filter_map(|path| {
            let filename = path.file_name().unwrap().to_string_lossy().to_string();
            let result: Result<Toolset> = match config.legacy_files.get(&filename) {
                Some(plugin) => LegacyVersionFile::parse(
                    &config.settings,
                    path.into(),
                    config.plugins.get(plugin).unwrap(),
                )
                .map(|cf| cf.to_toolset()),
                None => config_file::parse(path).map(|cf| cf.to_toolset()),
            };
            match result {
                Ok(ts) => Some(ts),
                Err(e) => {
                    warn!("error parsing config file: {}", e);
                    warn!("file: {}", file::display_path(path));
                    None
                }
            }
        })
        .collect();
    for toolset in toolsets {
        ts.merge(toolset);
    }
}

fn load_runtime_env(ts: &mut Toolset, env: IndexMap<String, String>) {
    for (k, v) in env {
        if k.starts_with("RTX_") && k.ends_with("_VERSION") {
            let plugin_name = k[4..k.len() - 8].to_lowercase();
            let source = ToolSource::Environment(k, v.clone());
            let mut env_ts = Toolset::new(source);
            let version = ToolVersion::new(plugin_name.clone(), ToolVersionType::Version(v));
            env_ts.add_version(plugin_name, version);
            ts.merge(env_ts);
        }
    }
}

fn load_runtime_args(ts: &mut Toolset, args: &[RuntimeArg]) {
    for (plugin_name, args) in args.iter().into_group_map_by(|arg| arg.plugin.clone()) {
        let mut arg_ts = Toolset::new(ToolSource::Argument);
        for arg in args {
            match arg.version {
                RuntimeArgVersion::Version(ref v) => {
                    let version =
                        ToolVersion::new(plugin_name.clone(), ToolVersionType::Version(v.clone()));
                    arg_ts.add_version(plugin_name.clone(), version);
                }
                // I believe this will do nothing since it would just default to the `.tool-versions` version
                // RuntimeArgVersion::None => {
                //     arg_ts.add_version(plugin_name.clone(), ToolVersion::None);
                // },
                _ => {
                    trace!("ignoring: {:?}", arg);
                }
            }
        }
        ts.merge(arg_ts);
    }
}
