use crate::cli::args::{ForgeArg, ToolArg};
use crate::config::{Config, Settings};
use crate::toolset::{ToolSource, ToolVersionRequest as ToolRequest};
use crate::{config, env};
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Display};

#[derive(Debug, Default, Clone)]
pub struct ToolRequestSet {
    pub tools: IndexMap<ForgeArg, Vec<ToolRequest>>,
    pub sources: BTreeMap<ForgeArg, ToolSource>,
}

impl ToolRequestSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tools_with_sources(&self) -> Vec<(&ForgeArg, &Vec<ToolRequest>, &ToolSource)> {
        self.tools
            .iter()
            .map(|(forge, tvr)| (forge, tvr, self.sources.get(forge).unwrap()))
            .collect()
    }

    // pub fn installed_tools(&self) -> eyre::Result<Vec<&ToolRequest>> {
    //     self.tools
    //         .values()
    //         .flatten()
    //         .map(|tvr| match tvr.is_installed()? {
    //             true => Ok(Some(tvr)),
    //             false => Ok(None),
    //         })
    //         .flatten_ok()
    //         .collect()
    // }

    pub fn missing_tools(&self) -> Vec<&ToolRequest> {
        self.tools
            .values()
            .flatten()
            .filter(|tvr| !tvr.is_installed())
            .collect()
    }

    pub fn add_version(&mut self, tr: ToolRequest, source: &ToolSource) {
        let fa = tr.forge();
        if !self.tools.contains_key(fa) {
            self.sources.insert(fa.clone(), source.clone());
        }
        let list = self.tools.entry(tr.forge().clone()).or_default();
        list.push(tr);
    }
}

impl Display for ToolRequestSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (forge, versions, source) in self.tools_with_sources() {
            writeln!(f, "ToolRequestSet: {} ({:?})", forge, source)?;
            writeln!(f, "  {}", versions.iter().join(" "))?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct ToolRequestSetBuilder {
    /// cli tool args
    args: Vec<ToolArg>,
    /// only use global config files
    global_only: bool,
    /// default to latest version if no version is specified (for `mise x`)
    default_to_latest: bool,
    /// tools which will be disabled
    disable_tools: Vec<ForgeArg>,
    /// whitelist of tools to install
    tool_filter: Option<BTreeSet<ForgeArg>>,
    // /// only show tools which are already installed
    // installed_only: bool,
}

impl ToolRequestSetBuilder {
    pub fn new() -> Self {
        let settings = Settings::get();
        Self {
            disable_tools: settings.disable_tools.iter().map(|s| s.into()).collect(),
            ..Default::default()
        }
    }

    // pub fn add_arg(mut self, arg: ToolArg) -> Self {
    //     self.args.push(arg);
    //     self
    // }
    //
    // pub fn default_to_latest(mut self) -> Self {
    //     self.default_to_latest = true;
    //     self
    // }
    //
    // pub fn global_only(mut self) -> Self {
    //     self.global_only = true;
    //     self
    // }

    pub fn build(&self) -> eyre::Result<ToolRequestSet> {
        let start_ms = std::time::Instant::now();
        let mut trs = ToolRequestSet::default();
        self.load_config_files(&mut trs)?;
        if !self.global_only {
            self.load_runtime_env(&mut trs)?;
            self.load_runtime_args(&mut trs)?;
        }

        let forges = trs.tools.keys().cloned().collect::<Vec<_>>();
        for fa in &forges {
            if self.is_disabled(fa) {
                trs.tools.swap_remove(fa);
                trs.sources.remove(fa);
            }
        }

        debug!("ToolRequestSet ({:?}): {trs}", start_ms.elapsed());
        Ok(trs)
    }

    fn is_disabled(&self, fa: &ForgeArg) -> bool {
        self.disable_tools.contains(fa)
            || self.tool_filter.as_ref().is_some_and(|tf| !tf.contains(fa))
    }

    fn load_config_files(&self, trs: &mut ToolRequestSet) -> eyre::Result<()> {
        let config = Config::get();
        for cf in config.config_files.values().rev() {
            if self.global_only && !config::is_global_config(cf.get_path()) {
                return Ok(());
            }
            merge(trs, cf.to_tool_request_set()?);
        }
        Ok(())
    }

    fn load_runtime_env(&self, trs: &mut ToolRequestSet) -> eyre::Result<()> {
        for (k, v) in env::vars() {
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
                let mut env_ts = ToolRequestSet::new();
                for v in v.split_whitespace() {
                    let tvr = ToolRequest::new(fa.clone(), v)?;
                    env_ts.add_version(tvr, &source);
                }
                merge(trs, env_ts);
            }
        }
        Ok(())
    }

    fn load_runtime_args(&self, trs: &mut ToolRequestSet) -> eyre::Result<()> {
        for (_, args) in self.args.iter().into_group_map_by(|arg| arg.forge.clone()) {
            let mut arg_ts = ToolRequestSet::new();
            for arg in args {
                if let Some(tvr) = &arg.tvr {
                    arg_ts.add_version(tvr.clone(), &ToolSource::Argument);
                } else if self.default_to_latest {
                    // this logic is required for `mise x` because with that specific command mise
                    // should default to installing the "latest" version if no version is specified
                    // in .mise.toml

                    if !trs.tools.contains_key(&arg.forge) {
                        // no active version, so use "latest"
                        let tr = ToolRequest::new(arg.forge.clone(), "latest")?;
                        arg_ts.add_version(tr, &ToolSource::Argument);
                    }
                }
            }
            merge(trs, arg_ts);
        }
        Ok(())
    }
}

fn merge(a: &mut ToolRequestSet, b: ToolRequestSet) {
    for (fa, versions) in b.tools {
        a.tools.insert(fa, versions);
    }
    for (fa, source) in b.sources {
        a.sources.insert(fa, source);
    }
}
