//! The parts of the daemons module that drive mise's config and toolsets.
//!
//! The daemon declarations, ports, presets, and runtime live apart from these; this is where a
//! project's tools are resolved and installed for its daemons, and the host entries that let the
//! rest call back in.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, bail};
use indexmap::IndexMap;

use super::runtime::Runtime;
use super::{Daemon, DaemonSet, providers};
use crate::args::ToolArg;
use crate::config::Config;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::env_directive::EnvDirective;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource, Toolset, ToolsetBuilder};

pub async fn config_for_root(config: &Arc<Config>, root: &Path) -> Result<Arc<Config>> {
    let (paths, idiomatic) = crate::config::load_config_hierarchy_from_dir(root).await?;
    let files = crate::config::load_config_files_from_paths(&paths, &idiomatic).await?;
    Ok(config.with_config_files(files))
}

/// Build the daemon toolset without installing anything.
///
/// Kept separate from [`toolset`] so callers that must not install can avoid the
/// install path entirely. That path reaches code which is not `Send`, and a
/// caller inside a spawned task would not compile if this future contained it.
pub async fn toolset_resolved(config: &Arc<Config>, include_pitchfork: bool) -> Result<Toolset> {
    let pitchfork: ToolArg = "pitchfork".parse()?;
    let args = if include_pitchfork
        && !config
            .get_tool_request_set()
            .await?
            .tools
            .contains_key(&pitchfork.ba)
    {
        vec![pitchfork]
    } else {
        vec![]
    };
    // Resolve from what is on disk. `install_missing_versions` re-resolves the
    // specific requests it has to fetch, so an already-satisfied project costs
    // no network round trip -- which matters when this runs on every `mise run`
    // of a task that requires daemons.
    ToolsetBuilder::new()
        .with_args(&args)
        .with_default_to_latest(true)
        .with_resolve_options(crate::toolset::ResolveOptions {
            offline: true,
            ..Default::default()
        })
        .build(config)
        .await
}

/// The tools of `ts` that installing for `set` needs: pitchfork, each daemon's
/// tool, and everything those tools depend on to install.
///
/// Dependencies come from the same declarations the installer resolves, so a
/// runtime configured under its full backend id or named in a tool's `depends`
/// option is found the way the installer finds it.
fn install_scope(ts: &Toolset, set: &DaemonSet) -> Vec<Arc<crate::args::BackendArg>> {
    let mut roots = vec![crate::args::BackendArg::from("pitchfork")];
    for daemon in set.daemons.values().filter(|d| !d.imported) {
        if let Some((tool, _)) = &daemon.tool {
            roots.push(crate::args::BackendArg::from(tool.as_str()));
        }
    }
    let mut scope: Vec<Arc<crate::args::BackendArg>> = ts
        .versions
        .keys()
        .filter(|ba| {
            roots
                .iter()
                .any(|root| crate::install_context::backend_args_match(root, ba))
        })
        .cloned()
        .collect();
    let mut next = 0;
    while let Some(ba) = scope.get(next).cloned() {
        next += 1;
        let Some(list) = ts.versions.get(&ba) else {
            continue;
        };
        for request in &list.requests {
            let declarations = crate::install_context::install_dependency_declarations(request);
            for candidate in ts.versions.keys() {
                if declarations.matches(candidate)
                    && !scope.iter().any(|s| Arc::ptr_eq(s, candidate))
                {
                    scope.push(candidate.clone());
                }
            }
        }
    }
    scope
}

/// Build the daemon toolset, installing what `install` needs when it is given.
///
/// `install` is the set of daemons about to be validated and started. Only
/// pitchfork and the tools those daemons declare are installed: a preset's tool
/// comes from its daemon declaration, not the command line, so the default
/// install would leave it out and validation would report a version mismatch.
/// A tool that belongs to another daemon, or to no daemon, is left alone, so
/// one that cannot be installed does not keep an unrelated daemon from starting.
pub async fn toolset(
    config: &Arc<Config>,
    install: Option<&DaemonSet>,
) -> Result<(Arc<Config>, Toolset)> {
    let mut config = config.clone();
    let mut ts = toolset_resolved(&config, install.is_some()).await?;
    if let Some(set) = install {
        let wanted = install_scope(&ts, set);
        let mut skip = crate::config::Settings::get()
            .auto_install_disable_tools
            .clone()
            .unwrap_or_default();
        skip.extend(
            ts.versions
                .keys()
                .filter(|ba| !wanted.iter().any(|w| Arc::ptr_eq(w, ba)))
                .map(|ba| ba.short.clone()),
        );
        let (_, missing) = ts
            .install_missing_versions(
                &mut config,
                &crate::toolset::InstallOptions {
                    missing_args_only: false,
                    auto_install_disable_tools: Some(skip),
                    ..Default::default()
                },
            )
            .await?;
        ts.notify_missing_versions(missing);
    }
    Ok((config, ts))
}

pub async fn validate_tools(set: &DaemonSet, config: &Arc<Config>, ts: &Toolset) -> Result<()> {
    for daemon in set.daemons.values() {
        // Imported daemons resolve their tool against the project that declares
        // them, which happens when that root is prepared.
        let Some((tool, version)) = daemon.tool.as_ref().filter(|_| !daemon.imported) else {
            continue;
        };
        let ba: crate::args::BackendArg = tool.as_str().into();
        let Some(versions) = ts.versions.get(&ba) else {
            bail!("daemon {} requires {tool}@{version}", daemon.name);
        };
        let actual = versions
            .versions
            .first()
            .ok_or_else(|| eyre::eyre!("missing {tool}"))?;
        let compatible = ba
            .backend()?
            .list_installed_versions_matching(version)
            .contains(&actual.version);
        if compatible {
            continue;
        }
        let requested = ToolRequest::new(
            ba.into(),
            version,
            ToolSource::MiseTomlDaemon(daemon.source.clone()),
        )?
        .resolve(
            config,
            &crate::toolset::ResolveOptions {
                offline: true,
                ..Default::default()
            },
        )
        .await?;
        if actual.version != requested.version {
            bail!(
                "daemon {} requires {tool}@{version} ({}), but [tools] selects {}",
                daemon.name,
                requested.version,
                actual.version
            );
        }
    }
    Ok(())
}

pub async fn runtime_from_toolset(
    config: &Arc<Config>,
    ts: &Toolset,
    fallback: Option<&Path>,
) -> Result<Runtime> {
    let env = ts.env_with_path(config).await?;
    let bin = which::which_in(
        "pitchfork",
        env.get(&*crate::env::PATH_KEY),
        crate::dirs::CWD.as_deref().unwrap_or(Path::new(".")),
    )
    .ok()
    .or_else(|| fallback.filter(|p| p.is_file()).map(Path::to_path_buf))
    .ok_or_else(|| {
        eyre::eyre!("daemons require pitchfork; run `mise use pitchfork` or `mise daemons start`")
    })?;
    Ok(Runtime { bin, env })
}

async fn provider_tool_config(name: &str, source: &Path, daemon: &Daemon) -> Result<Arc<Config>> {
    let (tool, version) = daemon
        .tool
        .as_ref()
        .ok_or_else(|| eyre::eyre!("provider needs a tool"))?;
    let doc = toml::Table::from_iter([(
        "tools".into(),
        toml::Value::Table(toml::Table::from_iter([(
            tool.clone(),
            version.clone().into(),
        )])),
    )]);
    let cf = MiseToml::from_str(&toml::to_string(&doc)?, source)?;
    let mut config = Config::load_from_config_files(
        IndexMap::from([(
            source.to_path_buf(),
            Arc::new(cf) as Arc<dyn crate::config::config_file::ConfigFile>,
        )]),
        true,
    )
    .await?;
    Arc::get_mut(&mut config)
        .expect("new provider configuration")
        .project_root = Some(providers::directory(name));
    Ok(config)
}

/// The user's global configuration as a supervisor runtime: the recorded pitchfork binary when
/// there is one, otherwise the one the global toolset provides.
pub async fn global_runtime(previous_bin: Option<PathBuf>) -> Result<Runtime> {
    let config = Config::get().await?;
    let files = config
        .config_files
        .iter()
        .filter(|(path, _)| crate::config::is_global_config(path))
        .map(|(path, cf)| (path.clone(), cf.clone()))
        .collect();
    let global = Config::load_from_config_files(files, true).await?;
    let (_, ts) = toolset(&global, None).await?;
    Ok(match previous_bin {
        Some(bin) => Runtime {
            bin,
            env: ts.env_with_path(&global).await?,
        },
        None => {
            runtime_from_toolset(&global, &ts, which::which("pitchfork").ok().as_deref()).await?
        }
    })
}

/// Install the tool a provider runs, resolving it against the provider's own configuration.
pub async fn install_provider_tools(name: &str, source: &Path, daemon: &Daemon) -> Result<()> {
    let mut config = provider_tool_config(name, source, daemon).await?;
    let mut ts = Toolset::default();
    for cf in config.config_files.values() {
        ts.merge(cf.to_toolset()?);
    }
    ts.resolve_with_opts(
        &config,
        &crate::toolset::ResolveOptions {
            offline: true,
            ..Default::default()
        },
    )
    .await?;
    let (_, missing) = ts
        .install_missing_versions(
            &mut config,
            &crate::toolset::InstallOptions {
                missing_args_only: false,
                reload_config: false,
                ..Default::default()
            },
        )
        .await?;
    ts.notify_missing_versions(missing);
    Ok(())
}

/// The bin paths of a provider's tool, after checking the installed version matches.
pub async fn provider_tool_paths(
    name: &str,
    source: &Path,
    daemon: &Daemon,
) -> Result<Vec<PathBuf>> {
    let config = provider_tool_config(name, source, daemon).await?;
    let mut ts = Toolset::default();
    for cf in config.config_files.values() {
        ts.merge(cf.to_toolset()?);
    }
    ts.resolve_with_opts(
        &config,
        &crate::toolset::ResolveOptions {
            offline: true,
            ..Default::default()
        },
    )
    .await?;
    validate_tools(
        &DaemonSet {
            daemons: IndexMap::from([("server".into(), daemon.clone())]),
            ..Default::default()
        },
        &config,
        &ts,
    )
    .await?;
    Ok(ts.list_paths(&config).await)
}

/// What the rest of mise asks of a loaded [`DaemonSet`]: the tools its daemons declare and the
/// environment they export.
#[async_trait::async_trait(?Send)]
pub trait DaemonSetExt {
    fn add_tool_requests(&self, trs: &mut ToolRequestSet) -> Result<()>;
    fn env_entries(&self) -> Vec<(EnvDirective, PathBuf)>;
    async fn validate_tasks(&self, config: &Arc<Config>) -> Result<()>;
}

#[async_trait::async_trait(?Send)]
impl DaemonSetExt for DaemonSet {
    fn add_tool_requests(&self, trs: &mut ToolRequestSet) -> Result<()> {
        for daemon in self.daemons.values() {
            // An imported daemon runs out of its own project, which installs and
            // resolves its tool there. Adding it here would force an unrelated
            // version onto this project's toolset.
            let Some((tool, version)) = daemon.tool.as_ref().filter(|_| !daemon.imported) else {
                continue;
            };
            let ba = crate::args::BackendArg::from(tool.as_str());
            if let Some(existing) = trs.tools.get(&ba) {
                // Explicit declarations are validated against backend resolution before starting.
                if existing.iter().all(|tr| tr.source().is_mise_toml_daemon())
                    && existing.iter().any(|tr| tr.version() != *version)
                {
                    bail!(
                        "daemon {} requests {tool}@{version}, conflicting with another daemon; use one version per tool",
                        daemon.name
                    );
                }
                continue;
            }
            let source = ToolSource::MiseTomlDaemon(daemon.source.clone());
            let request = ToolRequest::new(ba.into(), version, source.clone())?;
            trs.add_version(request, &source);
        }
        Ok(())
    }

    fn env_entries(&self) -> Vec<(EnvDirective, PathBuf)> {
        self.daemons
            .values()
            .filter(|daemon| !daemon.imported)
            .flat_map(|daemon| {
                daemon.exports.iter().map(|(k, v)| {
                    (
                        EnvDirective::Val(k.clone(), v.clone(), Default::default()),
                        daemon.source.clone(),
                    )
                })
            })
            .collect()
    }

    /// Check `task = "..."` references against the loaded task list. Task
    /// loading is asynchronous and reads the filesystem, so this cannot run
    /// while `[daemons]` is parsed. The three paths that register a generated
    /// pitchfork configuration call it first: `mise daemons start`, a task that
    /// requires daemons, and the shell auto-lifecycle hook.
    async fn validate_tasks(&self, config: &Arc<Config>) -> Result<()> {
        if self.daemons.values().all(|d| d.task.is_none()) {
            return Ok(());
        }
        let tasks = config.tasks_with_aliases().await?;
        for daemon in self.daemons.values() {
            let Some(task) = &daemon.task else {
                continue;
            };
            if !tasks.contains_key(task) {
                bail!("daemon {} runs unknown task {task:?}", daemon.name);
            }
        }
        Ok(())
    }
}

/// A loaded config file, seen through the narrow interface the daemon modules use.
#[derive(Debug)]
struct FileView(Arc<dyn crate::config::config_file::ConfigFile>);

impl super::host::ConfigFileView for FileView {
    fn get_path(&self) -> &Path {
        self.0.get_path()
    }
    fn project_root(&self) -> Option<PathBuf> {
        self.0.project_root()
    }
    fn config_root(&self) -> PathBuf {
        self.0.config_root()
    }
    fn daemon_declarations(&self) -> IndexMap<String, super::Declaration> {
        self.0.daemon_declarations()
    }
    fn daemon_settings(&self) -> Option<super::DaemonSettings> {
        self.0.daemon_settings()
    }
    fn daemon_group_declarations(&self) -> IndexMap<String, super::GroupDeclaration> {
        self.0.daemon_group_declarations()
    }
    fn daemon_providers(&self) -> IndexMap<String, toml::Table> {
        self.0.daemon_providers()
    }
}

/// Convert mise's config files to the view the daemon modules take.
pub fn view_map(files: &crate::config::ConfigMap) -> super::host::ConfigMap {
    files
        .iter()
        .map(|(path, file)| {
            (
                path.clone(),
                Arc::new(FileView(file.clone())) as Arc<dyn super::host::ConfigFileView>,
            )
        })
        .collect()
}

/// [`super::load`] over mise's config files.
pub fn load(files: &crate::config::ConfigMap) -> Result<DaemonSet> {
    super::load(&view_map(files))
}

pub fn register() {
    use crate::config::{self, Settings, SettingsExt};
    super::host::register(super::host::Host {
        is_global_config: config::is_global_config,
        config_dir_is_ignored: config::config_dir_is_ignored,
        config_path_is_ignored: config::config_path_is_ignored,
        config_paths_in_dir: config::config_paths_in_dir,
        is_path_trusted: config::config_file::is_path_trusted,
        config_trust_root: config::config_file::config_trust_root,
        config_root: config::config_file::config_root::config_root,
        global_config_path: config::global_config_path,
        parse_config_file: |path| Ok(Arc::new(FileView(Arc::new(MiseToml::from_file(path)?)))),
        ceiling_paths: || &crate::env::MISE_CEILING_PATHS,
        safe_mode: Settings::safe_mode,
        ensure_not_safe: Settings::ensure_not_safe,
        global_runtime: |previous| Box::pin(global_runtime(previous)),
        install_provider_tools: |name, source, daemon| {
            Box::pin(install_provider_tools(name, source, daemon))
        },
        provider_tool_paths: |name, source, daemon| {
            Box::pin(provider_tool_paths(name, source, daemon))
        },
    });
}
