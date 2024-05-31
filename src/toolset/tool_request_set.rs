use std::collections::{BTreeMap, HashSet};
use std::fmt::{Debug, Display};

use indexmap::IndexMap;
use itertools::Itertools;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::{Config, Settings};
use crate::env;
use crate::toolset::{ToolRequest, ToolSource};

#[derive(Debug, Default, Clone)]
pub struct ToolRequestSet {
    pub tools: IndexMap<BackendArg, Vec<ToolRequest>>,
    pub sources: BTreeMap<BackendArg, ToolSource>,
}

impl ToolRequestSet {
    pub fn new() -> Self {
        Self::default()
    }

    // pub fn tools_with_sources(&self) -> Vec<(&BackendArg, &Vec<ToolRequest>, &ToolSource)> {
    //     self.tools
    //         .iter()
    //         .map(|(backend, tvr)| (backend, tvr, self.sources.get(backend).unwrap()))
    //         .collect()
    // }

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

    pub fn list_plugins(&self) -> Vec<&BackendArg> {
        self.tools.keys().collect()
    }

    pub fn list_current_versions(&self) -> Vec<(&BackendArg, &ToolRequest)> {
        self.tools
            .iter()
            .map(|(fa, tvr)| (fa, tvr.last().unwrap()))
            .collect()
    }

    pub fn add_version(&mut self, tr: ToolRequest, source: &ToolSource) {
        let fa = tr.backend();
        if !self.tools.contains_key(fa) {
            self.sources.insert(fa.clone(), source.clone());
        }
        let list = self.tools.entry(tr.backend().clone()).or_default();
        list.push(tr);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BackendArg, &Vec<ToolRequest>, &ToolSource)> {
        self.tools
            .iter()
            .map(|(backend, tvr)| (backend, tvr, self.sources.get(backend).unwrap()))
    }

    pub fn into_iter(self) -> impl Iterator<Item = (BackendArg, Vec<ToolRequest>, ToolSource)> {
        self.tools.into_iter().map(move |(fa, tvr)| {
            let source = self.sources.get(&fa).unwrap().clone();
            (fa, tvr, source)
        })
    }

    pub fn filter_by_tool(&self, tools: &HashSet<BackendArg>) -> Self {
        self.iter()
            .filter(|(fa, ..)| tools.contains(fa))
            .map(|(fa, trl, ts)| (fa.clone(), trl.clone(), ts.clone()))
            .collect::<ToolRequestSet>()
    }
}

impl Display for ToolRequestSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let versions = self.tools.values().flatten().join(" ");
        writeln!(f, "ToolRequestSet: {}", versions)?;
        Ok(())
    }
}

impl FromIterator<(BackendArg, Vec<ToolRequest>, ToolSource)> for ToolRequestSet {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (BackendArg, Vec<ToolRequest>, ToolSource)>,
    {
        let mut trs = ToolRequestSet::new();
        for (_fa, tvr, source) in iter {
            for tr in tvr {
                trs.add_version(tr.clone(), &source);
            }
        }
        trs
    }
}

#[derive(Debug, Default)]
pub struct ToolRequestSetBuilder {
    /// cli tool args
    args: Vec<ToolArg>,
    /// default to latest version if no version is specified (for `mise x`)
    default_to_latest: bool,
    /// tools which will be disabled
    disable_tools: Vec<BackendArg>,
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

    pub fn build(&self) -> eyre::Result<ToolRequestSet> {
        let start_ms = std::time::Instant::now();
        let mut trs = ToolRequestSet::default();
        self.load_config_files(&mut trs)?;
        self.load_runtime_env(&mut trs)?;
        self.load_runtime_args(&mut trs)?;

        let backends = trs.tools.keys().cloned().collect::<Vec<_>>();
        for fa in &backends {
            if self.is_disabled(fa) {
                trs.tools.swap_remove(fa);
                trs.sources.remove(fa);
            }
        }

        debug!("ToolRequestSet.build({:?}): {trs}", start_ms.elapsed());
        Ok(trs)
    }

    fn is_disabled(&self, fa: &BackendArg) -> bool {
        self.disable_tools.contains(fa)
    }

    fn load_config_files(&self, trs: &mut ToolRequestSet) -> eyre::Result<()> {
        let config = Config::get();
        for cf in config.config_files.values().rev() {
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
                let fa: BackendArg = plugin_name.as_str().into();
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
        for (_, args) in self
            .args
            .iter()
            .into_group_map_by(|arg| arg.backend.clone())
        {
            let mut arg_ts = ToolRequestSet::new();
            for arg in args {
                if let Some(tvr) = &arg.tvr {
                    arg_ts.add_version(tvr.clone(), &ToolSource::Argument);
                } else if self.default_to_latest {
                    // this logic is required for `mise x` because with that specific command mise
                    // should default to installing the "latest" version if no version is specified
                    // in .mise.toml

                    if !trs.tools.contains_key(&arg.backend) {
                        // no active version, so use "latest"
                        let tr = ToolRequest::new(arg.backend.clone(), "latest")?;
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
        let source = b.sources[&fa].clone();
        a.tools.insert(fa.clone(), versions);
        a.sources.insert(fa, source);
    }
}
