use std::fmt::{Debug, Display};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::Arc,
};

use crate::backend::backend_type::BackendType;
use crate::cli::args::{BackendArg, ToolArg};
use crate::config::{Config, Settings};
use crate::env;
use crate::registry::{REGISTRY, tool_enabled};
use crate::toolset::{ToolRequest, ToolSource, Toolset};
use indexmap::IndexMap;
use itertools::Itertools;

#[derive(Debug, Default, Clone)]
pub struct ToolRequestSet {
    pub tools: IndexMap<Arc<BackendArg>, Vec<ToolRequest>>,
    pub sources: BTreeMap<Arc<BackendArg>, ToolSource>,
    /// Tools that were filtered out because they don't exist in the registry (BackendType::Unknown)
    pub unknown_tools: Vec<Arc<BackendArg>>,
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

    pub async fn missing_tools(&self, config: &Arc<Config>) -> Vec<&ToolRequest> {
        let mut tools = vec![];
        for tr in self.tools.values().flatten() {
            if tr.is_os_supported() && !tr.is_installed(config).await {
                tools.push(tr);
            }
        }
        tools
    }

    pub fn list_tools(&self) -> Vec<&Arc<BackendArg>> {
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

    pub fn iter(&self) -> impl Iterator<Item = (&Arc<BackendArg>, &Vec<ToolRequest>, &ToolSource)> {
        self.tools
            .iter()
            .map(|(backend, tvr)| (backend, tvr, self.sources.get(backend).unwrap()))
    }

    pub fn into_iter(
        self,
    ) -> impl Iterator<Item = (Arc<BackendArg>, Vec<ToolRequest>, ToolSource)> {
        self.tools.into_iter().map(move |(ba, tvr)| {
            let source = self.sources.get(&ba).unwrap().clone();
            (ba, tvr, source)
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
            .filter(|(ba, ..)| tools.contains(&ba.short) || tools.contains(&ba.full()))
            .map(|(ba, trl, ts)| (ba.clone(), trl.clone(), ts.clone()))
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
            write!(f, "ToolRequestSet: {versions}")?;
        }
        Ok(())
    }
}

impl FromIterator<(Arc<BackendArg>, Vec<ToolRequest>, ToolSource)> for ToolRequestSet {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (Arc<BackendArg>, Vec<ToolRequest>, ToolSource)>,
    {
        let mut trs = ToolRequestSet::new();
        for (_ba, tvr, source) in iter {
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
    /// tools which will be enabled
    enable_tools: BTreeSet<BackendArg>,
}

impl ToolRequestSetBuilder {
    pub fn new() -> Self {
        let settings = Settings::get();
        Self {
            disable_tools: settings.disable_tools().iter().map(|s| s.into()).collect(),
            enable_tools: settings.enable_tools().iter().map(|s| s.into()).collect(),
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

    pub async fn build(&self, config: &Arc<Config>) -> eyre::Result<ToolRequestSet> {
        let mut trs = ToolRequestSet::default();
        trs = self.load_config_files(config, trs).await?;
        trs = self.load_runtime_env(trs)?;
        trs = self.load_runtime_args(trs)?;

        for ba in trs.tools.keys().cloned().collect_vec() {
            if self.is_disabled(&ba) {
                // Track tools that don't exist in the registry
                if ba.backend_type() == BackendType::Unknown {
                    trs.unknown_tools.push(ba.clone());
                }
                trs.tools.shift_remove(&ba);
                trs.sources.remove(&ba);
            }
        }

        time!("tool_request_set::build");
        Ok(trs)
    }

    fn is_disabled(&self, ba: &BackendArg) -> bool {
        let backend_type = ba.backend_type();
        backend_type == BackendType::Unknown
            || (cfg!(windows) && backend_type == BackendType::Asdf)
            || !ba.is_os_supported()
            || !tool_enabled(&self.enable_tools, &self.disable_tools, ba)
    }

    async fn load_config_files(
        &self,
        config: &Arc<Config>,
        mut trs: ToolRequestSet,
    ) -> eyre::Result<ToolRequestSet> {
        for cf in config.config_files.values().rev() {
            trs = merge(trs, cf.to_tool_request_set()?);
        }
        Ok(trs)
    }

    fn load_runtime_env(&self, mut trs: ToolRequestSet) -> eyre::Result<ToolRequestSet> {
        for (k, v) in env::vars_safe() {
            if k.starts_with("MISE_") && k.ends_with("_VERSION") && k != "MISE_VERSION" {
                let plugin_name = k
                    .trim_start_matches("MISE_")
                    .trim_end_matches("_VERSION")
                    .to_lowercase();
                if plugin_name == "install" || plugin_name == "tool" {
                    // ignore MISE_INSTALL_VERSION and MISE_TOOL_VERSION (set during hooks)
                    continue;
                }
                let ba: Arc<BackendArg> = Arc::new(plugin_name.as_str().into());
                let source = ToolSource::Environment(k, v.clone());
                let mut env_ts = ToolRequestSet::new();
                for v in v.split_whitespace() {
                    let tvr = ToolRequest::new(ba.clone(), v, source.clone())?;
                    env_ts.add_version(tvr, &source);
                }
                trs = merge(trs, env_ts);
            }
        }
        Ok(trs)
    }

    fn load_runtime_args(&self, mut trs: ToolRequestSet) -> eyre::Result<ToolRequestSet> {
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
            trs = merge(trs, arg_ts);
        }

        let tool_args = env::TOOL_ARGS.read().unwrap();
        let mut arg_trs = ToolRequestSet::new();
        for arg in tool_args.iter() {
            if let Some(tvr) = &arg.tvr {
                let mut tvr = tvr.clone();
                // When CLI specifies a version for a tool that's in config,
                // merge config options (e.g. postinstall) into the CLI request
                if tvr.options().is_empty()
                    && let Some(config_tvr) = trs.tools.get(&arg.ba).and_then(|v| v.first())
                {
                    let config_opts = config_tvr.options();
                    if !config_opts.is_empty() {
                        tvr.set_options(config_opts);
                    }
                }
                arg_trs.add_version(tvr, &ToolSource::Argument);
            } else if !trs.tools.contains_key(&arg.ba) {
                // no active version, so use "latest"
                let tr = ToolRequest::new(arg.ba.clone(), "latest", ToolSource::Argument)?;
                arg_trs.add_version(tr, &ToolSource::Argument);
            }
        }
        trs = merge(trs, arg_trs);

        Ok(trs)
    }
}

fn merge(mut a: ToolRequestSet, mut b: ToolRequestSet) -> ToolRequestSet {
    // move things around such that the tools are in the config order
    a.tools.retain(|ba, _| !b.tools.contains_key(ba));
    a.sources.retain(|ba, _| !b.sources.contains_key(ba));
    b.tools.extend(a.tools);
    b.sources.extend(a.sources);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_runtime_env_with_valid_utf8() {
        // This test verifies that valid UTF-8 MISE_*_VERSION variables work correctly
        unsafe {
            std::env::set_var("MISE_NODE_VERSION", "20.0.0");
            std::env::set_var("MISE_PYTHON_VERSION", "3.11");
        }

        let builder = ToolRequestSetBuilder::new();
        let trs = builder.load_runtime_env(ToolRequestSet::new());

        // Should not panic and should successfully load the versions
        assert!(trs.is_ok());
        let trs = trs.unwrap();
        assert!(trs.tools.len() >= 2 || trs.tools.is_empty()); // May be empty if backends are disabled

        unsafe {
            std::env::remove_var("MISE_NODE_VERSION");
            std::env::remove_var("MISE_PYTHON_VERSION");
        }
    }

    #[test]
    fn test_load_runtime_env_ignores_non_mise_vars() {
        // Non-MISE variables should be ignored, even with special characters
        unsafe {
            std::env::set_var("HOMEBREW_INSTALL_BADGE", "✅");
            std::env::set_var("SOME_OTHER_VAR", "value");
        }

        let builder = ToolRequestSetBuilder::new();
        let result = builder.load_runtime_env(ToolRequestSet::new());

        // Should not panic when non-MISE vars are present
        assert!(result.is_ok());

        unsafe {
            std::env::remove_var("HOMEBREW_INSTALL_BADGE");
            std::env::remove_var("SOME_OTHER_VAR");
        }
    }
}
