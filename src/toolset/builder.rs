use std::collections::BTreeMap;

use eyre::Result;
use itertools::Itertools;

use crate::cli::args::tool::ToolArg;
use crate::config::{Config, Settings};
use crate::env;
use crate::toolset::{ToolSource, ToolVersionRequest, Toolset};

#[derive(Debug, Default)]
pub struct ToolsetBuilder {
    args: Vec<ToolArg>,
    global_only: bool,
    tool_filter: Option<Vec<String>>,
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

    pub fn with_tools(mut self, tools: &[&str]) -> Self {
        self.tool_filter = Some(tools.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn build(self, config: &Config) -> Result<Toolset> {
        let settings = Settings::try_get()?;
        let mut toolset = Toolset {
            disable_tools: settings.disable_tools.clone(),
            ..Default::default()
        };
        self.load_config_files(config, &mut toolset);
        self.load_runtime_env(&mut toolset, env::vars().collect());
        self.load_runtime_args(&mut toolset);
        if let Some(tools) = self.tool_filter {
            toolset.versions.retain(|p, _| tools.contains(p));
        }
        toolset.resolve(config);

        debug!("{}", toolset);
        Ok(toolset)
    }

    fn load_config_files(&self, config: &Config, ts: &mut Toolset) {
        for cf in config.config_files.values().rev() {
            if self.global_only && !cf.is_global() {
                return;
            }
            ts.merge(cf.to_toolset());
        }
    }

    fn load_runtime_env(&self, ts: &mut Toolset, env: BTreeMap<String, String>) {
        if self.global_only {
            return;
        }
        for (k, v) in env {
            if k.starts_with("RTX_") && k.ends_with("_VERSION") && k != "RTX_VERSION" {
                let plugin_name = k[4..k.len() - 8].to_lowercase();
                if plugin_name == "install" {
                    // ignore RTX_INSTALL_VERSION
                    continue;
                }
                let source = ToolSource::Environment(k, v.clone());
                let mut env_ts = Toolset::new(source);
                for v in v.split_whitespace() {
                    let tvr = ToolVersionRequest::new(plugin_name.clone(), v);
                    env_ts.add_version(tvr, Default::default());
                }
                ts.merge(&env_ts);
            }
        }
    }

    fn load_runtime_args(&self, ts: &mut Toolset) {
        if self.global_only {
            return;
        }
        for (_, args) in self.args.iter().into_group_map_by(|arg| arg.plugin.clone()) {
            let mut arg_ts = Toolset::new(ToolSource::Argument);
            for arg in args {
                if let Some(tvr) = &arg.tvr {
                    arg_ts.add_version(tvr.clone(), Default::default());
                }
            }
            ts.merge(&arg_ts);
        }
    }
}
