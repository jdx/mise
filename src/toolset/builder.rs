use std::collections::BTreeMap;

use eyre::Result;
use itertools::Itertools;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::Config;
use crate::env;
use crate::errors::Error;
use crate::toolset::{ToolRequest, ToolSource, Toolset};

#[derive(Debug, Default)]
pub struct ToolsetBuilder {
    args: Vec<ToolArg>,
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

    pub fn build(self, config: &Config) -> Result<Toolset> {
        let mut toolset = Toolset {
            ..Default::default()
        };
        self.load_config_files(config, &mut toolset)?;
        self.load_runtime_env(&mut toolset, env::vars().collect())?;
        self.load_runtime_args(&mut toolset)?;
        if let Err(err) = toolset.resolve() {
            if Error::is_argument_err(&err) {
                return Err(err);
            }
            warn!("failed to resolve toolset: {err:#}");
        }

        time!("toolset::builder::build", "{toolset}");
        Ok(toolset)
    }

    fn load_config_files(&self, config: &Config, ts: &mut Toolset) -> eyre::Result<()> {
        for cf in config.config_files.values().rev() {
            ts.merge(cf.to_toolset()?);
        }
        Ok(())
    }

    fn load_runtime_env(
        &self,
        ts: &mut Toolset,
        env: BTreeMap<String, String>,
    ) -> eyre::Result<()> {
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
                let fa: BackendArg = plugin_name.as_str().into();
                let source = ToolSource::Environment(k, v.clone());
                let mut env_ts = Toolset::new(source);
                for v in v.split_whitespace() {
                    let tvr = ToolRequest::new(fa.clone(), v)?;
                    env_ts.add_version(tvr);
                }
                ts.merge(env_ts);
            }
        }
        Ok(())
    }

    fn load_runtime_args(&self, ts: &mut Toolset) -> eyre::Result<()> {
        for (_, args) in self
            .args
            .iter()
            .into_group_map_by(|arg| arg.backend.clone())
        {
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
                        .any(|tvr| tvr.backend() == &arg.backend);

                    if set_as_latest {
                        // no active version, so use "latest"
                        arg_ts.add_version(ToolRequest::new(arg.backend.clone(), "latest")?);
                    }
                }
            }
            ts.merge(arg_ts);
        }
        Ok(())
    }
}
