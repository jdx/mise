use crate::request_exit;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::atomic::Ordering,
};

use crate::backend::Backend;
use crate::cli::args::{BackendArg, ToolArg};
use crate::cli::exec::Exec;
use crate::config::{CommandWrapper, Config, Settings, load_command_wrappers};
use crate::file::display_path;
use crate::lock_file::LockFile;
use crate::toolset::{ResolveOptions, ToolVersion, Toolset, ToolsetBuilder};
use crate::{backend, dirs, env, fake_asdf, file};
use color_eyre::eyre::{Result, bail, eyre};
use eyre::WrapErr;
#[cfg(windows)]
use indoc::formatdoc;
use itertools::Itertools;
use path_absolutize::Absolutize;
use tokio::task::JoinSet;

#[cfg(any(windows, test))]
const NATIVE_SHIM_MARKER: &[u8] = include_bytes!("../crates/mise-shim/native-shim-marker");
const GENERATED_SHELL_SHIM_HEADER: &str = "#!/bin/sh\n# mise generated shim\n";
#[cfg(any(windows, test))]
const GENERATED_WINDOWS_CMD_SHIM_HEADER: &str = "@echo off\r\nrem mise generated shim\r\n";
#[cfg(any(windows, test))]
const GENERATED_WINDOWS_BASH_SHIM_HEADER: &str = "#!/bin/bash\n# mise generated shim\n";
const SHIM_SCRIPT_INSPECTION_LIMIT: u64 = 16 * 1024;

pub(crate) const TASK_TOOL_ARGS_ENV: &str = "__MISE_TASK_TOOL_ARGS";

#[derive(serde::Deserialize, serde::Serialize)]
struct TaskToolArg {
    backend: String,
    version: Option<String>,
    options: crate::toolset::ToolVersionOptions,
}

/// Preserve runtime-only task tool requests for a shim process. A task's `tools`
/// entries are not part of the config that a shim reloads, so without this context
/// a bootstrap shim cannot find a lazy provider declared only on the task.
pub(crate) fn task_tool_args_env(tools: &[ToolArg]) -> Result<Option<String>> {
    let tools = tools
        .iter()
        .map(|tool| TaskToolArg {
            backend: tool.ba.short.clone(),
            version: tool.version.clone(),
            options: tool
                .tvr
                .as_ref()
                .map(|request| request.options())
                .unwrap_or_default(),
        })
        .collect_vec();
    if tools.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::to_string(&tools)?))
    }
}

fn task_tool_args_from_env() -> Result<Vec<ToolArg>> {
    let Ok(serialized) = env::var(TASK_TOOL_ARGS_ENV) else {
        return Ok(vec![]);
    };
    serde_json::from_str::<Vec<TaskToolArg>>(&serialized)?
        .into_iter()
        .map(|tool| {
            let input = tool.version.as_ref().map_or_else(
                || tool.backend.clone(),
                |version| format!("{}@{version}", tool.backend),
            );
            let mut arg: ToolArg = input.parse()?;
            if !tool.options.is_empty() {
                Arc::make_mut(&mut arg.ba).set_opts(Some(tool.options));
            }
            arg.tvr = arg
                .version
                .as_ref()
                .map(|version| {
                    crate::toolset::ToolRequest::new(
                        arg.ba.clone(),
                        version,
                        crate::toolset::ToolSource::Argument,
                    )
                })
                .transpose()?;
            Ok(arg)
        })
        .collect()
}

// executes as if it was a shim if the command is not "mise", e.g.: "node"
pub(crate) async fn handle_shim() -> Result<()> {
    // TODO: instead, check if bin is in shims dir
    let bin_name = *env::MISE_BIN_NAME;
    if env::is_mise_binary(bin_name) || cfg!(test) {
        return Ok(());
    }
    #[cfg(windows)]
    {
        let shim_path = invoked_shim_path();
        if env::var_path(env::MISE_SHIM_PATH_ENV)
            .as_ref()
            .is_some_and(|previous| {
                file::paths_eq(
                    &file::canonicalize_or_self(previous),
                    &file::canonicalize_or_self(&shim_path),
                )
            })
        {
            bail!(
                "recursive shim invocation detected for {bin_name}: {}",
                display_path(&shim_path)
            );
        }
        *env::MISE_SHIM_PATH.write().unwrap() = Some(shim_path.clone());
        env::set_var(env::MISE_SHIM_PATH_ENV, &shim_path);
    }
    let mut config = Config::get().await?;
    let mut args = env::ARGS.read().unwrap().clone();
    env::PREFER_OFFLINE.store(true, Ordering::Relaxed);
    trace!("shim[{bin_name}] args: {}", args.join(" "));
    let (bin, ts, wrapper) = which_shim(&mut config, &env::MISE_BIN_NAME, &args).await?;
    args[0] = bin.to_string_lossy().to_string();
    if let Some(wrapper) = &wrapper {
        args.splice(1..1, wrapper.args().iter().cloned());
    }
    env::set_var("__MISE_SHIM", "1");
    let exec = Exec {
        tool: vec![],
        c: None,
        command: Some(args),
        jobs: None,
        raw: false,
        no_deps: true, // Skip deps for shims to avoid performance impact
        fresh_env: false,
        deny_all: false,
        deny_read: false,
        deny_write: false,
        deny_net: false,
        deny_env: false,
        allow_read: vec![],
        allow_write: vec![],
        allow_net: vec![],
        allow_env: vec![],
    };
    time!("shim exec");
    if let Some(wrapper) = wrapper {
        exec.run_with_command_wrapper(
            config,
            ts,
            wrapper
                .env()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
        .await?;
    } else {
        exec.run_with_toolset(config, ts).await?;
    }
    Err(request_exit(0))
}

#[cfg(windows)]
fn invoked_shim_path() -> PathBuf {
    let argv0 = PathBuf::from(&*env::ARGV0);
    if argv0.is_absolute() {
        return argv0;
    }
    if argv0.components().count() > 1 {
        return argv0
            .absolutize()
            .map(|path| path.into_owned())
            .unwrap_or(argv0);
    }
    which::which(&argv0)
        .ok()
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or(argv0)
}

async fn which_shim(
    config: &mut Arc<Config>,
    bin_name: &str,
    args: &[String],
) -> Result<(PathBuf, Toolset, Option<CommandWrapper>)> {
    // Shell completion invokes `usage complete-word` through the `usage` shim.
    // It should use the installed CLI or fail locally, never resolve a floating
    // tool version or auto-install over the network while the user is pressing
    // tab. On Windows the shim is invoked as `usage.exe`, so strip the platform
    // executable suffix before comparing.
    let shim_name = command_name_without_exe_suffix(bin_name);
    let is_usage = if cfg!(windows) {
        shim_name.eq_ignore_ascii_case("usage")
    } else {
        shim_name == "usage"
    };
    let completion_offline = is_usage && args.get(1).is_some_and(|arg| arg == "complete-word");
    let resolve_options = if completion_offline {
        ResolveOptions {
            offline: true,
            ..Default::default()
        }
    } else {
        ResolveOptions::default()
    };
    let task_tools = task_tool_args_from_env()?;
    let mut ts = ToolsetBuilder::new()
        .with_args(&task_tools)
        .with_resolve_options(resolve_options)
        .build(config)
        .await?;
    let wrappers = load_command_wrappers(&config.config_files)?;
    validate_wrapper_names(wrappers.keys())?;
    let wrapper = if cfg!(macos) {
        wrappers
            .iter()
            .find(|(name, _)| command_names_eq(name, shim_name))
            .map(|(_, wrapper)| wrapper)
    } else {
        wrappers.get(shim_name)
    };
    if let Some(wrapper) = wrapper {
        if command_names_eq(wrapper.command(), shim_name) {
            bail!("command wrapper for {shim_name} cannot delegate to itself");
        }
        trace!("shim[{bin_name}] WRAPPER command: {}", wrapper.command());
        return Ok((PathBuf::from(wrapper.command()), ts, Some(wrapper.clone())));
    }
    // A configured tool may intentionally override an executable bundled by another installed
    // tool (for example, a pinned npm overrides Node's npm). Install a missing provider declared
    // by the registry before resolving an incidental installed provider.
    if !completion_offline
        && Settings::get().not_found_auto_install
        && ts
            .should_install_missing_registry_bin_provider(config, shim_name)
            .await?
    {
        for tv in ts
            .install_missing_bin(config, shim_name)
            .await?
            .unwrap_or_default()
        {
            let p = tv.backend()?;
            if let Some(bin) =
                backend_which_shim(p.as_ref(), config, &tv, shim_name, bin_name).await?
            {
                trace!(
                    "shim[{bin_name}] REGISTRY ToolVersion: {tv} bin: {bin}",
                    bin = display_path(&bin)
                );
                return Ok((bin, ts, None));
            }
        }
    }
    for lookup_name in [shim_name, bin_name].into_iter().unique() {
        if let Some((p, tv)) = ts.which(config, lookup_name).await
            && let Some(bin) = p.which(config, &tv, lookup_name).await?
        {
            trace!(
                "shim[{bin_name}] ToolVersion: {tv} bin: {bin}",
                bin = display_path(&bin)
            );
            return Ok((bin, ts, None));
        }
    }
    // Lazy tools are explicit fallback providers. They install on first shim use even when
    // general not-found auto-install is disabled, but only after configured/project providers
    // and already-installed tools have had a chance to win.
    if !completion_offline && ts.has_missing_lazy_bin_provider(config, shim_name).await? {
        for tv in ts
            .install_missing_lazy_bin(config, shim_name)
            .await?
            .unwrap_or_default()
        {
            let backend = tv.backend()?;
            if let Some(bin) =
                backend_which_shim(backend.as_ref(), config, &tv, shim_name, bin_name).await?
            {
                trace!(
                    "shim[{bin_name}] LAZY ToolVersion: {tv} bin: {bin}",
                    bin = display_path(&bin)
                );
                return Ok((bin, ts, None));
            }
        }
    }
    // Auto-installing here would download a tool over the network; skip it for
    // offline completion so `usage complete-word` fails locally instead.
    if !completion_offline && Settings::get().not_found_auto_install {
        for tv in ts
            .install_missing_bin(config, shim_name)
            .await?
            .unwrap_or_default()
        {
            let p = tv.backend()?;
            if let Some(bin) =
                backend_which_shim(p.as_ref(), config, &tv, shim_name, bin_name).await?
            {
                trace!(
                    "shim[{bin_name}] NOT_FOUND ToolVersion: {tv} bin: {bin}",
                    bin = display_path(&bin)
                );
                return Ok((bin, ts, None));
            }
        }
    }
    // fallback for "system"
    if Settings::get().not_found_system_fallback {
        let mise_bin = file::canonicalize_or_self(&env::MISE_BIN);
        for path in &*env::PATH {
            if file::is_mise_shims_dir(path) || file::is_command_wrapper_dir(path) {
                continue;
            }
            let bin = path.join(bin_name);
            if bin.is_file() && file::is_executable(&bin) {
                if file::is_active_mise_shim(&bin) {
                    continue;
                }
                // Skip if this binary is a mise shim (symlink pointing to the mise binary)
                if file::canonicalize_cached(&bin).is_some_and(|bin| bin == mise_bin) {
                    continue;
                }
                trace!("shim[{bin_name}] SYSTEM {bin}", bin = display_path(&bin));
                return Ok((bin, ts, None));
            }
        }
    }
    let mut tvs = ts.list_rtvs_with_bin(config, shim_name).await?;
    if tvs.is_empty() && shim_name != bin_name {
        tvs = ts.list_rtvs_with_bin(config, bin_name).await?;
    }
    match err_no_version_set(config, ts, shim_name, tvs).await {
        Ok(_) => unreachable!("err_no_version_set always returns an error"),
        Err(err) => Err(err),
    }
}

async fn backend_which_shim(
    backend: &dyn Backend,
    config: &Arc<Config>,
    tv: &ToolVersion,
    shim_name: &str,
    bin_name: &str,
) -> Result<Option<PathBuf>> {
    // The extensionless name preserves normal Windows extension expansion (including `.cmd`),
    // while the original name is required for dotted stems such as `python3.12.exe`.
    for lookup_name in [shim_name, bin_name].into_iter().unique() {
        if let Some(bin) = backend.which(config, tv, lookup_name).await? {
            return Ok(Some(bin));
        }
    }
    Ok(None)
}

/// Build the actionable, `which_shim`-style resolution error for a bin that a
/// shim failed to resolve while dispatching through `mise x` (the exe-mode shim
/// on Windows, invoked with `__MISE_SHIM_PATH` set). Without this, that path
/// surfaces the opaque `cannot find binary path`; symlink shims already get
/// this message directly from `which_shim`. See discussion #11183.
#[cfg(not(test))]
pub(crate) async fn err_shim_not_found(bin_name: &str) -> color_eyre::Report {
    // Windows exe shims are invoked as `<tool>.exe`; name `<tool>` in the message.
    let bin_stem = bin_name
        .strip_suffix(std::env::consts::EXE_SUFFIX)
        .unwrap_or(bin_name);
    let build = async {
        let config = Config::get().await?;
        let ts = ToolsetBuilder::new().build(&config).await?;
        let tvs = ts.list_rtvs_with_bin(&config, bin_stem).await?;
        // err_no_version_set always returns Err; map its Ok arm defensively.
        err_no_version_set(&config, ts, bin_stem, tvs)
            .await
            .map(|_| eyre!("cannot find binary path: {bin_stem}"))
    };
    match build.await {
        Ok(report) | Err(report) => report,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShimScope {
    User,
    System,
    Both,
}

/// The shim farms that serve as the lazy-install fallback boundary on PATH: the user
/// farm, plus the system farm when it exists as separate storage.
pub(crate) fn shim_farm_dirs() -> Vec<PathBuf> {
    let user_shims = dirs::shims();
    let system_shims = dirs::system_shims();
    let mut dirs = vec![user_shims];
    if system_shims.is_dir() && !file::storage_paths_eq(&dirs[0], &system_shims) {
        dirs.push(system_shims);
    }
    dirs
}

/// Add bootstrap shims for missing lazy declarations without pruning either farm.
///
/// A `lazy = true` entry written by hand has no shim until something rebuilds the farm.
/// An interactive shell still recovers, because its not-found handler installs the
/// tool, but a task or `mise x` child has no such handler and fails with "command not
/// found" (discussion #12678). `missing` is the toolset's already-computed missing
/// version list. Once every lazy declaration is installed there is nothing left to
/// write, and the function returns before locating the mise binary, so that steady
/// state costs only the option checks.
pub(crate) fn ensure_lazy_shims(missing: &[ToolVersion]) -> Result<()> {
    let mut bins_by_dir = BTreeMap::<PathBuf, Vec<String>>::new();
    let mut lazy_bins_error = None;
    for tv in missing {
        if tv.request.options().lazy != Some(true) {
            continue;
        }
        let bins = match tv.request.lazy_bins() {
            Ok(Some(bins)) => bins,
            Ok(None) => continue,
            Err(err) => {
                // One malformed lazy declaration must not prevent bootstrap shims
                // from being written for every other declaration in the toolset.
                lazy_bins_error.get_or_insert(err);
                continue;
            }
        };
        let shims_dir = if shim_scope_contains_request(ShimScope::System, &tv.request) {
            dirs::system_shims()
        } else {
            dirs::shims()
        };
        bins_by_dir.entry(shims_dir).or_default().extend(bins);
    }
    if !bins_by_dir.is_empty() {
        // Locating the mise binary walks PATH, so defer it until a declaration
        // actually needs a shim.
        let mise_bin = mise_bin_for_shims().absolutize()?.into_owned();
        for (shims_dir, bins) in bins_by_dir {
            let shims = bins
                .iter()
                .flat_map(|bin| platform_shim_names(&mise_bin, bin))
                .collect::<BTreeSet<String>>();
            match write_bootstrap_shims(&mise_bin, &shims_dir, &shims)? {
                None => {}
                // A shared farm such as `/usr/local/bin` may belong to root. No
                // command can write a bootstrap shim there for this user, so
                // warning on every `mise env`, `mise x` and `mise run` would only
                // be noise. Lazy tools in that farm still install through the
                // not-found handler or an explicit `mise install`.
                Some(err) => {
                    debug!(
                        "skipping bootstrap shims in {}: {err:#}",
                        display_path(&shims_dir)
                    );
                }
            }
        }
    }
    if let Some(err) = lazy_bins_error {
        Err(err)
    } else {
        Ok(())
    }
}

fn write_bootstrap_shims(
    mise_bin: &Path,
    shims_dir: &Path,
    shims: &BTreeSet<String>,
) -> Result<Option<eyre::Report>> {
    if let Err(err) = file::create_dir_all(shims_dir) {
        if is_permission_denied(&err) {
            return Ok(Some(err));
        }
        return Err(err);
    }
    // Lock failures come from the cache rather than the target farm and must
    // remain visible to the user.
    let _lock = LockFile::new(shims_dir).lock()?;

    #[cfg(windows)]
    validate_windows_shim_source(mise_bin)?;

    for shim in shims {
        let path = shims_dir.join(shim);
        if !path.exists()
            && let Err(err) = add_shim(mise_bin, &path, shim)
        {
            if is_permission_denied(&err) {
                return Ok(Some(err));
            }
            return Err(err);
        }
    }
    Ok(None)
}

#[cfg(windows)]
fn validate_windows_shim_source(mise_bin: &Path) -> Result<()> {
    match effective_shim_mode(mise_bin).as_ref() {
        "exe" => {
            let source =
                find_mise_shim_bin(mise_bin).ok_or_else(|| eyre!("mise-shim.exe not found"))?;
            fs::File::open(&source)
                .wrap_err_with(|| eyre!("Failed to open shim source {}", display_path(&source)))?;
        }
        "hardlink" => {
            fs::metadata(mise_bin).wrap_err_with(|| {
                eyre!("Failed to access shim source {}", display_path(mise_bin))
            })?;
        }
        _ => {}
    }
    Ok(())
}

fn is_permission_denied(err: &eyre::Report) -> bool {
    err.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            )
        })
    })
}

pub(crate) async fn reshim_for(
    config: &Arc<Config>,
    ts: &Toolset,
    force: bool,
    requested_scope: ShimScope,
) -> Result<()> {
    let user_shims = dirs::shims();
    let system_shims = dirs::system_shims();
    let collocated = file::storage_paths_eq(&user_shims, &system_shims);
    let scope = if collocated {
        ShimScope::Both
    } else {
        requested_scope
    };
    let shims_dir = match requested_scope {
        ShimScope::User | ShimScope::Both => user_shims,
        ShimScope::System => system_shims,
    };
    let _lock = LockFile::new(&shims_dir)
        .with_callback(|l| {
            trace!("reshim callback {}", l.display());
        })
        .lock();

    let mise_bin = mise_bin_for_shims();
    let mise_bin = mise_bin.absolutize()?; // relative paths don't work as shims

    #[cfg(windows)]
    let shim_mode = effective_shim_mode(&mise_bin);
    #[cfg(not(windows))]
    let shim_mode = String::new();
    let shim_mode_changed = cfg!(windows) && {
        let mode_file = shims_dir.join(".mode");
        mode_file
            .exists()
            .then(|| fs::read_to_string(&mode_file).unwrap_or_default())
            .is_some_and(|prev| prev.trim() != shim_mode)
    };
    // On Windows, "exe"/"hardlink" shims are literal copies of the mise(-shim)
    // binary, so they go stale when mise is updated (by self-update or an
    // external package manager) until a forced reshim. Track the mise version
    // that generated the shims in a `.version` marker (mirroring `.mode`) and
    // rebuild from scratch whenever it changes. The marker is written by
    // whichever binary runs reshim, so after an update the new binary stamps
    // the new version. See discussion #10022.
    let shim_version = env!("CARGO_PKG_VERSION");
    let shim_version_changed = cfg!(windows) && {
        let version_file = shims_dir.join(".version");
        let prev = fs::read_to_string(&version_file).ok();
        shim_version_stale(prev.as_deref(), shim_version, &shim_mode)
    };
    let full_rebuild = force || shim_mode_changed || shim_version_changed;
    file::create_dir_all(&shims_dir)?;

    let dedicated = is_dedicated_shims_dir(&shims_dir);
    let (desired, shims_to_stage, known_owned, prune_entries) = if full_rebuild {
        let desired = get_desired_shims(config, &mise_bin, ts, scope, force).await?;
        let shims_to_stage = desired.iter().cloned().collect();
        if dedicated {
            (desired, shims_to_stage, HashSet::new(), HashSet::new())
        } else {
            let actual = get_actual_shims(&mise_bin, &shims_dir).await?;
            let prune_entries = actual.owned.difference(&desired).cloned().collect();
            (desired, shims_to_stage, actual.owned, prune_entries)
        }
    } else {
        let diffs = get_shim_diffs(config, &mise_bin, ts, &shims_dir, scope, false).await?;
        (
            diffs.desired,
            diffs.missing,
            diffs.owned,
            diffs.extra.into_iter().collect(),
        )
    };
    let staging = stage_shim_farm(
        &shims_dir,
        &mise_bin,
        scope,
        &shims_to_stage,
        &shim_mode,
        shim_version,
    )
    .await?;
    publish_staged_shim_farm(
        &shims_dir,
        &mise_bin,
        staging,
        desired,
        known_owned,
        prune_entries,
        full_rebuild && dedicated,
    )?;

    if matches!(requested_scope, ShimScope::User | ShimScope::Both) {
        sync_command_wrapper_shims(config, &mise_bin, full_rebuild)?;
    }

    Ok(())
}

async fn stage_shim_farm(
    shims_dir: &Path,
    mise_bin: &Path,
    scope: ShimScope,
    desired: &BTreeSet<String>,
    shim_mode: &str,
    shim_version: &str,
) -> Result<tempfile::TempDir> {
    let staging = tempfile::Builder::new()
        .prefix(".mise-shims-stage-")
        .tempdir_in(shims_dir)
        .wrap_err_with(|| {
            format!(
                "failed to create shim staging directory in {}",
                display_path(shims_dir)
            )
        })?;
    write_shim_metadata(staging.path(), shim_mode, shim_version)?;
    for shim in desired {
        add_shim(mise_bin, &staging.path().join(shim), shim)?;
    }
    add_plugin_shims(staging.path(), scope).await?;
    Ok(staging)
}

fn write_shim_metadata(shims_dir: &Path, shim_mode: &str, shim_version: &str) -> Result<()> {
    if cfg!(windows) {
        // Written for every shim mode even though `.version` is only consulted
        // for exe/hardlink shims. Keeping both markers current makes later mode
        // transitions deterministic.
        file::write(shims_dir.join(".mode"), shim_mode)?;
        file::write(shims_dir.join(".version"), shim_version)?;
    }
    Ok(())
}

async fn add_plugin_shims(shims_dir: &Path, scope: ShimScope) -> Result<()> {
    if !matches!(scope, ShimScope::User | ShimScope::Both) {
        return Ok(());
    }
    let mut jset = JoinSet::new();
    for plugin in backend::list() {
        let shims_dir = shims_dir.to_path_buf();
        jset.spawn(async move {
            if let Ok(files) = dirs::PLUGINS.join(plugin.id()).join("shims").read_dir() {
                for bin in files {
                    let bin = bin?;
                    let bin_name = bin.file_name().into_string().unwrap();
                    let symlink_path = shims_dir.join(bin_name);
                    make_shim(&bin.path(), &symlink_path).await?;
                }
            }
            Ok(())
        });
    }
    jset.join_all()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    Ok(())
}

fn publish_staged_shim_farm(
    shims_dir: &Path,
    mise_bin: &Path,
    staging: tempfile::TempDir,
    mut desired: HashSet<String>,
    known_owned: HashSet<String>,
    prune_entries: HashSet<String>,
    prune_unmanaged: bool,
) -> Result<()> {
    let mut entries = staging
        .path()
        .read_dir()
        .wrap_err_with(|| {
            format!(
                "failed to read staged shim directory: {}",
                display_path(staging.path())
            )
        })?
        .collect::<std::io::Result<Vec<_>>>()?;
    // Publish metadata last. If publication is interrupted, an old marker makes
    // the next reshim retry instead of treating a partially published farm as
    // current.
    entries.sort_by_key(|entry| is_hidden_shim_name(&entry.file_name()));
    desired.extend(
        entries
            .iter()
            .filter_map(|entry| entry.file_name().into_string().ok()),
    );
    for entry in entries {
        let source = entry.path();
        let destination = shims_dir.join(entry.file_name());
        let destination_exists = destination.exists() || destination.is_symlink();
        let destination_owned = is_hidden_shim_name(&entry.file_name())
            || known_owned.contains(&entry.file_name().to_string_lossy().into_owned())
            || (destination_exists
                && (is_mise_shim(&destination, mise_bin)?
                    || symlink_target_names_mise(&destination)?));
        if destination_exists
            && !prune_unmanaged
            && !destination_owned
            && !files_identical(&source, &destination).unwrap_or(false)
        {
            warn!(
                "not replacing unmanaged file in shims directory: {}",
                display_path(&destination)
            );
            continue;
        }
        if cfg!(windows) && destination_exists {
            remove_shim_with_rename_fallback(&destination)?;
        }
        // Rename replaces a same-named file atomically. A publication error
        // therefore leaves that live shim untouched; an interrupted rebuild can
        // leave a mixture of old and new shims, but never evacuates the farm.
        fs::rename(&source, &destination).wrap_err_with(|| {
            format!(
                "failed to publish shim {} to {}",
                display_path(&source),
                display_path(&destination)
            )
        })?;
    }

    // A shared executable directory can contain arbitrary user and package
    // manager files. Only prune entries that can be identified as mise shims.
    // Mise's default, dedicated farms retain their historical full cleanup.
    for shim in list_shims_in(shims_dir)?.difference(&desired) {
        let path = shims_dir.join(shim);
        if prune_unmanaged || prune_entries.contains(shim) {
            if cfg!(windows) {
                remove_shim_with_rename_fallback(&path)?;
            } else {
                file::remove_all(&path)?;
            }
        }
    }

    if let Err(err) = staging.close() {
        warn!("failed to remove shim staging directory: {err}");
    }
    Ok(())
}

fn is_dedicated_shims_dir(path: &Path) -> bool {
    matches_unredirected_dedicated_dir(path, &dirs::DATA.join("shims"))
        || matches_unredirected_dedicated_dir(path, &env::MISE_SYSTEM_DATA_DIR.join("shims"))
}

fn matches_unredirected_dedicated_dir(path: &Path, dedicated: &Path) -> bool {
    if !file::paths_eq(path, dedicated) {
        return false;
    }
    let Some((parent, file_name)) = path.parent().zip(path.file_name()) else {
        return false;
    };
    match (dunce::canonicalize(path), dunce::canonicalize(parent)) {
        (Ok(resolved), Ok(resolved_parent)) => {
            file::paths_eq(&resolved, &resolved_parent.join(file_name))
        }
        _ => false,
    }
}

fn files_identical(a: &Path, b: &Path) -> Result<bool> {
    if a.is_symlink() || b.is_symlink() {
        return Ok(a.is_symlink() && b.is_symlink() && fs::read_link(a)? == fs::read_link(b)?);
    }
    if !a.is_file() || !b.is_file() {
        return Ok(false);
    }
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    Ok(fs::read(a)? == fs::read(b)?)
}

fn read_file_prefix(path: &Path) -> Result<Vec<u8>> {
    let mut contents = Vec::new();
    fs::File::open(path)?
        .take(SHIM_SCRIPT_INSPECTION_LIMIT)
        .read_to_end(&mut contents)?;
    Ok(contents)
}

fn is_mise_dispatcher_name(name: &str) -> bool {
    if cfg!(windows) {
        name.eq_ignore_ascii_case("mise") || name.eq_ignore_ascii_case("mise.exe")
    } else {
        name == "mise"
    }
}

fn resolved_symlink_target(path: &Path) -> Result<Option<PathBuf>> {
    if !path.is_symlink() {
        return Ok(None);
    }
    let target = fs::read_link(path)?;
    Ok(Some(if target.is_absolute() {
        target
    } else {
        path.parent().unwrap_or_else(|| Path::new(".")).join(target)
    }))
}

fn symlink_target_names_mise(path: &Path) -> Result<bool> {
    Ok(resolved_symlink_target(path)?.is_some_and(|target| {
        target
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_mise_dispatcher_name)
    }))
}

fn is_mise_shim(path: &Path, mise_bin: &Path) -> Result<bool> {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_mise_dispatcher_name)
    {
        // A package manager may install mise itself as a symlink in the shared
        // directory. It is the dispatcher, not one of its shims.
        return Ok(false);
    }
    if path.is_symlink() {
        let target = resolved_symlink_target(path)?.expect("symlink target");
        return Ok(file::paths_eq(
            &file::canonicalize_or_self(&target),
            &file::canonicalize_or_self(mise_bin),
        ));
    }

    #[cfg(windows)]
    {
        if !path.is_file() {
            return Ok(false);
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        if is_dedicated_shims_dir(parent) {
            // Preserve the existing upgrade behavior in mise's dedicated
            // farms. Native copies from an older mise cannot be identified by
            // their contents after mise-shim.exe changes. Use the same
            // unredirected check as pruning so a junction to a shared bin
            // directory never grants ownership of every regular file there.
            return Ok(true);
        }

        let is_script = path.extension().is_none()
            || path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"));
        let contents = if is_script {
            read_file_prefix(path).unwrap_or_default()
        } else {
            Vec::new()
        };
        if is_generated_shell_shim(&contents)
            || is_generated_windows_file_shim_contents(path, &contents)
        {
            return Ok(true);
        }
        let matches_mise = files_identical(path, mise_bin).unwrap_or(false);
        let matches_launcher = find_mise_shim_bin(mise_bin)
            .is_some_and(|launcher| files_identical(path, &launcher).unwrap_or(false));
        if matches_mise || matches_launcher {
            return Ok(true);
        }
        let contents = if is_script {
            contents
        } else {
            fs::read(path).unwrap_or_default()
        };
        return Ok(has_mise_native_shim_fingerprint(&contents));
    }

    #[cfg(not(windows))]
    {
        if !path.is_file() {
            return Ok(false);
        }
        Ok(is_generated_shell_shim(
            &read_file_prefix(path).unwrap_or_default(),
        ))
    }
}

#[cfg(any(windows, test))]
fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn is_generated_shell_shim(contents: &[u8]) -> bool {
    contents.starts_with(GENERATED_SHELL_SHIM_HEADER.as_bytes()) || is_legacy_plugin_shim(contents)
}

fn is_legacy_plugin_shim(contents: &[u8]) -> bool {
    let Ok(contents) = std::str::from_utf8(contents) else {
        return false;
    };
    let mut lines = contents.lines();
    lines.next() == Some("#!/bin/sh")
        && lines
            .next()
            .is_some_and(|line| line.starts_with("export ASDF_DATA_DIR=") && line.len() > 21)
        && lines
            .next()
            .is_some_and(|line| line.starts_with("export PATH=\"") && line.ends_with(":$PATH\""))
        && lines
            .next()
            .is_some_and(|line| line.starts_with("mise x -- ") && line.ends_with(" \"$@\""))
        && lines.next().is_none()
}

#[cfg(any(windows, test))]
fn has_mise_native_shim_fingerprint(contents: &[u8]) -> bool {
    bytes_contain(contents, NATIVE_SHIM_MARKER)
        // Transition shims made by mise versions predating the stable marker.
        || (bytes_contain(
            contents,
            b"mise-shim: failed to determine executable path",
        ) && bytes_contain(contents, b"mise-shim: failed to execute mise"))
        || (bytes_contain(contents, b"__MISE_SHIM_PATH")
            && bytes_contain(contents, b"recursive shim invocation detected")
            && bytes_contain(contents, b"mise x --"))
}

#[cfg(test)]
fn is_generated_windows_file_shim(path: &Path) -> bool {
    fs::read(path).is_ok_and(|contents| is_generated_windows_file_shim_contents(path, &contents))
}

#[cfg(any(windows, test))]
fn is_generated_windows_file_shim_contents(path: &Path, contents: &[u8]) -> bool {
    let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
        return false;
    };
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"))
    {
        contents.starts_with(GENERATED_WINDOWS_CMD_SHIM_HEADER.as_bytes())
            || contents == windows_file_shim_body(name).as_bytes()
            || is_legacy_windows_cmd_shim(contents)
    } else if path.extension().is_none() {
        if contents.starts_with(GENERATED_WINDOWS_BASH_SHIM_HEADER.as_bytes()) {
            return true;
        }
        #[cfg(windows)]
        return contents == bash_shim_script(name).as_bytes();
        #[cfg(not(windows))]
        return false;
    } else {
        false
    }
}

#[cfg(any(windows, test))]
fn is_legacy_windows_cmd_shim(contents: &[u8]) -> bool {
    let normalized = String::from_utf8_lossy(contents).replace("\r\n", "\n");
    matches!(
        normalized.as_str(),
        "@echo off\nsetlocal\nmise x -- %*\n" | "@echo off\nsetlocal\nmise x -- %*"
    )
}

fn sync_command_wrapper_shims(config: &Config, mise_bin: &Path, force: bool) -> Result<()> {
    let wrappers = load_command_wrappers(&config.config_files)?;
    validate_wrapper_names(wrappers.keys())?;
    if wrappers.is_empty() {
        if cfg!(windows) {
            remove_shims_individually(&dirs::COMMAND_WRAPPERS)?;
        } else {
            file::remove_all(&*dirs::COMMAND_WRAPPERS)?;
        }
        return Ok(());
    }
    if force {
        if cfg!(windows) {
            remove_shims_individually(&dirs::COMMAND_WRAPPERS)?;
        } else {
            file::remove_all(&*dirs::COMMAND_WRAPPERS)?;
        }
    }
    file::create_dir_all(&*dirs::COMMAND_WRAPPERS)?;

    let mut desired = HashSet::new();
    for name in wrappers.keys() {
        desired.extend(platform_shim_names(mise_bin, name));
    }
    let actual = list_shims_in(&dirs::COMMAND_WRAPPERS)?;
    for shim in desired.difference(&actual) {
        let path = dirs::COMMAND_WRAPPERS.join(shim);
        if cfg!(windows) && path.exists() {
            remove_shim_with_rename_fallback(&path)?;
        }
        add_shim(mise_bin, &path, shim)?;
    }
    for shim in actual.difference(&desired) {
        let path = dirs::COMMAND_WRAPPERS.join(shim);
        if cfg!(windows) {
            remove_shim_with_rename_fallback(&path)?;
        } else {
            file::remove_all(&path)?;
        }
    }
    Ok(())
}

fn command_names_eq(a: &str, b: &str) -> bool {
    if cfg!(macos) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

pub(crate) fn command_name_without_exe_suffix(bin_name: &str) -> &str {
    let suffix = std::env::consts::EXE_SUFFIX;
    if suffix.is_empty() {
        return bin_name;
    }
    let suffix_start = bin_name.len().saturating_sub(suffix.len());
    match (bin_name.get(..suffix_start), bin_name.get(suffix_start..)) {
        (Some(name), Some(actual_suffix)) if actual_suffix.eq_ignore_ascii_case(suffix) => name,
        _ => bin_name,
    }
}

fn validate_wrapper_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        bail!("invalid command wrapper name: {name:?}");
    }
    if cfg!(windows) && name.contains('.') {
        bail!("command wrapper names cannot contain dots on Windows: {name:?}");
    }
    Ok(())
}

fn validate_wrapper_names<'a>(names: impl IntoIterator<Item = &'a String>) -> Result<()> {
    let mut normalized = HashSet::new();
    for name in names {
        validate_wrapper_name(name)?;
        if cfg!(macos) && !normalized.insert(name.to_lowercase()) {
            bail!("command wrapper names collide on macOS after case normalization: {name:?}");
        }
    }
    Ok(())
}

/// Resolve the mise executable that Unix symlink shims should target.
///
/// Snap exposes applications through `/snap/bin`, where each command is a symlink to the
/// `snap` dispatcher. That dispatcher identifies the application from argv[0], so invoking it
/// through a mise shim named `node`, `python`, etc. runs the snap CLI instead of mise. Point Snap
/// shims at the payload beneath its refresh-stable `current` symlink instead. For other package
/// managers, retain the PATH-visible executable so their stable launcher survives upgrades.
pub(crate) fn mise_bin_for_shims() -> PathBuf {
    env::var_path("SNAP")
        .as_deref()
        .and_then(|snap| snap_mise_bin(&env::MISE_BIN, snap))
        .unwrap_or_else(|| file::which_no_shims("mise").unwrap_or(env::MISE_BIN.clone()))
}

fn snap_mise_bin(mise_bin: &Path, snap: &Path) -> Option<PathBuf> {
    let relative = mise_bin
        .strip_prefix(snap)
        .map(Path::to_path_buf)
        .or_else(|_| {
            let mise_bin = file::canonicalize_or_self(mise_bin);
            let snap = file::canonicalize_or_self(snap);
            mise_bin.strip_prefix(snap).map(Path::to_path_buf)
        })
        .ok()?;
    let snap_mount = snap.parent()?;
    Some(snap_mount.join("current").join(relative))
}

/// Remove all shim files from a directory individually, skipping dotfiles like
/// `.mode`. Uses [`remove_shim_with_rename_fallback`] for each entry so locked
/// `.exe` files on Windows are renamed out of the way instead of causing a
/// hard error.
fn remove_shims_individually(shims_dir: &Path) -> Result<()> {
    let entries = match shims_dir.read_dir() {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(e).wrap_err_with(|| {
                format!(
                    "failed to read shims directory: {}",
                    display_path(shims_dir)
                )
            });
        }
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        // skip dotfiles (e.g. .mode) — these are metadata, not shims
        if is_hidden_shim_name(&name) {
            continue;
        }
        let path = entry.path();
        remove_shim_with_rename_fallback(&path)?;
    }
    Ok(())
}

/// Remove a single shim file. On Windows, if deletion fails (e.g. because the
/// `.exe` is locked by another process), rename it to `<name>.old` so the path
/// is freed for a new shim. The `.old` file will be cleaned up on the next
/// reshim or when the lock is released.
fn remove_shim_with_rename_fallback(path: &Path) -> Result<()> {
    // First, try to clean up any leftover .old files from a previous run.
    let old_path = old_shim_path(path);
    if old_path.exists() {
        let _ = fs::remove_file(&old_path); // best-effort
    }

    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if cfg!(windows) && matches!(e.raw_os_error(), Some(5) | Some(32)) => {
            // ERROR_ACCESS_DENIED (5) or ERROR_SHARING_VIOLATION (32): file is
            // locked by another process, rename it instead.
            trace!(
                "cannot delete locked shim {}, renaming to .old",
                display_path(path)
            );
            fs::rename(path, &old_path).wrap_err_with(|| {
                format!(
                    "failed to rename locked shim {} to {}",
                    display_path(path),
                    display_path(&old_path)
                )
            })?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).wrap_err_with(|| format!("failed to remove shim: {}", display_path(path))),
    }
}

fn old_shim_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    path.with_file_name(name)
}

/// Find the `mise-shim.exe` that ships beside the real `mise_bin` — following
/// symlinks/junctions — if there is one.
///
/// Not `#[cfg(windows)]`, unlike the shim code around it: `mise generate task-stubs
/// --windows-launcher=exe` asks the same question, and on a host where the answer is always `None`
/// that is the honest answer to report rather than a compile error to route around.
pub(crate) fn find_mise_shim_bin(mise_bin: &Path) -> Option<PathBuf> {
    // mise-shim.exe ships beside the real mise.exe, which may sit behind a
    // symlink or junction (dunce avoids the `\\?\` verbatim prefix)
    let real_bin = dunce::canonicalize(mise_bin).unwrap_or_else(|_| mise_bin.to_path_buf());
    if let Some(parent) = real_bin.parent() {
        let candidate = parent.join("mise-shim.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // Fall back to searching PATH
    // Note: file::which on Windows checks extension only, not file existence,
    // so we must verify the file actually exists.
    file::which("mise-shim.exe").filter(|p| p.is_file())
}

/// Resolve the effective Windows shim mode, falling back to "file" if "exe" is
/// requested but mise-shim.exe is not available.
#[cfg(windows)]
fn effective_shim_mode(mise_bin: &Path) -> String {
    let mode = Settings::get().windows_shim_mode.clone();
    if mode == "exe" && find_mise_shim_bin(mise_bin).is_none() {
        // Once, not once per shim: this runs for every desired shim and for every shim written.
        warn_once!(
            "mise-shim.exe not found next to {} or on PATH, falling back to \"file\" shim mode",
            display_path(mise_bin)
        );
        return "file".to_string();
    }
    mode
}

/// Build the extension-less bash shim used on Windows in "file" mode (for Git
/// Bash/Cygwin). "exe" mode does not emit this — its native <tool>.exe is found
/// by those shells via `.exe` magic — so only "file" mode reaches this code.
///
/// The shim's directory can leak into WSL via the default Windows-PATH interop
/// (it is mounted under /mnt/c where every file is treated as executable), so WSL
/// runs this script natively. Calling the Windows `mise` from there either fails
/// with `exec: mise: not found` or, with a Linux mise present, recurses forever --
/// mise's loop guard only recognises its own shims dir, not the Windows one under
/// /mnt/c. So detect WSL, drop this shim's own directory from PATH, and exec a
/// native tool instead (or fail with a clear `<tool>: not found`). Outside WSL the
/// guard is inert, so Git Bash/Cygwin behaviour is unchanged. (#10299)
#[cfg(windows)]
fn bash_shim_script(tool: &str) -> String {
    formatdoc! {r#"
        #!/bin/bash
        # mise generated shim

        shim_dir=$(cd -- "$(dirname -- "$0")" && pwd -P)
        shim_path="$shim_dir/${{0##*/}}"
        if [ "${{__MISE_SHIM_PATH:-}}" = "$shim_path" ]; then
          echo "mise: recursive shim invocation detected for {tool}: $shim_path" >&2
          exit 1
        fi

        if [ -n "${{WSL_DISTRO_NAME:-}}" ] || [ -n "${{WSL_INTEROP:-}}" ] || [ -e /proc/sys/fs/binfmt_misc/WSLInterop ]; then
          new_path=
          # disable globbing so a PATH entry containing * ? [ is not expanded
          set -f
          IFS=:
          for p in $PATH; do
            [ "$p" = "$shim_dir" ] && continue
            new_path="${{new_path:+$new_path:}}$p"
          done
          unset IFS
          set +f
          export PATH="$new_path"
          exec {tool} "$@"
        fi

        export __MISE_SHIM_PATH="$shim_path"
        exec mise x -- {tool} "$@"
        "#}
}

/// The `.cmd` body for a `"file"`-mode shim: run `mise x -- <tool>` with the caller's arguments.
///
/// `%*` alone does not carry them. cmd.exe parses the whole command line before a batch file runs,
/// so calling one from PowerShell loses `& ^ | " < >` and expands `%VAR%` before `%*` ever expands.
/// Measured through a shim, every shape that arrives intact in `"exe"` mode arrives different here:
/// `c&d` runs `d` as a second command, `i^j` becomes `ij`, `a>b` writes a file called `b`, `e%OS%f`
/// becomes `eWindows_NTf`. The other three modes are native executables and are handed argv
/// directly, so this is the only mode that needs it.
///
/// The recovery is the one [`crate::cli::generate::windows_launcher_body`] documents in full: what
/// cmd destroys on the way in it also keeps, in its `CMDCMDLINE` pseudo-variable, so the body
/// copies that out — `!CMDCMDLINE!` rather than `%CMDCMDLINE%`, which is substituted before special
/// characters are parsed and truncates at the first `&` — and passes it through the environment,
/// where cmd gets no second chance to parse it. mise re-splits it with the rules a native program's
/// runtime uses and substitutes the result for everything after [`env::LAUNCHER_ARGS_SENTINEL`].
///
/// The guard decides whether cmd was spawned *for* this shim. When it was not — an interactive
/// prompt, a `call` from another batch file — the shell already split the arguments as it would for
/// a native program, so `%*` is correct and the script must not `exit`, which would close the
/// caller's shell. When it was, `exit` (rather than `exit /b`) also stops cmd running whatever it
/// queued from the same line, which would otherwise be reported as the tool's exit code.
///
/// Kept separate from the launcher body rather than shared: this one runs its recursion guard
/// first, and the launcher's line layout is pinned by `is_generated_launcher`, which has no
/// business tracking shim changes. The names come from the same constants, and
/// `the_shim_body_carries_the_same_recovery` fails if the shapes drift.
///
/// Compiled for tests on every platform, unlike the shim code around it, so the body itself is
/// unit-tested everywhere; `pub(crate)` so that comparison can live beside the launcher.
#[cfg(any(windows, test))]
pub(crate) fn windows_file_shim_body(shim: &str) -> String {
    let raw = env::LAUNCHER_RAW_CMDLINE_ENV;
    let path = env::LAUNCHER_PATH_ENV;
    let sentinel = env::LAUNCHER_ARGS_SENTINEL;
    let shim_env = env::MISE_SHIM_PATH_ENV;
    let run = format!("mise x -- {shim} {sentinel} %*");
    [
        "@echo off",
        "rem mise generated shim",
        // `%~f0` is captured before delayed expansion is on, so a `!` in the path survives.
        "setlocal DisableDelayedExpansion",
        "set \"shim_path=%~f0\"",
        &format!("if /I \"%{shim_env}%\"==\"%shim_path%\" ("),
        &format!("  echo mise: recursive shim invocation detected for {shim}: %shim_path% 1>&2"),
        "  exit /b 1",
        ")",
        &format!("set \"{shim_env}=%shim_path%\""),
        "setlocal EnableDelayedExpansion",
        &format!("set \"{path}=!shim_path!\""),
        &format!("set \"{raw}=!CMDCMDLINE!\""),
        &format!("if \"!{raw}!\"==\"!{raw}:%{path}%=!\" goto mise_shim_fallback"),
        &run,
        "exit !ERRORLEVEL!",
        ":mise_shim_fallback",
        // Cleared, or a value inherited from an outer launcher would be recovered as this one's.
        &format!("set \"{raw}=\""),
        &format!("set \"{path}=\""),
        &run,
    ]
    .join("\r\n")
        + "\r\n"
}

#[cfg(windows)]
fn add_shim(mise_bin: &Path, symlink_path: &Path, shim: &str) -> Result<()> {
    match effective_shim_mode(mise_bin).as_ref() {
        "exe" => {
            // In "exe" mode every desired shim is a native <tool>.exe copy of
            // mise-shim.exe (see get_desired_shims). No extension-less bash shim is
            // emitted: Git Bash / Cygwin resolve a bare name to the .exe via their
            // `.exe` magic, so emitting one is redundant and only pollutes WSL via
            // /mnt/c PATH interop (#10299).
            let mise_shim_bin =
                find_mise_shim_bin(mise_bin).ok_or_else(|| eyre!("mise-shim.exe not found"))?;
            // Copy mise-shim.exe as <tool>.exe
            fs::copy(&mise_shim_bin, symlink_path).wrap_err_with(|| {
                eyre!(
                    "Failed to copy {} to {}",
                    display_path(&mise_shim_bin),
                    display_path(symlink_path)
                )
            })?;
            Ok(())
        }
        "file" => {
            let shim = shim.trim_end_matches(".cmd");
            // write a shim file without extension for use in Git Bash/Cygwin
            file::write(symlink_path.with_extension(""), bash_shim_script(shim)).wrap_err_with(
                || {
                    eyre!(
                        "Failed to create symlink from {} to {}",
                        display_path(mise_bin),
                        display_path(symlink_path)
                    )
                },
            )?;
            file::write(
                symlink_path.with_extension("cmd"),
                windows_file_shim_body(shim),
            )
            .wrap_err_with(|| {
                eyre!(
                    "Failed to create symlink from {} to {}",
                    display_path(mise_bin),
                    display_path(symlink_path)
                )
            })
        }
        "hardlink" => fs::hard_link(mise_bin, symlink_path).wrap_err_with(|| {
            eyre!(
                "Failed to create hardlink from {} to {}",
                display_path(mise_bin),
                display_path(symlink_path)
            )
        }),
        "symlink" => {
            std::os::windows::fs::symlink_file(mise_bin, symlink_path).wrap_err_with(|| {
                eyre!(
                    "Failed to create symlink from {} to {}",
                    display_path(mise_bin),
                    display_path(symlink_path)
                )
            })
        }
        _ => panic!("Unknown shim mode"),
    }
}

#[cfg(unix)]
fn add_shim(mise_bin: &Path, symlink_path: &Path, _shim: &str) -> Result<()> {
    file::make_symlink(mise_bin, symlink_path).wrap_err_with(|| {
        eyre!(
            "Failed to create symlink from {} to {}",
            display_path(mise_bin),
            display_path(symlink_path)
        )
    })?;
    Ok(())
}

pub(crate) struct ShimDiffs {
    pub missing: BTreeSet<String>,
    pub extra: BTreeSet<String>,
    pub desired: HashSet<String>,
    owned: HashSet<String>,
}

struct ActualShims {
    current: HashSet<String>,
    dedicated_present: HashSet<String>,
    owned: HashSet<String>,
    occupied: HashSet<String>,
    repairable: HashSet<String>,
}

fn calculate_shim_diffs(
    actual: &ActualShims,
    desired: &HashSet<String>,
    dedicated: bool,
) -> (BTreeSet<String>, BTreeSet<String>) {
    // In a shared executable directory, a same-named unmanaged entry is an
    // intentional collision, not a missing shim that reshim can repair. Treat
    // it as occupied so doctor and automatic post-install reshims stay quiet.
    let (missing, extra) = if dedicated {
        (
            desired
                .difference(&actual.dedicated_present)
                .cloned()
                .collect(),
            actual
                .dedicated_present
                .difference(desired)
                .cloned()
                .collect(),
        )
    } else {
        (
            desired
                .iter()
                .filter(|name| {
                    !actual.current.contains(*name)
                        && (!actual.occupied.contains(*name)
                            || actual.owned.contains(*name)
                            || actual.repairable.contains(*name))
                })
                .cloned()
                .collect(),
            actual.owned.difference(desired).cloned().collect(),
        )
    };
    (missing, extra)
}

// get_shim_diffs contrasts the actual shims on disk
// with the desired shims specified by the Toolset
pub(crate) async fn get_shim_diffs(
    config: &Arc<Config>,
    mise_bin: impl AsRef<Path>,
    toolset: &Toolset,
    shims_dir: &Path,
    scope: ShimScope,
    strict_lazy_bins: bool,
) -> Result<ShimDiffs> {
    let mise_bin = mise_bin.as_ref();
    let (actual_shims, desired_shims) = tokio::join!(
        get_actual_shims(mise_bin, shims_dir),
        get_desired_shims(config, mise_bin, toolset, scope, strict_lazy_bins)
    );
    let (actual_shims, desired_shims) = (actual_shims?, desired_shims?);
    let (missing, extra) = calculate_shim_diffs(
        &actual_shims,
        &desired_shims,
        is_dedicated_shims_dir(shims_dir),
    );
    time!("get_shim_diffs sizes: ({},{})", missing.len(), extra.len());
    Ok(ShimDiffs {
        missing,
        extra,
        desired: desired_shims,
        owned: actual_shims.owned,
    })
}

async fn get_actual_shims(mise_bin: impl AsRef<Path>, shims_dir: &Path) -> Result<ActualShims> {
    let mise_bin = mise_bin.as_ref();
    let occupied = list_shims_in(shims_dir)?;
    let mut current = HashSet::new();
    let mut dedicated_present = HashSet::new();
    let mut owned = HashSet::new();
    let mut repairable = HashSet::new();
    for bin in &occupied {
        let path = shims_dir.join(bin);
        if is_mise_shim(&path, mise_bin).unwrap_or(false) {
            owned.insert(bin.clone());
            if is_current_owned_mise_shim(&path, mise_bin).unwrap_or(false) {
                current.insert(bin.clone());
            }
        } else if symlink_target_names_mise(&path).unwrap_or(false) {
            repairable.insert(bin.clone());
        }
        if !path.is_symlink() || current.contains(bin) {
            dedicated_present.insert(bin.clone());
        }
    }
    Ok(ActualShims {
        current,
        dedicated_present,
        owned,
        occupied,
        repairable,
    })
}

fn is_current_owned_mise_shim(path: &Path, mise_bin: &Path) -> Result<bool> {
    if !path.is_symlink() {
        return Ok(true);
    }
    let target = resolved_symlink_target(path)?.expect("symlink target");
    // Raw path equality is deliberate. A shim that points through to the same
    // binary but bypasses the stable package-manager launcher must be migrated
    // before the versioned path disappears during an upgrade.
    Ok(file::paths_eq(&target, mise_bin))
}

fn list_executables_in_dir(dir: &Path) -> Result<HashSet<String>> {
    Ok(dir
        .read_dir()?
        .map(|bin| {
            let bin = bin?;
            let name = bin.file_name();
            if is_hidden_shim_name(&name) {
                return Ok(None);
            }
            // files and symlinks which are executable
            if file::is_executable(&bin.path())
                && (bin.file_type()?.is_file() || bin.file_type()?.is_symlink())
            {
                Ok(name.into_string().ok())
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn list_shims_in(dir: &Path) -> Result<HashSet<String>> {
    Ok(dir
        .read_dir()?
        .map(|bin| {
            let bin = bin?;
            let name = bin.file_name();
            // skip dotfiles (e.g. .mode) — these are metadata, not shims
            if is_hidden_shim_name(&name) {
                return Ok(None);
            }
            // files and symlinks which are executable or extensionless files (Git Bash/Cygwin)
            if (file::is_executable(&bin.path()) || bin.path().extension().is_none())
                && (bin.file_type()?.is_file() || bin.file_type()?.is_symlink())
            {
                Ok(name.into_string().ok())
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn is_hidden_shim_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// Whether existing shims were generated by a different mise version AND the
/// current shim mode produces version-dependent shim files. "exe"/"hardlink"
/// embed a literal copy of the mise/mise-shim binary; "file" writes a bash script
/// whose contents are baked into the mise binary as well (e.g. the WSL guard added
/// in #10299), so all three must rebuild on a version change to pick up script
/// changes — otherwise a normal reshim leaves the old script in place. "symlink"
/// only points at the mise binary (no embedded content), so it is never
/// version-stale. `prev == None` (no `.version` marker yet) heals installs that
/// predate the marker by forcing a one-time rebuild. See discussions #10022 and
/// #10299.
fn shim_version_stale(prev: Option<&str>, current: &str, shim_mode: &str) -> bool {
    if !matches!(shim_mode, "exe" | "hardlink" | "file") {
        return false;
    }
    prev.map(|p| p.trim() != current).unwrap_or(true)
}

fn shim_scope_contains_install(scope: ShimScope, install_path: &Path) -> bool {
    if scope == ShimScope::Both {
        return true;
    }
    let system_installs = Settings::get().system_installs_dir().to_path_buf();
    if file::storage_paths_eq(&system_installs, &dirs::INSTALLS) {
        return true;
    }
    let is_system = install_path.starts_with(system_installs);
    matches!(scope, ShimScope::System) == is_system
}

fn shim_scope_contains_request(scope: ShimScope, request: &crate::toolset::ToolRequest) -> bool {
    if scope == ShimScope::Both {
        return true;
    }
    let is_system = request.source().path().is_some_and(|path| {
        crate::config::provenance::ConfigProvenance::from_path(path).scope()
            == crate::config::provenance::ConfigFileScope::System
    });
    matches!(scope, ShimScope::System) == is_system
}

async fn get_desired_shims(
    config: &Arc<Config>,
    mise_bin: &Path,
    toolset: &Toolset,
    scope: ShimScope,
    strict_lazy_bins: bool,
) -> Result<HashSet<String>> {
    let _mise_bin = mise_bin; // used on Windows only
    let mut shims = HashSet::new();
    for (t, tv) in toolset.list_installed_versions(config).await? {
        if !shim_scope_contains_install(scope, &tv.install_path()) {
            continue;
        }
        let bins = list_tool_bins(config, t.clone(), &tv)
            .await
            .unwrap_or_else(|e| {
                warn!("Error listing bin paths for {}: {:#}", tv, e);
                Vec::new()
            });
        shims.extend(
            bins.into_iter()
                .flat_map(|b| platform_shim_names(_mise_bin, &b)),
        );
    }
    for request in toolset.list_current_requests() {
        if !shim_scope_contains_request(scope, request) {
            continue;
        }
        match request.lazy_bins() {
            Ok(Some(bins)) => shims.extend(
                bins.into_iter()
                    .flat_map(|bin| platform_shim_names(_mise_bin, &bin)),
            ),
            Ok(None) => {}
            Err(err) if strict_lazy_bins => return Err(err),
            Err(err) => warn!("Skipping invalid lazy shim declaration: {err:#}"),
        }
    }
    Ok(shims)
}

fn platform_shim_names(_mise_bin: &Path, bin: &str) -> Vec<String> {
    if cfg!(windows) {
        #[cfg(windows)]
        let shim_mode = effective_shim_mode(_mise_bin);
        #[cfg(not(windows))]
        let shim_mode = String::new();
        let p = PathBuf::from(bin);
        match shim_mode.as_ref() {
            "hardlink" | "symlink" | "exe" => {
                vec![p.with_extension("exe").to_string_lossy().to_string()]
            }
            "file" => vec![
                p.with_extension("").to_string_lossy().to_string(),
                p.with_extension("cmd").to_string_lossy().to_string(),
            ],
            _ => panic!("Unknown shim mode"),
        }
    } else if cfg!(macos) {
        vec![bin.to_lowercase()]
    } else {
        vec![bin.to_string()]
    }
}

// lists all the paths to bins in a tv that shims will be needed for
async fn list_tool_bins(
    config: &Arc<Config>,
    t: Arc<dyn Backend>,
    tv: &ToolVersion,
) -> Result<Vec<String>> {
    Ok(t.list_bin_paths(config, tv)
        .await?
        .into_iter()
        .filter(|p| p.parent().is_some())
        .filter(|path| path.exists())
        .map(|dir| list_executables_in_dir(&dir))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

async fn make_shim(target: &Path, shim: &Path) -> Result<()> {
    file::remove_file_async_if_exists(shim).await?;
    file::write_async(
        shim,
        format!(
            "{GENERATED_SHELL_SHIM_HEADER}export ASDF_DATA_DIR={data_dir}\nexport PATH=\"{fake_asdf_dir}:$PATH\"\nmise x -- {target} \"$@\"\n",
        data_dir = dirs::DATA.display(),
        fake_asdf_dir = fake_asdf::setup()?.display(),
        target = target.display()
        ),
    )
    .await?;
    file::make_executable_async(shim).await?;
    trace!(
        "shim created from {} to {}",
        target.display(),
        shim.display()
    );
    Ok(())
}

async fn err_no_version_set(
    config: &Arc<Config>,
    ts: Toolset,
    bin_name: &str,
    tvs: Vec<ToolVersion>,
) -> Result<PathBuf> {
    if tvs.is_empty() {
        bail!(
            "{bin_name} is not a valid shim. This likely means you uninstalled a tool and the shim does not point to anything. Run `mise use <TOOL>` to reinstall the tool."
        );
    }
    let missing_plugins = tvs.iter().map(|tv| tv.ba()).collect::<HashSet<_>>();
    let mut missing_tools = ts
        .list_missing_versions(config)
        .await
        .into_iter()
        .filter(|t| missing_plugins.contains(t.ba()))
        .collect_vec();
    if missing_tools.is_empty() {
        if let Some(msg) = unavailable_configured_tool_message(config, &ts, bin_name) {
            return Err(eyre!(msg));
        }
        let mut msg = format!("No version is set for shim: {bin_name}\n");
        msg.push_str("Set a global default version with one of the following:\n");
        for tv in tvs {
            msg.push_str(&format!("mise use -g {}@{}\n", tv.ba(), tv.version));
        }
        Err(eyre!(msg.trim().to_string()))
    } else {
        let mut msg = format!(
            "Tool{} not installed for shim: {}\n",
            if missing_tools.len() > 1 { "s" } else { "" },
            bin_name
        );
        for t in missing_tools.drain(..) {
            msg.push_str(&format!("Missing tool version: {t}\n"));
        }
        msg.push_str("Install all missing tools with: mise install\n");
        Err(eyre!(msg.trim().to_string()))
    }
}

pub(crate) fn unavailable_configured_tool_message(
    config: &Arc<Config>,
    ts: &Toolset,
    bin_name: &str,
) -> Option<String> {
    let versions = ts
        .list_current_versions()
        .into_iter()
        .filter(|(backend, tv)| {
            tv.ba().matches_bin_name(bin_name) && backend.is_version_installed(config, tv, true)
        })
        .map(|(_, tv)| tv)
        .collect_vec();
    if versions.is_empty() {
        return None;
    }

    let mut msg = format!("No executable found for configured tool: {bin_name}\n");
    msg.push_str(
        "The installed version does not provide this executable with its current backend metadata.\n",
    );
    msg.push_str("Reinstall it with:\n");
    for tv in versions {
        msg.push_str(&format!(
            "mise install --force {}@{}\n",
            tv.ba(),
            tv.version
        ));
    }
    Some(msg.trim().to_string())
}

/// `mise install <tool>` deliberately writes to no config file, so the tool's
/// bin dir never joins the PATH `mise exec` builds and resolution fails with an
/// opaque error (`cannot find binary path` on Windows, `couldn't exec process`
/// on unix). When an installed-but-unconfigured tool would have supplied the
/// bin, name it and say how to activate it. See discussion #4407.
///
/// Returns `None` when a configured tool already matches the bin: that failure
/// has a different cause, and `err_no_version_set` /
/// `unavailable_configured_tool_message` already explain it.
///
/// Matching is by tool name, so a bin that shares no name with the tool
/// providing it (`npm` from `node`) is not recognized. Those callers fall back
/// to the existing message rather than getting a wrong one.
pub(crate) fn inactive_installed_tool_message(
    ts: &Toolset,
    installed_shorts: &[String],
    bin_name: &str,
) -> Option<String> {
    // Every tool declared in config is a key here, even one that failed version
    // resolution, is unsupported on this OS, or whose backend could not be
    // built. `list_current_versions()` drops all three, which would let a
    // configured tool be reported as "not in any config file".
    if ts.versions.keys().any(|ba| ba.matches_bin_name(bin_name)) {
        return None;
    }
    let shorts = installed_shorts
        .iter()
        .filter(|short| BackendArg::from(short.as_str()).matches_bin_name(bin_name))
        .collect_vec();
    if shorts.is_empty() {
        return None;
    }
    let mut msg =
        format!("{bin_name} is installed but not activated — it is not in any config file.\n");
    msg.push_str("To activate it, run:\n");
    for short in &shorts {
        msg.push_str(&format!("  mise use {short}\n"));
    }
    msg.push_str("To run it without changing any config file, run:\n");
    for short in &shorts {
        msg.push_str(&format!("  mise exec {short} -- {bin_name}\n"));
    }
    Some(msg.trim().to_string())
}

/// Name the registry tool that provides `bin_name` on other platforms but not this one.
///
/// `registry/<tool>.toml` carries an `os` list, and a tool whose list omits the running OS is
/// dropped by `ToolRequestSetBuilder::is_disabled` before any version is resolved. That drop is
/// silent by design — the tool is not unknown, and the user has not disabled it — so nothing is
/// installed and the bin is simply absent. `mise install` and `mise use` reach the backend and
/// report the reason; `mise exec` only ever saw the missing bin. mise has the answer in its own
/// registry, so say it rather than leaving the user with `cannot find binary path`.
///
/// Matched on `bins` and on the tool's own name, so `mise x aws-cli -- aws` is recognised through
/// either. Returns `None` when the OS list is empty or includes this one, which is the normal case.
pub(crate) fn os_unsupported_tool_message(bin_name: &str) -> Option<String> {
    let shorts = crate::registry::REGISTRY
        .values()
        .unique_by(|rt| rt.short)
        .filter(|rt| !rt.is_supported_os())
        .filter(|rt| rt.short == bin_name || rt.bins.contains(&bin_name))
        .map(|rt| (rt.short, rt.os.join(", ")))
        .collect_vec();
    if shorts.is_empty() {
        return None;
    }
    let mut msg = String::new();
    for (short, oses) in &shorts {
        let provides = if *short == bin_name {
            String::new()
        } else {
            format!(", which provides {bin_name},")
        };
        msg.push_str(&format!(
            "{short}{provides} is not available on {}: mise's registry lists it for {oses} only.\n",
            std::env::consts::OS,
        ));
    }
    Some(msg.trim().to_string())
}

/// Gather what [`inactive_installed_tool_message`] needs. Only called once a
/// binary has definitively failed to resolve, so the config/toolset load lands
/// on a path that is about to abort anyway.
#[cfg(not(test))]
pub(crate) async fn exec_resolution_hint(bin_name: &str) -> Option<String> {
    // Windows invokes binaries as `<tool>.exe`; name `<tool>` in the message.
    let bin_stem = bin_name
        .strip_suffix(std::env::consts::EXE_SUFFIX)
        .unwrap_or(bin_name);
    let config = Config::get().await.ok()?;
    let ts = ToolsetBuilder::new().build(&config).await.ok()?;
    // A disabled tool is skipped by `Toolset::add_version`, so it never reaches
    // the configured-tool check above. Suggesting `mise use` for one would be
    // wrong twice over: it may well be in a config file, and mise has been told
    // not to manage it.
    let settings = Settings::get();
    let enable_tools = settings.enable_tools();
    let disable_tools = settings.disable_tools();
    let installed_shorts = crate::toolset::install_state::list_tools()
        .values()
        .filter(|t| !t.versions.is_empty())
        .map(|t| t.short.clone())
        .filter(|short| crate::registry::tool_enabled(enable_tools.as_ref(), &disable_tools, short))
        .collect_vec();
    inactive_installed_tool_message(&ts, &installed_shorts, bin_stem)
        // Checked second because the two cannot both apply: a tool this OS is excluded from is
        // never installed here, so there is nothing to be "installed but not activated".
        .or_else(|| os_unsupported_tool_message(bin_stem))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendArg;
    use crate::toolset::{ToolRequest, ToolSource, ToolVersionList};

    #[test]
    fn locked_windows_shims_get_distinct_old_paths() {
        assert_eq!(old_shim_path(Path::new("foo")), PathBuf::from("foo.old"));
        assert_eq!(
            old_shim_path(Path::new("foo.cmd")),
            PathBuf::from("foo.cmd.old")
        );
    }

    #[cfg(macos)]
    #[test]
    fn case_colliding_macos_wrapper_names_are_rejected() {
        let names = ["Foo".to_string(), "foo".to_string()];
        assert!(validate_wrapper_names(&names).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn dotted_windows_wrapper_names_are_rejected() {
        let names = ["foo.bar".to_string()];
        assert!(validate_wrapper_names(&names).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_shim_command_names_drop_exe_suffix_case_insensitively() {
        assert_eq!(command_name_without_exe_suffix("dummy.exe"), "dummy");
        assert_eq!(command_name_without_exe_suffix("DUMMY.EXE"), "DUMMY");
        assert_eq!(
            command_name_without_exe_suffix("python3.12.exe"),
            "python3.12"
        );
        assert_eq!(command_name_without_exe_suffix("dummy.cmd"), "dummy.cmd");
    }

    #[test]
    fn snap_mise_bin_uses_refresh_stable_current_path() {
        assert_eq!(
            snap_mise_bin(
                Path::new("/snap/mise/189/bin/mise"),
                Path::new("/snap/mise/189")
            ),
            Some(PathBuf::from("/snap/mise/current/bin/mise"))
        );
    }

    #[test]
    fn snap_mise_bin_rejects_unrelated_executable() {
        assert_eq!(
            snap_mise_bin(
                Path::new("/home/user/.local/bin/mise"),
                Path::new("/snap/mise/189")
            ),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn snap_mise_bin_handles_symlinked_snap_mount() {
        let temp = tempfile::tempdir().unwrap();
        let canonical_mount = temp.path().join("var/lib/snapd/snap");
        let canonical_snap = canonical_mount.join("mise/189");
        let mise_bin = canonical_snap.join("bin/mise");
        fs::create_dir_all(mise_bin.parent().unwrap()).unwrap();
        fs::write(&mise_bin, "").unwrap();

        let snap_mount = temp.path().join("snap");
        std::os::unix::fs::symlink(&canonical_mount, &snap_mount).unwrap();
        let snap = snap_mount.join("mise/189");

        assert_eq!(
            snap_mise_bin(&mise_bin, &snap),
            Some(snap_mount.join("mise/current/bin/mise"))
        );
    }

    #[tokio::test]
    async fn unavailable_tool_message_prefers_matching_configured_tool() {
        let config = Config::get().await.unwrap();
        let temp = tempfile::tempdir().unwrap();
        let mut ts = Toolset::new(ToolSource::Argument);

        for name in ["codex", "node"] {
            let ba = Arc::new(BackendArg::from(name));
            let request = ToolRequest::new(ba.clone(), "1.0.0", ToolSource::Argument).unwrap();
            let mut tv = ToolVersion::new(request.clone(), "1.0.0".into());
            let install_path = temp.path().join(name);
            file::create_dir_all(&install_path).unwrap();
            tv.install_path = Some(install_path);

            let mut tvl = ToolVersionList::new(ba.clone(), ToolSource::Argument);
            tvl.requests.push(request);
            tvl.versions.push(tv);
            ts.versions.insert(ba, tvl);
        }

        let msg = unavailable_configured_tool_message(&config, &ts, "codex").unwrap();
        assert!(msg.contains("mise install --force codex@1.0.0"));
        assert!(!msg.contains("node@1.0.0"));
    }

    #[tokio::test]
    async fn inactive_tool_message_names_the_installed_tool() {
        let _config = Config::get().await.unwrap();
        let ts = Toolset::new(ToolSource::Argument);

        let msg = inactive_installed_tool_message(&ts, &["gh".to_string()], "gh").unwrap();
        assert!(msg.contains("installed but not activated"));
        assert!(msg.contains("mise use gh"));
        assert!(msg.contains("mise exec gh -- gh"));
    }

    #[tokio::test]
    async fn inactive_tool_message_is_none_for_configured_tool() {
        let _config = Config::get().await.unwrap();
        let mut ts = Toolset::new(ToolSource::Argument);
        let ba = Arc::new(BackendArg::from("gh"));
        let request = ToolRequest::new(ba.clone(), "1.0.0", ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request.clone(), "1.0.0".into());
        let mut tvl = ToolVersionList::new(ba.clone(), ToolSource::Argument);
        tvl.requests.push(request);
        tvl.versions.push(tv);
        ts.versions.insert(ba, tvl);

        // A configured tool that still fails to resolve has a different cause;
        // err_no_version_set/unavailable_configured_tool_message own that case.
        assert!(inactive_installed_tool_message(&ts, &["gh".to_string()], "gh").is_none());
    }

    /// A tool can be declared in config and still be absent from
    /// `list_current_versions()` -- version resolution failed, the OS is not
    /// supported, or its backend could not be built. It must not then be
    /// reported as "not in any config file".
    #[tokio::test]
    async fn inactive_tool_message_is_none_for_a_configured_tool_with_no_resolved_versions() {
        let _config = Config::get().await.unwrap();
        let mut ts = Toolset::new(ToolSource::Argument);
        let ba = Arc::new(BackendArg::from("gh"));
        ts.versions.insert(
            ba.clone(),
            ToolVersionList::new(ba, ToolSource::Argument), // no versions resolved
        );

        assert!(inactive_installed_tool_message(&ts, &["gh".to_string()], "gh").is_none());
    }

    #[tokio::test]
    async fn inactive_tool_message_is_none_when_bin_does_not_name_the_tool() {
        let _config = Config::get().await.unwrap();
        let ts = Toolset::new(ToolSource::Argument);

        // Matching is by tool name, so a bin like npm (provided by node) is not
        // recognized and the caller keeps its existing message.
        assert!(inactive_installed_tool_message(&ts, &["node".to_string()], "npm").is_none());
    }

    #[test]
    fn os_unsupported_tool_message_names_the_tool_and_this_platform() {
        // Taken from the registry rather than hardcoded: which tools are excluded depends on the
        // platform the test runs on, and a hardcoded name would pass for the wrong reason
        // wherever it happens to be supported. Every platform has some — the registry carries
        // `["macos"]`, `["linux"]` and `["windows"]` entries.
        let rt = crate::registry::REGISTRY
            .values()
            .unique_by(|rt| rt.short)
            .find(|rt| !rt.is_supported_os() && !rt.bins.is_empty())
            .expect("the registry lists no tool this platform is excluded from");

        let msg = os_unsupported_tool_message(rt.bins[0])
            .expect("a bin only an excluded tool provides should be explained");
        assert!(msg.contains(rt.short), "{msg}");
        assert!(msg.contains(std::env::consts::OS), "{msg}");
        // and it must say where the tool *is* available, or the user learns nothing actionable
        assert!(msg.contains(rt.os[0]), "{msg}");
    }

    #[test]
    fn os_unsupported_tool_message_is_silent_when_nothing_is_excluded() {
        // The control. Without it a message that fired for every name would satisfy the test
        // above. `node` carries no `os` list, so it is supported everywhere.
        assert_eq!(os_unsupported_tool_message("node"), None);
        assert_eq!(os_unsupported_tool_message("not-a-registry-bin-9f3a"), None);
    }

    // `e2e-win/exec_os_unsupported_tool.Tests.ps1` observes this message by running
    // `mise x docker-slim -- mint` on a Windows runner, and it can only observe it while
    // `docker-slim` is still restricted away from Windows and still provides a bin under another
    // name. A registry edit that takes either away leaves that file green with nothing to assert,
    // and a full Windows e2e run to notice. The same strings are pinned here so `windows-unit`
    // fails first, saying which one went.
    //
    // Windows-only because everywhere else `docker-slim` is supported and `None` is the right
    // answer, so the assertions could not run at all.
    #[cfg(windows)]
    #[test]
    fn os_unsupported_tool_message_still_backs_the_windows_e2e_fixture() {
        let msg = os_unsupported_tool_message("mint")
            .expect("docker-slim provides mint and its os list omits windows");
        for expected in [
            "docker-slim",
            "not available on windows",
            "mint",
            "linux",
            "macos",
        ] {
            assert!(msg.contains(expected), "{expected:?} missing from {msg:?}");
        }
    }

    #[test]
    fn windows_file_shim_body_recovers_the_arguments_cmd_destroys() {
        let body = windows_file_shim_body("gh");
        let lines: Vec<&str> = body.lines().collect();
        let enable = lines
            .iter()
            .position(|l| l.contains("EnableDelayedExpansion"))
            .unwrap();

        // `%~f0` is captured before delayed expansion is on, or a `!` in the path would be eaten.
        let capture = lines.iter().position(|l| l.contains("%~f0")).unwrap();
        assert!(capture < enable, "{body}");

        // `!CMDCMDLINE!`, not `%CMDCMDLINE%`: the percent form is substituted before special
        // characters are parsed and truncates the line at the first `&`.
        assert!(
            body.contains(r#"set "__MISE_RAW_CMDLINE=!CMDCMDLINE!""#),
            "{body}"
        );
        assert!(!body.contains("%CMDCMDLINE%"), "{body}");

        // The recursion guard is unchanged, and still decides before anything else runs.
        let guard = lines
            .iter()
            .position(|l| l.contains("%__MISE_SHIM_PATH%"))
            .unwrap();
        assert!(guard < enable, "{body}");
        assert!(
            body.contains("recursive shim invocation detected for gh"),
            "{body}"
        );
        assert!(body.contains("exit /b 1"), "{body}");

        // Both arms hand mise the same command; the sentinel marks where `%*` begins, so a run
        // that cannot recover the raw line still gets what cmd managed to deliver.
        let run = format!("mise x -- gh {} %*", env::LAUNCHER_ARGS_SENTINEL);
        assert_eq!(lines.iter().filter(|l| **l == run).count(), 2, "{body}");

        // The recovering arm exits rather than `exit /b`, so cmd does not go on to run whatever it
        // queued from the same line -- given `gh c&d`, that is `d`.
        assert!(body.contains("goto mise_shim_fallback"), "{body}");
        assert!(body.contains("exit !ERRORLEVEL!"), "{body}");
    }

    #[test]
    fn windows_file_shim_body_clears_the_launcher_variables_when_it_declines() {
        let body = windows_file_shim_body("gh");
        let after: Vec<&str> = body
            .lines()
            .skip_while(|l| *l != ":mise_shim_fallback")
            .collect();
        assert!(!after.is_empty(), "{body}");
        // Or a value inherited from an outer launcher would be recovered as this shim's arguments.
        assert!(after.contains(&r#"set "__MISE_RAW_CMDLINE=""#), "{body}");
        assert!(after.contains(&r#"set "__MISE_LAUNCHER=""#), "{body}");
    }

    #[test]
    fn windows_file_shim_body_is_crlf_terminated() {
        // A label reached by `goto` is the classic thing a lone `\n` breaks in a batch file.
        let body = windows_file_shim_body("gh");
        assert!(body.ends_with("\r\n"));
        assert_eq!(body.matches('\n').count(), body.matches("\r\n").count());
    }

    #[test]
    fn windows_file_shim_detection_uses_stable_markers_and_legacy_body() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("gh.cmd");
        fs::write(&shim, windows_file_shim_body("gh")).unwrap();
        assert!(is_generated_windows_file_shim(&shim));

        fs::write(
            &shim,
            format!("{GENERATED_WINDOWS_CMD_SHIM_HEADER}echo future body\r\n"),
        )
        .unwrap();
        assert!(is_generated_windows_file_shim(&shim));

        fs::write(&shim, "@echo off\r\nsetlocal\r\nmise x -- %*\r\n").unwrap();
        assert!(is_generated_windows_file_shim(&shim));

        fs::write(&shim, "@echo off\r\necho user script\r\n").unwrap();
        assert!(!is_generated_windows_file_shim(&shim));

        let bash_shim = dir.path().join("gh");
        fs::write(
            &bash_shim,
            format!("{GENERATED_WINDOWS_BASH_SHIM_HEADER}echo future body\n"),
        )
        .unwrap();
        assert!(is_generated_windows_file_shim(&bash_shim));
    }

    #[cfg(windows)]
    #[test]
    fn bash_shim_script_includes_wsl_guard() {
        let script = bash_shim_script("gh");
        assert!(script.starts_with("#!/bin/bash"));
        // WSL detection
        assert!(script.contains("WSL_DISTRO_NAME"));
        assert!(script.contains("WSL_INTEROP"));
        assert!(script.contains("/proc/sys/fs/binfmt_misc/WSLInterop"));
        assert!(script.contains(r#"shim_dir=$(cd -- "$(dirname -- "$0")" && pwd -P)"#));
        // globbing disabled while splitting PATH so wildcard entries are not expanded
        assert!(script.contains("set -f"));
        // The shim identifies its actual location even if MISE_DATA_DIR is absent.
        assert!(script.contains(r#"shim_path="$shim_dir/${0##*/}""#));
        assert!(script.contains(r#"export __MISE_SHIM_PATH="$shim_path""#));
        assert!(script.contains("recursive shim invocation detected"));
        // In WSL: drop the shim dir and run the native tool directly.
        assert!(script.contains(r#"exec gh "$@""#));
        // Outside WSL: defer to mise as before.
        assert!(script.contains(r#"exec mise x -- gh "$@""#));
    }

    #[test]
    fn list_executables_in_dir_skips_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        let visible_name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let visible = dir.path().join(visible_name);
        let hidden = dir.path().join(".librsvg-post-link.exe");

        fs::write(&visible, "").unwrap();
        fs::write(&hidden, "").unwrap();
        file::make_executable(&visible).unwrap();
        file::make_executable(&hidden).unwrap();

        let bins = list_executables_in_dir(dir.path()).unwrap();

        assert!(bins.contains(visible_name));
        assert!(!bins.contains(".librsvg-post-link.exe"));
    }

    #[test]
    fn staged_shim_publication_preserves_unmanaged_and_modified_files() {
        let live = tempfile::tempdir().unwrap();
        let mise_bin = live.path().join("mise");
        fs::write(&mise_bin, "mise").unwrap();
        let unmanaged = live.path().join("unmanaged");
        fs::write(&unmanaged, "from another package manager").unwrap();
        file::make_executable(&unmanaged).unwrap();

        let staging = tempfile::Builder::new()
            .prefix(".mise-shims-stage-")
            .tempdir_in(live.path())
            .unwrap();
        fs::write(staging.path().join("unmanaged"), "mise shim").unwrap();
        fs::write(
            staging.path().join("owned"),
            "#!/bin/sh\n# mise generated shim\nmise x -- owned \"$@\"\n",
        )
        .unwrap();
        publish_staged_shim_farm(
            live.path(),
            &mise_bin,
            staging,
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
            false,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(&unmanaged).unwrap(),
            "from another package manager"
        );
        assert_eq!(
            fs::read_to_string(live.path().join("owned")).unwrap(),
            "#!/bin/sh\n# mise generated shim\nmise x -- owned \"$@\"\n"
        );

        // Once an owned shim is changed outside mise, an otherwise empty
        // reshim cedes ownership instead of deleting the replacement.
        fs::write(live.path().join("owned"), "user replacement").unwrap();
        let empty_staging = tempfile::Builder::new()
            .prefix(".mise-shims-stage-")
            .tempdir_in(live.path())
            .unwrap();
        publish_staged_shim_farm(
            live.path(),
            &mise_bin,
            empty_staging,
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
            false,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(live.path().join("owned")).unwrap(),
            "user replacement"
        );
    }

    #[test]
    fn staged_shim_publication_removes_unchanged_owned_shims() {
        let live = tempfile::tempdir().unwrap();
        let mise_bin = live.path().join("mise");
        fs::write(&mise_bin, "mise").unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".mise-shims-stage-")
            .tempdir_in(live.path())
            .unwrap();
        fs::write(
            staging.path().join("owned"),
            "#!/bin/sh\n# mise generated shim\nmise x -- owned \"$@\"\n",
        )
        .unwrap();
        publish_staged_shim_farm(
            live.path(),
            &mise_bin,
            staging,
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
            false,
        )
        .unwrap();

        let empty_staging = tempfile::Builder::new()
            .prefix(".mise-shims-stage-")
            .tempdir_in(live.path())
            .unwrap();
        publish_staged_shim_farm(
            live.path(),
            &mise_bin,
            empty_staging,
            HashSet::new(),
            HashSet::from(["owned".to_string()]),
            HashSet::from(["owned".to_string()]),
            false,
        )
        .unwrap();

        assert!(!live.path().join("owned").exists());
    }

    #[test]
    fn staged_shim_publication_prunes_unmanaged_files_in_dedicated_farm() {
        let live = tempfile::tempdir().unwrap();
        let mise_bin = live.path().join("mise");
        fs::write(&mise_bin, "mise").unwrap();
        let orphan = live.path().join("orphan");
        fs::write(&orphan, "not a mise shim").unwrap();
        file::make_executable(&orphan).unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".mise-shims-stage-")
            .tempdir_in(live.path())
            .unwrap();
        let collision = live.path().join("collision");
        fs::write(&collision, "unmanaged old file").unwrap();
        fs::write(staging.path().join("collision"), "replacement shim").unwrap();

        publish_staged_shim_farm(
            live.path(),
            &mise_bin,
            staging,
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
            true,
        )
        .unwrap();

        assert!(!orphan.exists());
        assert_eq!(fs::read_to_string(collision).unwrap(), "replacement shim");
    }

    #[test]
    fn staged_shim_publication_updates_metadata_in_shared_directory() {
        let live = tempfile::tempdir().unwrap();
        let mise_bin = live.path().join("mise");
        fs::write(&mise_bin, "mise").unwrap();
        fs::write(live.path().join(".version"), "old").unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".mise-shims-stage-")
            .tempdir_in(live.path())
            .unwrap();
        fs::write(staging.path().join(".version"), "new").unwrap();

        publish_staged_shim_farm(
            live.path(),
            &mise_bin,
            staging,
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
            false,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(live.path().join(".version")).unwrap(),
            "new"
        );
    }

    #[test]
    fn shared_collision_is_not_missing_or_extra() {
        let actual = ActualShims {
            current: HashSet::new(),
            dedicated_present: HashSet::new(),
            owned: HashSet::new(),
            occupied: HashSet::from(["black".to_string()]),
            repairable: HashSet::new(),
        };
        let desired = HashSet::from(["black".to_string()]);

        let (missing, extra) = calculate_shim_diffs(&actual, &desired, false);

        assert!(missing.is_empty());
        assert!(extra.is_empty());
    }

    #[test]
    fn dedicated_diff_reports_the_same_entries_incremental_pruning_removes() {
        let actual = ActualShims {
            current: HashSet::from(["node".to_string()]),
            dedicated_present: HashSet::from(["node".to_string(), "orphan".to_string()]),
            owned: HashSet::from(["node".to_string()]),
            occupied: HashSet::from([
                "node".to_string(),
                "orphan".to_string(),
                "foreign-symlink".to_string(),
            ]),
            repairable: HashSet::new(),
        };
        let desired = HashSet::from(["node".to_string()]);

        let (missing, extra) = calculate_shim_diffs(&actual, &desired, true);

        assert!(missing.is_empty());
        assert_eq!(extra, BTreeSet::from(["orphan".to_string()]));
    }

    #[test]
    fn dangling_desired_mise_symlink_remains_missing_in_shared_directory() {
        let actual = ActualShims {
            current: HashSet::new(),
            dedicated_present: HashSet::new(),
            owned: HashSet::new(),
            occupied: HashSet::from(["node".to_string()]),
            repairable: HashSet::from(["node".to_string()]),
        };
        let desired = HashSet::from(["node".to_string()]);

        let (missing, extra) = calculate_shim_diffs(&actual, &desired, false);

        assert_eq!(missing, BTreeSet::from(["node".to_string()]));
        assert!(extra.is_empty());
    }

    #[test]
    fn stale_owned_shim_is_missing_in_shared_directory() {
        let actual = ActualShims {
            current: HashSet::new(),
            dedicated_present: HashSet::new(),
            owned: HashSet::from(["node".to_string()]),
            occupied: HashSet::from(["node".to_string()]),
            repairable: HashSet::new(),
        };
        let desired = HashSet::from(["node".to_string()]);

        let (missing, extra) = calculate_shim_diffs(&actual, &desired, false);

        assert_eq!(missing, BTreeSet::from(["node".to_string()]));
        assert!(extra.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_dedicated_farm_is_treated_as_shared() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared");
        let dedicated = root.path().join("shims");
        fs::create_dir(&shared).unwrap();
        std::os::unix::fs::symlink(&shared, &dedicated).unwrap();

        assert!(!matches_unredirected_dedicated_dir(&dedicated, &dedicated));
        assert!(matches_unredirected_dedicated_dir(&shared, &shared));
    }

    #[test]
    fn staged_shim_publication_preserves_unstaged_desired_shims() {
        let live = tempfile::tempdir().unwrap();
        let mise_bin = live.path().join("mise");
        fs::write(&mise_bin, "mise").unwrap();
        let existing = live.path().join("existing");
        fs::write(
            &existing,
            "#!/bin/sh\n# mise generated shim\nmise x -- existing \"$@\"\n",
        )
        .unwrap();
        file::make_executable(&existing).unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".mise-shims-stage-")
            .tempdir_in(live.path())
            .unwrap();

        publish_staged_shim_farm(
            live.path(),
            &mise_bin,
            staging,
            HashSet::from(["existing".to_string()]),
            HashSet::from(["existing".to_string()]),
            HashSet::new(),
            false,
        )
        .unwrap();

        assert!(existing.exists());
    }

    #[cfg(unix)]
    #[test]
    fn mise_shim_detection_distinguishes_symlink_targets() {
        let dir = tempfile::tempdir().unwrap();
        let mise_bin = dir.path().join("mise");
        let other_bin = dir.path().join("other");
        fs::write(&mise_bin, "mise").unwrap();
        fs::write(&other_bin, "other").unwrap();
        let shim = dir.path().join("shim");

        std::os::unix::fs::symlink(&mise_bin, &shim).unwrap();
        assert!(is_mise_shim(&shim, &mise_bin).unwrap());

        fs::remove_file(&shim).unwrap();
        std::os::unix::fs::symlink(&other_bin, &shim).unwrap();
        assert!(!is_mise_shim(&shim, &mise_bin).unwrap());

        // A dangling target name alone is not proof of ownership: an unrelated
        // symlink in a shared directory may also point at a file named `mise`.
        // Publication uses this weaker signal only for a desired collision.
        fs::remove_file(&shim).unwrap();
        std::os::unix::fs::symlink(dir.path().join("old/bin/mise"), &shim).unwrap();
        assert!(!is_mise_shim(&shim, &mise_bin).unwrap());
        assert!(symlink_target_names_mise(&shim).unwrap());
        assert!(!is_current_owned_mise_shim(&shim, &mise_bin).unwrap());

        fs::remove_file(&shim).unwrap();
        std::os::unix::fs::symlink(dir.path().join("scripts/mise-wrapper.sh"), &shim).unwrap();
        assert!(!is_mise_shim(&shim, &mise_bin).unwrap());
        assert!(!symlink_target_names_mise(&shim).unwrap());

        // The real mise dispatcher may itself be installed as a symlink in a
        // shared bin directory; its own name keeps it out of shim pruning.
        let dispatcher = dir.path().join("mise");
        fs::remove_file(&dispatcher).unwrap();
        std::os::unix::fs::symlink(&other_bin, &dispatcher).unwrap();
        assert!(!is_mise_shim(&dispatcher, &mise_bin).unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn current_shim_check_migrates_to_stable_launcher_path() {
        let dir = tempfile::tempdir().unwrap();
        let versioned = dir.path().join("Cellar/mise/1/bin/mise");
        fs::create_dir_all(versioned.parent().unwrap()).unwrap();
        fs::write(&versioned, "mise").unwrap();
        let launcher = dir.path().join("bin/mise");
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&versioned, &launcher).unwrap();
        let shim = dir.path().join("shims/node");
        fs::create_dir_all(shim.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&versioned, &shim).unwrap();

        assert!(is_mise_shim(&shim, &launcher).unwrap());
        assert!(!is_current_owned_mise_shim(&shim, &launcher).unwrap());

        let actual = get_actual_shims(&launcher, shim.parent().unwrap())
            .await
            .unwrap();
        let desired = HashSet::from(["node".to_string()]);
        let (missing, extra) = calculate_shim_diffs(&actual, &desired, false);
        assert_eq!(missing, BTreeSet::from(["node".to_string()]));
        assert!(extra.is_empty());
    }

    #[test]
    fn mise_shim_detection_recognizes_current_and_legacy_plugin_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let mise_bin = dir.path().join("mise");
        fs::write(&mise_bin, "mise").unwrap();
        let shim = dir.path().join("shim");

        fs::write(
            &shim,
            format!("{GENERATED_SHELL_SHIM_HEADER}mise x -- foo \"$@\"\n"),
        )
        .unwrap();
        assert!(is_mise_shim(&shim, &mise_bin).unwrap());

        fs::write(
            &shim,
            "#!/bin/sh\nexport ASDF_DATA_DIR=/tmp/mise\necho user-wrapper\nmise x -- foo \"$@\"\n",
        )
        .unwrap();
        assert!(!is_mise_shim(&shim, &mise_bin).unwrap());

        fs::write(
            &shim,
            "#!/bin/sh\nexport ASDF_DATA_DIR=/tmp/mise\nexport PATH=\"x:$PATH\"\nmise x -- foo \"$@\"\n",
        )
        .unwrap();
        assert!(is_mise_shim(&shim, &mise_bin).unwrap());

        fs::write(&shim, "#!/bin/sh\necho user-script\n").unwrap();
        assert!(!is_mise_shim(&shim, &mise_bin).unwrap());
    }

    #[test]
    fn native_shim_fingerprint_recognizes_current_and_transition_binaries() {
        assert!(has_mise_native_shim_fingerprint(
            b"PE\0mise generated native shim v1\n\0"
        ));
        assert!(has_mise_native_shim_fingerprint(
            b"mise-shim: failed to determine executable path\0mise-shim: failed to execute mise"
        ));
        assert!(has_mise_native_shim_fingerprint(
            b"__MISE_SHIM_PATH\0recursive shim invocation detected\0mise x --"
        ));
        assert!(!has_mise_native_shim_fingerprint(
            b"an unrelated executable mentioning mise x --"
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn list_executables_in_dir_skips_non_utf8_names() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir().unwrap();
        let non_utf8 = dir.path().join(OsString::from_vec(vec![0xff]));

        fs::write(&non_utf8, "").unwrap();
        file::make_executable(&non_utf8).unwrap();

        let bins = list_executables_in_dir(dir.path()).unwrap();

        assert!(bins.is_empty());
    }

    #[test]
    fn shim_version_stale_detects_version_changes() {
        // exe/hardlink copies embed the binary: a version change makes them stale
        assert!(shim_version_stale(Some("2026.5.13"), "2026.5.16", "exe"));
        assert!(shim_version_stale(
            Some("2026.5.13"),
            "2026.5.16",
            "hardlink"
        ));
        // file mode writes a versioned bash script (e.g. the WSL guard, #10299),
        // so a version change must rebuild it too
        assert!(shim_version_stale(Some("2026.5.13"), "2026.5.16", "file"));
        // matching version is not stale
        assert!(!shim_version_stale(Some("2026.5.16"), "2026.5.16", "exe"));
        assert!(!shim_version_stale(Some("2026.5.16"), "2026.5.16", "file"));
        // surrounding whitespace in the marker is ignored
        assert!(!shim_version_stale(Some("2026.5.16\n"), "2026.5.16", "exe"));
        // no marker yet: heal once (covers installs created before this marker)
        assert!(shim_version_stale(None, "2026.5.16", "exe"));
        assert!(shim_version_stale(None, "2026.5.16", "file"));
        // symlink shims only point at the mise binary, so never version-stale
        assert!(!shim_version_stale(
            Some("2026.5.13"),
            "2026.5.16",
            "symlink"
        ));
    }

    /// Create a file symlink, or return `false` when the platform refuses
    /// (e.g. Windows without the symlink privilege) so the caller can skip
    /// itself. Any other failure panics: swallowing it would silently turn the
    /// new coverage into a no-op.
    fn try_symlink_file(target: &Path, link: &Path) -> bool {
        let result = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(target, link)
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_file(target, link)
            }
        };
        match result {
            Ok(()) => true,
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
                ) =>
            {
                false
            }
            Err(err) => panic!("failed to create file symlink: {err}"),
        }
    }

    /// A single-link mise layout: the PATH-visible `mise.exe` is a link, and
    /// `mise-shim.exe` ships only beside the real binary. Returns the linked
    /// mise and the real shim, or `None` when symlinks cannot be created on
    /// this host.
    fn single_link_layout(temp: &Path) -> Option<(PathBuf, PathBuf)> {
        let real_dir = temp.join("real").join("bin");
        let links_dir = temp.join("links");
        fs::create_dir_all(&real_dir).unwrap();
        fs::create_dir_all(&links_dir).unwrap();
        let real_mise = real_dir.join("mise.exe");
        fs::write(&real_mise, "mise").unwrap();
        let real_shim = real_dir.join("mise-shim.exe");
        fs::write(&real_shim, "mise-shim").unwrap();
        let linked_mise = links_dir.join("mise.exe");
        if !try_symlink_file(&real_mise, &linked_mise) {
            return None;
        }
        Some((linked_mise, real_shim))
    }

    #[test]
    fn find_mise_shim_bin_finds_the_shim_beside_the_binary() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("mise.exe"), "mise").unwrap();
        let shim = bin.join("mise-shim.exe");
        fs::write(&shim, "mise-shim").unwrap();

        let found = find_mise_shim_bin(&bin.join("mise.exe")).expect("shim beside the binary");
        assert_eq!(
            dunce::canonicalize(&found).unwrap(),
            dunce::canonicalize(&shim).unwrap()
        );
    }

    #[test]
    fn find_mise_shim_bin_follows_a_symlinked_mise_bin() {
        let temp = tempfile::tempdir().unwrap();
        let Some((linked_mise, real_shim)) = single_link_layout(temp.path()) else {
            return;
        };

        // Regression: the old lookup checked only beside the link and on PATH.
        let found = find_mise_shim_bin(&linked_mise).expect("shim beside the real binary");
        assert_eq!(
            dunce::canonicalize(&found).unwrap(),
            dunce::canonicalize(&real_shim).unwrap()
        );
    }

    #[test]
    fn find_mise_shim_bin_follows_chained_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let Some((linked_mise, real_shim)) = single_link_layout(temp.path()) else {
            return;
        };
        let redirect_dir = temp.path().join("redirect");
        fs::create_dir_all(&redirect_dir).unwrap();
        let redirected = redirect_dir.join("mise.exe");
        if !try_symlink_file(&linked_mise, &redirected) {
            return;
        }

        let found = find_mise_shim_bin(&redirected).expect("shim through two links");
        assert_eq!(
            dunce::canonicalize(&found).unwrap(),
            dunce::canonicalize(&real_shim).unwrap()
        );
    }

    #[test]
    fn find_mise_shim_bin_prefers_the_shim_beside_the_real_binary() {
        let temp = tempfile::tempdir().unwrap();
        let Some((linked_mise, real_shim)) = single_link_layout(temp.path()) else {
            return;
        };
        let beside_link = temp.path().join("links").join("mise-shim.exe");
        fs::write(&beside_link, "another shim").unwrap();

        let found = find_mise_shim_bin(&linked_mise).expect("a shim beside the real binary");
        assert_eq!(
            dunce::canonicalize(&found).unwrap(),
            dunce::canonicalize(&real_shim).unwrap()
        );
        assert_ne!(
            dunce::canonicalize(&found).unwrap(),
            dunce::canonicalize(&beside_link).unwrap()
        );
    }

    #[test]
    fn find_mise_shim_bin_returns_none_when_no_shim_exists() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let mise = bin.join("mise.exe");
        fs::write(&mise, "mise").unwrap();

        // A mise-shim.exe on PATH happens in dev environments that build it, so
        // the assertion is scoped to what this test controls. `file::which` on
        // Windows matches the extension only, so filter to existing files the
        // same way the resolver does before deciding to skip.
        let found = find_mise_shim_bin(&mise);
        if file::which("mise-shim.exe")
            .filter(|p| p.is_file())
            .is_none()
        {
            assert_eq!(found, None);
        }
    }
}
