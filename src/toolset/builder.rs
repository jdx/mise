use std::collections::BTreeMap;

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

    pub fn build(self, config: &Config) -> Result<Toolset> {
        let settings = Settings::try_get()?;
        let mut toolset = Toolset {
            disable_tools: settings
                .disable_tools
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_>>()?,
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
                let fa: ForgeArg = plugin_name.parse().unwrap();
                let source = ToolSource::Environment(k, v.clone());
                let mut env_ts = Toolset::new(source);
                for v in v.split_whitespace() {
                    let tvr = ToolVersionRequest::new(fa.clone(), v);
                    env_ts.add_version(tvr, Default::default());
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
                    arg_ts.add_version(tvr.clone(), Default::default());
                } else if self.default_to_latest {
                    // TODO: see if there is a cleaner way to handle this scenario
                    // this logic is required for `mise x` because with that specific command mise
                    // should default to installing the "latest" version if no version is specified
                    // in .mise.toml
                    let versions_by_plugin = ts.list_versions_by_plugin();
                    let set_as_latest = versions_by_plugin.iter().find(|(_ta, fa)| {
                        !fa.iter().any(|f|
                            // Same forget type and same forgeArg name
                            f.forge.forge_type == arg.forge.forge_type &&
                                f.forge.name == arg.forge.name)
                    });

                    if let Some((_ta, _fa)) = set_as_latest {
                        arg_ts.add_version(
                            ToolVersionRequest::new(arg.forge.clone(), "latest"),
                            Default::default(),
                        );
                    }
                }
            }
            ts.merge(arg_ts);
        }
    }
}
