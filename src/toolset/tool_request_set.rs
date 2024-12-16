use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::{Debug, Display};

use indexmap::IndexMap;
use itertools::Itertools;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::{Config, Settings};
use crate::env;
use crate::registry::REGISTRY;
use crate::toolset::{ToolRequest, ToolSource, Toolset};

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
            .filter(|tr| tr.is_os_supported() && !tr.is_installed())
            .collect()
    }

    pub fn list_tools(&self) -> Vec<&BackendArg> {
        self.tools.keys().collect()
    }

    pub fn add_version(&mut self, tr: ToolRequest, source: &ToolSource) {
        let fa = tr.ba();
        if !self.tools.contains_key(fa) {
            self.sources.insert(fa.clone(), source.clone());
        }
        let list = self.tools.entry(tr.ba().clone()).or_default();
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

    pub fn filter_by_tool(&self, mut tools: HashSet<String>) -> ToolRequestSet {
        // add in the full names so something like cargo:cargo-binstall can be used in place of cargo-binstall
        for short in tools.clone().iter() {
            if let Some(rt) = REGISTRY.get(short.as_str()) {
                tools.extend(rt.backends().iter().map(|s| s.to_string()));
            }
        }
        self.iter()
            .filter(|(ba, ..)| tools.contains(&ba.short))
            .map(|(fa, trl, ts)| (fa.clone(), trl.clone(), ts.clone()))
            .collect::<ToolRequestSet>()
    }

    pub fn into_toolset(self) -> Toolset {
        self.into()
    }
}

impl Display for ToolRequestSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let versions = self.tools.values().flatten().join(" ");
        if versions.is_empty() {
            write!(f, "ToolRequestSet: <empty>")?;
        } else {
            write!(f, "ToolRequestSet: {}", versions)?;
        }
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
    disable_tools: BTreeSet<BackendArg>,
}

impl ToolRequestSetBuilder {
    pub fn new() -> Self {
        let settings = Settings::get();
        Self {
            disable_tools: settings.disable_tools().iter().map(|s| s.into()).collect(),
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
        let mut trs = ToolRequestSet::default();
        self.load_config_files(&mut trs)?;
        self.load_runtime_env(&mut trs)?;
        self.load_runtime_args(&mut trs)?;

        let backends = trs.tools.keys().cloned().collect::<Vec<_>>();
        for fa in &backends {
            if self.is_disabled(fa) {
                trs.tools.shift_remove(fa);
                trs.sources.remove(fa);
            }
        }

        time!("tool_request_set::build");
        Ok(trs)
    }

    fn is_disabled(&self, ba: &BackendArg) -> bool {
        !ba.is_os_supported() || self.disable_tools.contains(ba)
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
                    let tvr = ToolRequest::new(fa.clone(), v, source.clone())?;
                    env_ts.add_version(tvr, &source);
                }
                merge(trs, env_ts);
            }
        }
        Ok(())
    }

    fn load_runtime_args(&self, trs: &mut ToolRequestSet) -> eyre::Result<()> {
        for (_, args) in self.args.iter().into_group_map_by(|arg| arg.ba.clone()) {
            let mut arg_ts = ToolRequestSet::new();
            for arg in args {
                if let Some(tvr) = &arg.tvr {
                    arg_ts.add_version(tvr.clone(), &ToolSource::Argument);
                } else if self.default_to_latest {
                    // this logic is required for `mise x` because with that specific command mise
                    // should default to installing the "latest" version if no version is specified
                    // in mise.toml

                    if !trs.tools.contains_key(&arg.ba) {
                        // no active version, so use "latest"
                        let tr = ToolRequest::new(arg.ba.clone(), "latest", ToolSource::Argument)?;
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
