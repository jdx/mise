use std::sync::Arc;

use eyre::Result;
use itertools::Itertools;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::Config;
use crate::env_diff::EnvMap;
use crate::errors::Error;
use crate::toolset::{ToolRequest, ToolSource, Toolset};
use crate::{config, env};

#[derive(Debug, Default)]
pub struct ToolsetBuilder {
    args: Vec<ToolArg>,
    global_only: bool,
    default_to_latest: bool,
}

impl ToolsetBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_args(mut self, args: &[ToolArg]) -> Self {
        self.args = args.to_vec();
        self
    }

    pub fn with_default_to_latest(mut self, default_to_latest: bool) -> Self {
        self.default_to_latest = default_to_latest;
        self
    }

    pub fn with_global_only(mut self, global_only: bool) -> Self {
        self.global_only = global_only;
        self
    }

    pub async fn build(self, config: &Arc<Config>) -> Result<Toolset> {
        let mut toolset = Toolset {
            ..Default::default()
        };
        measure!("toolset_builder::build::load_config_files", {
            self.load_config_files(config, &mut toolset)?;
        });
        measure!("toolset_builder::build::load_runtime_env", {
            self.load_runtime_env(&mut toolset, env::vars().collect())?;
        });
        measure!("toolset_builder::build::load_runtime_args", {
            self.load_runtime_args(&mut toolset)?;
        });
        measure!("toolset_builder::build::resolve", {
            if let Err(err) = toolset.resolve(config).await {
                if Error::is_argument_err(&err) {
                    return Err(err);
                }
                warn!("failed to resolve toolset: {err}");
            }
        });

        time!("toolset::builder::build");
        Ok(toolset)
    }

    fn load_config_files(&self, config: &Arc<Config>, ts: &mut Toolset) -> eyre::Result<()> {
        for cf in config.config_files.values().rev() {
            if self.global_only && !config::is_global_config(cf.get_path()) {
                continue;
            }
            ts.merge(cf.to_toolset()?);
        }
        Ok(())
    }

    fn load_runtime_env(&self, ts: &mut Toolset, env: EnvMap) -> eyre::Result<()> {
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
                let ba: Arc<BackendArg> = Arc::new(plugin_name.as_str().into());
                let source = ToolSource::Environment(k, v.clone());
                let mut env_ts = Toolset::new(source.clone());
                for v in v.split_whitespace() {
                    let tvr = ToolRequest::new(ba.clone(), v, source.clone())?;
                    env_ts.add_version(tvr);
                }
                ts.merge(env_ts);
            }
        }
        Ok(())
    }

    fn load_runtime_args(&self, ts: &mut Toolset) -> eyre::Result<()> {
        for (_, args) in self.args.iter().into_group_map_by(|arg| arg.ba.clone()) {
            let mut arg_ts = Toolset::new(ToolSource::Argument);
            for arg in args {
                if let Some(tvr) = &arg.tvr {
                    arg_ts.add_version(tvr.clone());
                } else if self.default_to_latest {
                    // this logic is required for `mise x` because with that specific command mise
                    // should default to installing the "latest" version if no version is specified
                    // in mise.toml

                    // determine if we already have some active version in config
                    let current_active = ts
                        .list_current_requests()
                        .into_iter()
                        .find(|tvr| tvr.ba() == &arg.ba);

                    if let Some(current_active) = current_active {
                        // active version, so don't set "latest"
                        arg_ts.add_version(ToolRequest::new(
                            arg.ba.clone(),
                            &current_active.version(),
                            ToolSource::Argument,
                        )?);
                    } else {
                        // no active version, so use "latest"
                        arg_ts.add_version(ToolRequest::new(
                            arg.ba.clone(),
                            "latest",
                            ToolSource::Argument,
                        )?);
                    }
                }
            }
            ts.merge(arg_ts);
        }
        Ok(())
    }
}
