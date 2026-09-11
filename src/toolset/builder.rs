use std::sync::Arc;

use eyre::Result;
use itertools::Itertools;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::{Config, ConfigMap};
use crate::env_diff::EnvMap;
use crate::errors::Error;
use crate::toolset::tool_request::LockfileScope;
use crate::toolset::tool_request_set::{
    configured_options_for_runtime_request, postinstall_tool_request,
};
use crate::toolset::{ResolveOptions, ToolRequest, ToolSource, Toolset, tool_from_env_var_name};
use crate::{config, env};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigScope {
    /// Include tools from all config files
    #[default]
    All,
    /// Only include tools from local (non-global) config files
    LocalOnly,
    /// Only include tools from the global config file
    GlobalOnly,
}

#[derive(Debug, Default)]
pub(crate) struct ToolsetBuilder {
    args: Vec<ToolArg>,
    scope: ConfigScope,
    default_to_latest: bool,
    resolve_options: ResolveOptions,
    resolution_progress: bool,
    config_files: Option<ConfigMap>,
    warn_overridden_lockfiles: bool,
}

impl ToolsetBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_args(mut self, args: &[ToolArg]) -> Self {
        self.args = args.to_vec();
        self
    }

    pub(crate) fn with_default_to_latest(mut self, default_to_latest: bool) -> Self {
        self.default_to_latest = default_to_latest;
        self
    }

    pub(crate) fn with_scope(mut self, scope: ConfigScope) -> Self {
        self.scope = scope;
        self
    }

    pub(crate) fn with_resolution_progress(mut self, enabled: bool) -> Self {
        self.resolution_progress = enabled;
        self
    }

    pub(crate) fn with_resolve_options(mut self, resolve_options: ResolveOptions) -> Self {
        self.resolve_options = resolve_options;
        self
    }

    pub(crate) fn with_overridden_lockfile_warnings(mut self) -> Self {
        self.warn_overridden_lockfiles = true;
        self
    }

    /// Use custom config files instead of config.config_files
    pub(crate) fn with_config_files(mut self, config_files: ConfigMap) -> Self {
        self.config_files = Some(config_files);
        self
    }

    pub(crate) async fn build(self, config: &Arc<Config>) -> Result<Toolset> {
        let mut toolset = Toolset {
            ..Default::default()
        };
        measure!("toolset_builder::build::load_config_files", {
            self.load_config_files(config, &mut toolset)?;
        });
        measure!("toolset_builder::build::load_runtime_env", {
            self.load_runtime_env(&mut toolset, env::vars_safe().collect())?;
        });
        measure!("toolset_builder::build::load_runtime_args", {
            self.load_runtime_args(&mut toolset)?;
        });
        measure!("toolset_builder::build::resolve", {
            let result = toolset
                .resolve_with_progress(config, &self.resolve_options, self.resolution_progress)
                .await;
            if let Err(err) = result {
                if Error::is_argument_err(&err) || Error::is_required_channel_resolution_err(&err) {
                    return Err(err);
                }
                warn!("failed to resolve toolset: {err}");
            } else if self.warn_overridden_lockfiles && self.resolve_options.use_locked_version {
                self.warn_overridden_lockfiles(config, &toolset);
            }
        });

        time!("toolset::builder::build");
        Ok(toolset)
    }

    fn load_config_files(&self, config: &Arc<Config>, ts: &mut Toolset) -> eyre::Result<()> {
        let config_files = self.config_files.as_ref().unwrap_or(&config.config_files);

        for cf in config_files.values().rev() {
            let is_global = config::is_global_config(cf.get_path());
            match self.scope {
                ConfigScope::GlobalOnly if !is_global => continue,
                ConfigScope::LocalOnly if is_global => continue,
                _ => {}
            }
            ts.merge(cf.to_toolset()?);
        }
        let scoped_files = config_files
            .iter()
            .filter(|(_, cf)| match self.scope {
                ConfigScope::All => true,
                ConfigScope::LocalOnly => !config::is_global_config(cf.get_path()),
                ConfigScope::GlobalOnly => config::is_global_config(cf.get_path()),
            })
            .map(|(path, cf)| (path.clone(), cf.clone()))
            .collect();
        let scoped_daemons;
        let daemons = if self.config_files.is_none() && matches!(self.scope, ConfigScope::All) {
            config.daemons()?
        } else {
            scoped_daemons = crate::daemons::load(&scoped_files)?;
            &scoped_daemons
        };
        if !daemons.daemons.values().any(|daemon| daemon.tool.is_some()) {
            return Ok(());
        }
        let mut requests = crate::toolset::ToolRequestSet::new();
        for versions in ts.versions.values() {
            for request in &versions.requests {
                requests.add_version(request.clone(), request.source());
            }
        }
        daemons.add_tool_requests(&mut requests)?;
        ts.merge(requests.into_toolset());
        Ok(())
    }

    fn load_runtime_env(&self, ts: &mut Toolset, env: EnvMap) -> eyre::Result<()> {
        if self.scope == ConfigScope::LocalOnly {
            // LocalOnly excludes env-based tool versions (MISE_*_VERSION).
            return Ok(());
        }
        let postinstall = postinstall_tool_request(&env)?.map(|(mut request, source)| {
            if let Some(config_options) = ts
                .versions
                .get(request.ba())
                .and_then(|tvl| configured_options_for_runtime_request(&tvl.requests, &request))
            {
                request.apply_config_options(config_options);
            }
            (request, source)
        });
        for (k, v) in env {
            if let Some(tool_name) = tool_from_env_var_name(&k) {
                let ba: Arc<BackendArg> = Arc::new(tool_name.as_str().into());
                let source = ToolSource::Environment(k, v.clone());
                let mut env_ts = Toolset::new(source.clone());
                for v in v.split_whitespace() {
                    let tvr = ToolRequest::new(ba.clone(), v, source.clone())?;
                    env_ts.add_version(tvr);
                }
                ts.merge(env_ts);
            }
        }
        if let Some((request, source)) = postinstall {
            let mut postinstall_ts = Toolset::new(source);
            postinstall_ts.add_version(request);
            ts.merge(postinstall_ts);
        }
        Ok(())
    }

    fn warn_overridden_lockfiles(&self, config: &Config, ts: &Toolset) {
        let config_files = self.config_files.as_ref().unwrap_or(&config.config_files);
        for arg in &self.args {
            let Some(tvl) = ts.versions.get(&arg.ba) else {
                continue;
            };
            for effective in &tvl.requests {
                let Some(owner) = effective.lockfile_source() else {
                    continue;
                };
                let Some(path) = owner.path() else {
                    continue;
                };
                // Toolset resolution can warn about a failed list and still return Ok.
                let Some(resolved) = tvl.versions.iter().find(|version| {
                    version.request.version() == effective.version()
                        && version.request.options() == effective.options()
                }) else {
                    continue;
                };
                if resolved.resolved_from_lockfile() {
                    continue;
                }
                for cf in config_files
                    .values()
                    .skip_while(|cf| cf.get_path() != path)
                    .skip(1)
                {
                    let has_match = (|| -> Result<bool> {
                        let configured = cf.to_tool_request_set()?;
                        let Some(requests) = configured.tools.get(&arg.ba) else {
                            return Ok(false);
                        };
                        let mut request = match &arg.tvr {
                            Some(request) => request.clone(),
                            None => ToolRequest::new(
                                arg.ba.clone(),
                                &effective.version(),
                                ToolSource::Argument,
                            )?,
                        };
                        if let Some(options) =
                            configured_options_for_runtime_request(requests, &request)
                        {
                            request.apply_config_options(options);
                        }
                        request.set_lockfile_scope(LockfileScope::Source(cf.source()));
                        match request.lockfile_resolve(config) {
                            Ok(pin) => Ok(pin.is_some()),
                            Err(err)
                                if err
                                    .downcast_ref::<crate::lockfile::AmbiguousRequestBinding>()
                                    .is_some() =>
                            {
                                Ok(true)
                            }
                            Err(err) => Err(err),
                        }
                    })();
                    match has_match {
                        Ok(true) => warn!(
                            "Ignoring lockfile pins for {} from {} because {} overrides that tool and has no matching lock entry",
                            effective,
                            cf.source(),
                            owner
                        ),
                        Ok(false) => {}
                        Err(err) => debug!(
                            "could not inspect overridden lockfile pins for {} from {}: {err:#}",
                            effective,
                            cf.source()
                        ),
                    }
                }
            }
        }
    }

    fn load_runtime_args(&self, ts: &mut Toolset) -> eyre::Result<()> {
        for (_, args) in self.args.iter().into_group_map_by(|arg| arg.ba.clone()) {
            let mut arg_ts = Toolset::new(ToolSource::Argument);
            let configured = ts
                .versions
                .get(&args[0].ba)
                .map(|tvl| tvl.requests.clone())
                .unwrap_or_default();
            let apply_arg_options = |mut tvr: ToolRequest| {
                if let Some(config_options) =
                    configured_options_for_runtime_request(&configured, &tvr)
                {
                    tvr.apply_config_options(config_options);
                }
                if self.resolve_options.use_locked_version {
                    let scope = match configured.iter().find(|request| request.is_os_supported()) {
                        Some(configured) if configured.source().path().is_some() => {
                            LockfileScope::Source(configured.source().clone())
                        }
                        // Environment overrides retain their existing lookup policy.
                        Some(_) => LockfileScope::Default,
                        None => LockfileScope::NoOwner,
                    };
                    tvr.set_lockfile_scope(scope);
                }
                tvr
            };
            for arg in args {
                if let Some(tvr) = &arg.tvr {
                    let tvr = apply_arg_options(tvr.clone());
                    arg_ts.add_version(tvr);
                } else if self.default_to_latest {
                    // this logic is required for `mise x` because with that specific command mise
                    // should default to installing the "latest" version if no version is specified
                    // in mise.toml

                    // determine if we already have some active version in config
                    let current_active = ts
                        .list_current_requests()
                        .into_iter()
                        .filter(|tvr| tvr.is_os_supported())
                        .find(|tvr| tvr.ba() == &arg.ba);

                    if let Some(current_active) = current_active {
                        // active version, so don't set "latest"
                        let tvr = ToolRequest::new(
                            arg.ba.clone(),
                            &current_active.version(),
                            ToolSource::Argument,
                        )?;
                        let tvr = apply_arg_options(tvr);
                        arg_ts.add_version(tvr);
                    } else {
                        // no active version, so use "latest"
                        let tvr = ToolRequest::new(arg.ba.clone(), "latest", ToolSource::Argument)?;
                        let tvr = apply_arg_options(tvr);
                        arg_ts.add_version(tvr);
                    }
                }
            }
            ts.merge(arg_ts);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolset::parse_tool_options;

    #[tokio::test]
    async fn test_postinstall_request_preserves_configured_options() {
        crate::toolset::install_state::init().await.unwrap();
        let ba = Arc::new(BackendArg::from("dummy"));
        let configured = ToolRequest::new_with_options(
            ba.clone(),
            "1.0.0",
            parse_tool_options(r#"selected="configured""#),
            ToolSource::Unknown,
        )
        .unwrap();
        let mut ts = Toolset::new(ToolSource::Unknown);
        ts.add_version(configured);
        let env = EnvMap::from_iter([
            ("MISE_TOOL_INSTALL_PATH".into(), "/tmp/dummy".into()),
            ("MISE_TOOL_NAME".into(), "dummy".into()),
            (env::MISE_TOOL_VERSION_ENV_VAR.into(), "2.0.0".into()),
        ]);

        ToolsetBuilder::new()
            .load_runtime_env(&mut ts, env)
            .unwrap();

        let request = &ts.versions.get(&ba).unwrap().requests[0];
        assert_eq!(request.version(), "2.0.0");
        assert_eq!(request.options().get("selected"), Some("configured"));
    }

    #[tokio::test]
    async fn runtime_args_use_platform_supported_version_and_lockfile_owner() {
        crate::toolset::install_state::init().await.unwrap();
        let ba = Arc::new(BackendArg::from("dummy"));
        let inactive_os = match crate::cli::version::OS.as_str() {
            "linux" => "macos",
            _ => "linux",
        };
        let mut inactive_options = parse_tool_options(r#"selected="inactive""#);
        inactive_options.core.os = Some(vec![inactive_os.to_string()]);
        let active_source = ToolSource::MiseTomlDaemon("/project/mise.toml".into());
        let inactive = ToolRequest::new_with_options(
            ba.clone(),
            "1.0.0",
            inactive_options,
            ToolSource::MiseToml("/project/child/mise.toml".into()),
        )
        .unwrap();
        let active = ToolRequest::new_with_options(
            ba.clone(),
            "2.0.0",
            parse_tool_options(r#"selected="active""#),
            active_source.clone(),
        )
        .unwrap();
        for argument in ["dummy", "dummy@2.0.0"] {
            for include_active in [true, false] {
                let mut toolset = Toolset::new(ToolSource::Unknown);
                toolset.add_version(inactive.clone());
                if include_active {
                    toolset.add_version(active.clone());
                }
                ToolsetBuilder::new()
                    .with_args(&[argument.parse().unwrap()])
                    .with_default_to_latest(true)
                    .load_runtime_args(&mut toolset)
                    .unwrap();

                let requests = &toolset.versions.get(&ba).unwrap().requests;
                assert_eq!(requests.len(), 1);
                if include_active {
                    assert_eq!(requests[0].version(), "2.0.0");
                    assert_eq!(requests[0].options().get("selected"), Some("active"));
                    assert_eq!(requests[0].lockfile_source(), Some(&active_source));
                } else {
                    assert_eq!(requests[0].lockfile_source(), None);
                    assert_eq!(requests[0].options().get("selected"), None);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_runtime_arg_preserves_request_options_with_matching_config() {
        crate::toolset::install_state::init().await.unwrap();
        let ba = Arc::new(BackendArg::from("dummy"));
        let configured = ToolRequest::new_with_options(
            ba.clone(),
            "1.0.0",
            parse_tool_options(r#"selected="config""#),
            ToolSource::Unknown,
        )
        .unwrap();
        let mut toolset = Toolset::new(ToolSource::Unknown);
        toolset.add_version(configured);

        let mut arg = "dummy[inline_only=inline]@1.0.0"
            .parse::<ToolArg>()
            .unwrap();
        arg.tvr = Some(
            ToolRequest::new_with_options(
                arg.ba.clone(),
                "1.0.0",
                parse_tool_options(r#"request_only="request""#),
                ToolSource::Argument,
            )
            .unwrap(),
        );
        ToolsetBuilder::new()
            .with_args(&[arg])
            .load_runtime_args(&mut toolset)
            .unwrap();

        let requests = &toolset.versions.get(&ba).unwrap().requests;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].options().get("selected"), Some("config"));
        assert_eq!(requests[0].options().get("request_only"), Some("request"));
        assert_eq!(requests[0].options().get("inline_only"), Some("inline"));
    }

    #[tokio::test]
    async fn runtime_args_keep_provenance_and_scope_lockfile_reads() {
        crate::toolset::install_state::init().await.unwrap();
        let source = ToolSource::MiseToml("/project/mise.toml".into());
        for (argument, expected_version) in [("dummy@2", "2"), ("dummy", "latest")] {
            for use_locked_version in [true, false] {
                let ba = Arc::new(BackendArg::from("dummy"));
                let mut ts = Toolset::new(source.clone());
                ts.add_version(ToolRequest::new(ba.clone(), "latest", source.clone()).unwrap());
                ToolsetBuilder::new()
                    .with_args(&[argument.parse().unwrap()])
                    .with_default_to_latest(true)
                    .with_resolve_options(ResolveOptions {
                        use_locked_version,
                        ..Default::default()
                    })
                    .load_runtime_args(&mut ts)
                    .unwrap();
                let tvl = ts.versions.get(&ba).unwrap();
                assert_eq!(tvl.requests[0].version(), expected_version);
                assert_eq!(tvl.source, ToolSource::Argument);
                assert_eq!(tvl.requests[0].source(), &ToolSource::Argument);
                assert_eq!(
                    tvl.requests[0].lockfile_scope(),
                    &if use_locked_version {
                        LockfileScope::Source(source.clone())
                    } else {
                        LockfileScope::Default
                    }
                );
            }
        }
    }

    #[tokio::test]
    async fn runtime_args_without_config_do_not_borrow_stale_pins() {
        crate::toolset::install_state::init().await.unwrap();
        let ba = Arc::new(BackendArg::from("dummy"));
        let direct = ToolRequest::new(ba.clone(), "1", ToolSource::Argument).unwrap();
        assert_eq!(direct.lockfile_scope(), &LockfileScope::Default);

        let mut ts = Toolset::default();
        ToolsetBuilder::new()
            .with_args(&["dummy@1".parse().unwrap()])
            .load_runtime_args(&mut ts)
            .unwrap();
        let request = &ts.versions[&ba].requests[0];
        assert_eq!(request.source(), &ToolSource::Argument);
        assert_eq!(request.lockfile_source(), None);

        let source = ToolSource::Environment("MISE_DUMMY_VERSION".into(), "2".into());
        let mut ts = Toolset::new(source.clone());
        ts.add_version(ToolRequest::new(ba.clone(), "2", source).unwrap());
        ToolsetBuilder::new()
            .with_args(&["dummy@1".parse().unwrap()])
            .load_runtime_args(&mut ts)
            .unwrap();
        assert_eq!(
            ts.versions[&ba].requests[0].lockfile_scope(),
            &LockfileScope::Default
        );
    }
}
