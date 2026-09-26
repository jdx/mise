//! Running mise as a shim: find which tool the shim stands for and hand off to `mise exec`.

use crate::cli::exec::Exec;
use crate::config::{CommandWrapper, Config, Settings, load_command_wrappers};
use crate::file::display_path;
use crate::request_exit;
use crate::shims::*;
use crate::toolset::{ResolveOptions, Toolset, ToolsetBuilder};
use crate::{env, file};
use color_eyre::eyre::{Result, bail};
use itertools::Itertools;
#[cfg(windows)]
use path_absolutize::Absolutize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

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

/// Apply the command wrapper for `mise x -- <name>` run by a native Windows shim.
///
/// `exe`- and `file`-mode shims on Windows are not mise itself: they run `mise x -- <name>` with
/// `__MISE_SHIM_PATH` naming themselves, so `handle_shim` never sees them. Left to `mise x`, the
/// wrapper is lost: the PATH lookup reaches the wrapper's own shim, skips it as the active shim,
/// and runs the real tool (#13671). When a wrapper applies, `command` is rewritten to run it and
/// the wrapper's environment is returned.
pub(crate) async fn apply_native_shim_command_wrapper(
    config: &mut Arc<Config>,
    ts: &mut Toolset,
    command: &mut Vec<String>,
) -> Result<Option<BTreeMap<String, String>>> {
    let Some(shim_path) = env::MISE_SHIM_PATH.read().unwrap().clone() else {
        return Ok(None);
    };
    let Some(program) = command.first() else {
        return Ok(None);
    };
    // A shim dispatches under its own name. The wrapper to apply is the one for that name, which
    // on Windows can differ from the typed command only in case.
    let Some(shim_name) = shim_path.file_stem().and_then(|stem| stem.to_str()) else {
        return Ok(None);
    };
    let same_name = if cfg!(windows) {
        shim_name.eq_ignore_ascii_case(program)
    } else {
        command_names_eq(shim_name, program)
    };
    if !same_name {
        return Ok(None);
    }
    let Some(wrapper) = command_wrapper_for(config, ts, shim_name, true).await? else {
        return Ok(None);
    };
    trace!("shim[{shim_name}] WRAPPER command: {}", wrapper.command());
    command[0] = wrapper.command().to_string();
    command.splice(1..1, wrapper.args().iter().cloned());
    Ok(Some(
        wrapper
            .env()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    ))
}

/// The command wrapper configured for `shim_name`, with its command installed first when
/// `install_command` is set and a tool provides it.
async fn command_wrapper_for(
    config: &mut Arc<Config>,
    ts: &mut Toolset,
    shim_name: &str,
    install_command: bool,
) -> Result<Option<CommandWrapper>> {
    let wrappers = load_command_wrappers(
        &config.config_files,
        ts.versions.values().flat_map(|versions| &versions.requests),
    )?;
    validate_wrapper_names(wrappers.keys())?;
    let wrapper = if cfg!(macos) {
        wrappers
            .iter()
            .find(|(name, _)| command_names_eq(name, shim_name))
            .map(|(_, wrapper)| wrapper)
    } else {
        wrappers.get(shim_name)
    };
    let Some(wrapper) = wrapper else {
        return Ok(None);
    };
    if command_names_eq(wrapper.command(), shim_name) {
        bail!("command wrapper for {shim_name} cannot delegate to itself");
    }
    if install_command {
        install_missing_wrapper_command(config, ts, wrapper.command()).await?;
    }
    Ok(Some(wrapper.clone()))
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
    if let Some(wrapper) =
        command_wrapper_for(config, &mut ts, shim_name, !completion_offline).await?
    {
        trace!("shim[{bin_name}] WRAPPER command: {}", wrapper.command());
        return Ok((PathBuf::from(wrapper.command()), ts, Some(wrapper)));
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
