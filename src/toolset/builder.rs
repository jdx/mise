use std::collections::{BTreeMap, HashSet};

use eyre::Result;
use itertools::Itertools;

use crate::cli::args::{ForgeArg, ToolArg};
use crate::config::{Config, Settings};
use crate::toolset::{ToolSource, ToolVersionRequest, Toolset};
use crate::{config, env};

#[derive(Debug, Default)]
pub struct ToolsetBuilder {
    args: Vec<ToolArg>,
    global_only: bool,
    default_to_latest: bool,
    tool_filter: Option<HashSet<ForgeArg>>,
    installed_only: bool,
}

impl ToolsetBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_args(mut self, args: &[ToolArg]) -> Self {
        self.args = args.to_vec();
        self
    }

    pub fn with_global_only(mut self, global_only: bool) -> Self {
        self.global_only = global_only;
        self
    }

    pub fn with_default_to_latest(mut self, default_to_latest: bool) -> Self {
        self.default_to_latest = default_to_latest;
        self
    }

    pub fn with_tool_filter(mut self, tool_filter: HashSet<ForgeArg>) -> Self {
        self.tool_filter = Some(tool_filter);
        self
    }

    pub fn with_installed_only(mut self) -> Self {
        self.installed_only = true;
        self
    }

    pub fn build(self, config: &Config) -> Result<Toolset> {
        let settings = Settings::try_get()?;
        let mut toolset = Toolset {
            disable_tools: settings.disable_tools.iter().map(|s| s.into()).collect(),
            tool_filter: self.tool_filter.clone(),
            installed_only: self.installed_only,
            ..Default::default()
        };
        self.load_config_files(config, &mut toolset)?;
        self.load_runtime_env(&mut toolset, env::vars().collect());
        self.load_runtime_args(&mut toolset);
        toolset.resolve();

        debug!("Toolset: {}", toolset);
        Ok(toolset)
    }

    fn load_config_files(&self, config: &Config, ts: &mut Toolset) -> eyre::Result<()> {
        for cf in config.config_files.values().rev() {
            if self.global_only && !config::is_global_config(cf.get_path()) {
                return Ok(());
            }
            ts.merge(cf.to_toolset()?);
        }
        Ok(())
    }

    fn load_runtime_env(&self, ts: &mut Toolset, env: BTreeMap<String, String>) {
        if self.global_only {
            return;
        }
        for (k, v) in env {
            if k.starts_with("MISE_") && k.ends_with("_VERSION") && k != "MISE_VERSION" {
                let plugin_name = k
                    .trim_start_matches("MISE_")
                    .trim_end_matches("_VERSION")
                    .to_lowercase();
                if plugin_name == "install" {
                    // ignore MISE_INSTALL_VERSION
                    continue;
                }
                let fa: ForgeArg = plugin_name.as_str().into();
                let source = ToolSource::Environment(k, v.clone());
                let mut env_ts = Toolset::new(source);
                for v in v.split_whitespace() {
                    let tvr = ToolVersionRequest::new(fa.clone(), v);
                    env_ts.add_version(tvr);
                }
                ts.merge(env_ts);
            }
        }
    }

    fn load_runtime_args(&self, ts: &mut Toolset) {
        if self.global_only {
            return;
        }
        for (_, args) in self.args.iter().into_group_map_by(|arg| arg.forge.clone()) {
            let mut arg_ts = Toolset::new(ToolSource::Argument);
            for arg in args {
                if let Some(tvr) = &arg.tvr {
                    arg_ts.add_version(tvr.clone());
                } else if self.default_to_latest {
                    // this logic is required for `mise x` because with that specific command mise
                    // should default to installing the "latest" version if no version is specified
                    // in .mise.toml

                    // determine if we already have some active version in config
                    let set_as_latest = !ts
                        .list_current_requests()
                        .iter()
                        .any(|tvr| tvr.forge() == &arg.forge);

                    if set_as_latest {
                        // no active version, so use "latest"
                        arg_ts.add_version(ToolVersionRequest::new(arg.forge.clone(), "latest"));
                    }
                }
            }
            ts.merge(arg_ts);
        }
    }
}
