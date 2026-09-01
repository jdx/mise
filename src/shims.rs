use crate::request_exit;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{
    collections::{BTreeSet, HashSet},
    sync::atomic::Ordering,
};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cli::exec::Exec;
use crate::config::{CommandWrapper, Config, Settings, load_command_wrappers};
use crate::file::display_path;
use crate::lock_file::LockFile;
use crate::toolset::{ResolveOptions, ToolVersion, Toolset, ToolsetBuilder};
use crate::{backend, dirs, env, fake_asdf, file};
use color_eyre::eyre::{Result, bail, eyre};
use eyre::WrapErr;
use indoc::formatdoc;
use itertools::Itertools;
use path_absolutize::Absolutize;
use tokio::task::JoinSet;

#[cfg(any(windows, test))]
const NATIVE_SHIM_MARKER: &[u8] = b"mise generated native shim v1";

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
    let bin_stem = bin_name
        .strip_suffix(std::env::consts::EXE_SUFFIX)
        .unwrap_or(bin_name);
    let completion_offline =
        bin_stem == "usage" && args.get(1).is_some_and(|arg| arg == "complete-word");
    let resolve_options = if completion_offline {
        ResolveOptions {
            offline: true,
            ..Default::default()
        }
    } else {
        ResolveOptions::default()
    };
    let mut ts = ToolsetBuilder::new()
        .with_resolve_options(resolve_options)
        .build(config)
        .await?;
    let wrappers = load_command_wrappers(&config.config_files)?;
    validate_wrapper_names(wrappers.keys())?;
    let wrapper = if cfg!(macos) {
        wrappers
            .iter()
            .find(|(name, _)| command_names_eq(name, bin_stem))
            .map(|(_, wrapper)| wrapper)
    } else {
        wrappers.get(bin_stem)
    };
    if let Some(wrapper) = wrapper {
        if command_names_eq(wrapper.command(), bin_stem) {
            bail!("command wrapper for {bin_stem} cannot delegate to itself");
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
            .should_install_missing_registry_bin_provider(config, bin_name)
            .await?
    {
        for tv in ts
            .install_missing_bin(config, bin_name)
            .await?
            .unwrap_or_default()
        {
            let p = tv.backend()?;
            if let Some(bin) = p.which(config, &tv, bin_name).await? {
                trace!(
                    "shim[{bin_name}] REGISTRY ToolVersion: {tv} bin: {bin}",
                    bin = display_path(&bin)
                );
                return Ok((bin, ts, None));
            }
        }
    }
    if let Some((p, tv)) = ts.which(config, bin_name).await
        && let Some(bin) = p.which(config, &tv, bin_name).await?
    {
        trace!(
            "shim[{bin_name}] ToolVersion: {tv} bin: {bin}",
            bin = display_path(&bin)
        );
        return Ok((bin, ts, None));
    }
    // Lazy tools are explicit fallback providers. They install on first shim use even when
    // general not-found auto-install is disabled, but only after configured/project providers
    // and already-installed tools have had a chance to win.
    if !completion_offline && ts.has_missing_lazy_bin_provider(config, bin_name).await? {
        for tv in ts
            .install_missing_lazy_bin(config, bin_name)
            .await?
            .unwrap_or_default()
        {
            let backend = tv.backend()?;
            if let Some(bin) = backend.which(config, &tv, bin_name).await? {
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
            .install_missing_bin(config, bin_name)
            .await?
            .unwrap_or_default()
        {
            let p = tv.backend()?;
            if let Some(bin) = p.which(config, &tv, bin_name).await? {
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
    let tvs = ts.list_rtvs_with_bin(config, bin_name).await?;
    match err_no_version_set(config, ts, bin_name, tvs).await {
        Ok(_) => unreachable!("err_no_version_set always returns an error"),
        Err(err) => Err(err),
    }
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

    // Always materialize the complete farm first. Besides keeping publication
    // atomic per shim, this gives us the exact content used to decide whether a
    // same-named entry in a shared directory is ours or a collision.
    let desired = get_desired_shims(config, &mise_bin, ts, scope).await?;
    let staging = stage_shim_farm(
        &shims_dir,
        &mise_bin,
        scope,
        &desired.into_iter().collect::<BTreeSet<_>>(),
        &shim_mode,
        shim_version,
    )
    .await?;
    publish_staged_shim_farm(&shims_dir, &mise_bin, staging)?;

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
    let desired = entries
        .iter()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<HashSet<_>>();
    for entry in entries {
        let source = entry.path();
        let destination = shims_dir.join(entry.file_name());
        if (destination.exists() || destination.is_symlink())
            && !is_mise_shim(&destination, mise_bin)?
            && !files_identical(&source, &destination)?
        {
            warn!(
                "not replacing unmanaged file in shims directory: {}",
                display_path(&destination)
            );
            continue;
        }
        if cfg!(windows) && (destination.exists() || destination.is_symlink()) {
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
    for shim in list_shims_in(shims_dir)?.difference(&desired) {
        let path = shims_dir.join(shim);
        if is_mise_shim(&path, mise_bin)? {
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

fn files_identical(a: &Path, b: &Path) -> Result<bool> {
    if a.is_symlink() || b.is_symlink() {
        return Ok(a.is_symlink() && b.is_symlink() && fs::read_link(a)? == fs::read_link(b)?);
    }
    if !a.is_file() || !b.is_file() {
        return Ok(false);
    }
    Ok(fs::read(a)? == fs::read(b)?)
}

fn is_mise_shim(path: &Path, mise_bin: &Path) -> Result<bool> {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(env::is_mise_binary)
    {
        // A package manager may install mise itself as a symlink in the shared
        // directory. It is the dispatcher, not one of its shims.
        return Ok(false);
    }
    if path.is_symlink() {
        let target = fs::read_link(path)?;
        let target = if target.is_absolute() {
            target
        } else {
            path.parent().unwrap_or_else(|| Path::new(".")).join(target)
        };
        let target_names_mise = target
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(env::is_mise_binary);
        return Ok(target_names_mise
            || file::paths_eq(
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
        let is_default_farm = file::storage_paths_eq(parent, &dirs::DATA.join("shims"))
            || file::storage_paths_eq(parent, &env::MISE_SYSTEM_DATA_DIR.join("shims"));
        if is_default_farm {
            // Preserve the existing upgrade behavior in mise's dedicated
            // farms. Native copies from an older mise cannot be identified by
            // their contents after mise-shim.exe changes.
            return Ok(true);
        }

        let contents = fs::read(path).unwrap_or_default();
        if is_generated_shell_shim(&contents)
            || is_generated_windows_file_shim_contents(path, &contents)
            || has_mise_native_shim_fingerprint(&contents)
        {
            return Ok(true);
        }
        let matches_mise = files_identical(path, mise_bin).unwrap_or(false);
        let matches_launcher = find_mise_shim_bin(mise_bin)
            .is_some_and(|launcher| files_identical(path, &launcher).unwrap_or(false));
        return Ok(matches_mise || matches_launcher);
    }

    #[cfg(not(windows))]
    {
        if !path.is_file() {
            return Ok(false);
        }
        Ok(is_generated_shell_shim(&fs::read(path).unwrap_or_default()))
    }
}

fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn is_generated_shell_shim(contents: &[u8]) -> bool {
    contents.starts_with(b"#!/bin/sh\n# mise generated shim\n")
        || (contents.starts_with(b"#!/bin/sh\nexport ASDF_DATA_DIR=")
            && bytes_contain(contents, b"\nmise x -- "))
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
        contents == windows_file_shim_body(name).as_bytes()
    } else if path.extension().is_none() {
        #[cfg(windows)]
        return contents == bash_shim_script(name).as_bytes();
        #[cfg(not(windows))]
        return false;
    } else {
        false
    }
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

/// Find the `mise-shim.exe` that ships beside `mise_bin`, if there is one.
///
/// Not `#[cfg(windows)]`, unlike the shim code around it: `mise generate task-stubs
/// --windows-launcher=exe` asks the same question, and on a host where the answer is always `None`
/// that is the honest answer to report rather than a compile error to route around.
pub(crate) fn find_mise_shim_bin(mise_bin: &Path) -> Option<PathBuf> {
    // Look next to the mise binary first
    if let Some(parent) = mise_bin.parent() {
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
}

// get_shim_diffs contrasts the actual shims on disk
// with the desired shims specified by the Toolset
pub(crate) async fn get_shim_diffs(
    config: &Arc<Config>,
    mise_bin: impl AsRef<Path>,
    toolset: &Toolset,
    shims_dir: &Path,
    scope: ShimScope,
) -> Result<ShimDiffs> {
    let mise_bin = mise_bin.as_ref();
    let (actual_shims, desired_shims) = tokio::join!(
        get_actual_shims(mise_bin, shims_dir),
        get_desired_shims(config, mise_bin, toolset, scope)
    );
    let (actual_shims, desired_shims) = (actual_shims?, desired_shims?);
    let missing: BTreeSet<_> = desired_shims.difference(&actual_shims).cloned().collect();
    let extra: BTreeSet<_> = actual_shims.difference(&desired_shims).cloned().collect();
    time!("get_shim_diffs sizes: ({},{})", missing.len(), extra.len());
    Ok(ShimDiffs {
        missing,
        extra,
        desired: desired_shims,
    })
}

async fn get_actual_shims(mise_bin: impl AsRef<Path>, shims_dir: &Path) -> Result<HashSet<String>> {
    let mise_bin = mise_bin.as_ref();

    Ok(list_shims_in(shims_dir)?
        .into_iter()
        .filter(|bin| is_mise_shim(&shims_dir.join(bin), mise_bin).unwrap_or(false))
        .collect::<HashSet<_>>())
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
        if let Some(bins) = request.lazy_bins()? {
            shims.extend(
                bins.into_iter()
                    .flat_map(|bin| platform_shim_names(_mise_bin, &bin)),
            );
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
        formatdoc! {r#"
        #!/bin/sh
        # mise generated shim
        export ASDF_DATA_DIR={data_dir}
        export PATH="{fake_asdf_dir}:$PATH"
        mise x -- {target} "$@"
        "#,
        data_dir = dirs::DATA.display(),
        fake_asdf_dir = fake_asdf::setup()?.display(),
        target = target.display()},
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
    fn windows_file_shim_detection_requires_the_exact_generated_body() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("gh.cmd");
        fs::write(&shim, windows_file_shim_body("gh")).unwrap();
        assert!(is_generated_windows_file_shim(&shim));

        fs::write(&shim, "@echo off\r\necho user script\r\n").unwrap();
        assert!(!is_generated_windows_file_shim(&shim));
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
        publish_staged_shim_farm(live.path(), &mise_bin, staging).unwrap();

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
        publish_staged_shim_farm(live.path(), &mise_bin, empty_staging).unwrap();

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
        publish_staged_shim_farm(live.path(), &mise_bin, staging).unwrap();

        let empty_staging = tempfile::Builder::new()
            .prefix(".mise-shims-stage-")
            .tempdir_in(live.path())
            .unwrap();
        publish_staged_shim_farm(live.path(), &mise_bin, empty_staging).unwrap();

        assert!(!live.path().join("owned").exists());
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

        // A dangling shim left after mise moved can still be identified by its
        // target name and repaired.
        fs::remove_file(&shim).unwrap();
        std::os::unix::fs::symlink(dir.path().join("old/bin/mise"), &shim).unwrap();
        assert!(is_mise_shim(&shim, &mise_bin).unwrap());

        // The real mise dispatcher may itself be installed as a symlink in a
        // shared bin directory; its own name keeps it out of shim pruning.
        let dispatcher = dir.path().join("mise");
        fs::remove_file(&dispatcher).unwrap();
        std::os::unix::fs::symlink(&other_bin, &dispatcher).unwrap();
        assert!(!is_mise_shim(&dispatcher, &mise_bin).unwrap());
    }

    #[test]
    fn mise_shim_detection_recognizes_current_and_legacy_plugin_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let mise_bin = dir.path().join("mise");
        fs::write(&mise_bin, "mise").unwrap();
        let shim = dir.path().join("shim");

        fs::write(
            &shim,
            "#!/bin/sh\n# mise generated shim\nmise x -- foo \"$@\"\n",
        )
        .unwrap();
        assert!(is_mise_shim(&shim, &mise_bin).unwrap());

        fs::write(
            &shim,
            "#!/bin/sh\nexport ASDF_DATA_DIR=/tmp/mise\nexport PATH=x\nmise x -- foo \"$@\"\n",
        )
        .unwrap();
        assert!(is_mise_shim(&shim, &mise_bin).unwrap());

        fs::write(&shim, "#!/bin/sh\necho user-script\n").unwrap();
        assert!(!is_mise_shim(&shim, &mise_bin).unwrap());
    }

    #[test]
    fn native_shim_fingerprint_recognizes_current_and_transition_binaries() {
        assert!(has_mise_native_shim_fingerprint(
            b"PE\0mise generated native shim v1\0"
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
}
