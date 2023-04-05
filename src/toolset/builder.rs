use color_eyre::eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;

use crate::cli::args::runtime::RuntimeArg;
use crate::config::Config;
use crate::env;
use crate::toolset::{ToolSource, ToolVersionRequest, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;

#[derive(Debug, Default)]
pub struct ToolsetBuilder {
    args: Vec<RuntimeArg>,
    install_missing: bool,
    latest_versions: bool,
}

impl ToolsetBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_args(mut self, args: &[RuntimeArg]) -> Self {
        self.args = args.to_vec();
        self
    }

    pub fn with_install_missing(mut self) -> Self {
        self.install_missing = true;
        self
    }

    pub fn with_latest_versions(mut self) -> Self {
        self.latest_versions = true;
        self
    }

    pub fn build(self, config: &mut Config) -> Result<Toolset> {
        let mut toolset = Toolset {
            latest_versions: self.latest_versions,
            ..Default::default()
        };
        load_config_files(config, &mut toolset);
        load_runtime_env(&mut toolset, env::vars().collect());
        load_runtime_args(&mut toolset, &self.args);
        toolset.resolve(config);

        if self.install_missing {
            let mpr = MultiProgressReport::new(config.settings.verbose);
            toolset.install_missing(config, mpr)?;
        }

        debug!("{}", toolset);
        Ok(toolset)
    }
}

fn load_config_files(config: &Config, ts: &mut Toolset) {
    for cf in config.config_files.values().rev() {
        ts.merge(cf.to_toolset());
    }
}

fn load_runtime_env(ts: &mut Toolset, env: IndexMap<String, String>) {
    for (k, v) in env {
        if k.starts_with("RTX_") && k.ends_with("_VERSION") {
            let plugin_name = k[4..k.len() - 8].to_lowercase();
            if plugin_name == "install" {
                // ignore RTX_INSTALL_VERSION
                continue;
            }
            let source = ToolSource::Environment(k, v.clone());
            let mut env_ts = Toolset::new(source);
            let tvr = ToolVersionRequest::new(plugin_name, &v);
            env_ts.add_version(tvr, Default::default());
            ts.merge(&env_ts);
        }
    }
}

fn load_runtime_args(ts: &mut Toolset, args: &[RuntimeArg]) {
    for (_, args) in args.iter().into_group_map_by(|arg| arg.plugin.clone()) {
        let mut arg_ts = Toolset::new(ToolSource::Argument);
        for arg in args {
            if let Some(tvr) = &arg.tvr {
                arg_ts.add_version(tvr.clone(), Default::default());
            }
        }
        ts.merge(&arg_ts);
    }
}
