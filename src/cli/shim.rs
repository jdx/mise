//! Running mise as a shim: find which tool the shim stands for and hand off to `mise exec`.

use crate::cli::exec::{Exec, ExecExtras, exec_program};
use crate::config::{
    CommandWrapper, CommandWrapperOptions, Config, Settings, command_wrapper_source,
    insert_config_file_template_vars, load_command_wrappers,
};
use crate::file::display_path;
use crate::request_exit;
use crate::sandbox::SandboxConfig;
use crate::shims::*;
use crate::toolset::{ResolveOptions, Toolset, ToolsetBuilder};
use crate::{env, file};
use color_eyre::eyre::{Result, bail};
use itertools::Itertools;
#[cfg(windows)]
use path_absolutize::Absolutize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[cfg(unix)]
mod fallback;

enum ShimCommand {
    Tool(PathBuf),
    Wrapper(CommandWrapper),
    SystemFallback(PathBuf),
    /// What a `PATH` lookup finds behind a wrapper-directory shim this project has no wrapper for.
    PathLookup(PathBuf),
}

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
    #[cfg(unix)]
    let fallback_chain = fallback::Chain::take_inherited();
    let mut config = Config::get().await?;
    let mut args = env::ARGS.read().unwrap().clone();
    env::PREFER_OFFLINE.store(true, Ordering::Relaxed);
    trace!("shim[{bin_name}] args: {}", args.join(" "));
    let (mut command, mut ts) = which_shim(&mut config, &env::MISE_BIN_NAME, &args).await?;
    // A due `auto_update` upgrades this shim's tool before it runs, so look
    // the binary up again to launch the new version. Never while completing.
    // Nor for a shim that only passes the command on: the binary it runs is not this shim's tool.
    let shim_name = command_name_without_exe_suffix(&env::MISE_BIN_NAME);
    if !matches!(command, ShimCommand::PathLookup(_))
        && !is_offline_completion(shim_name, &args)
        && super::tool_update::update_before_launch(&config, &ts, shim_name).await
    {
        config = Config::reset().await?;
        (command, ts) = which_shim(&mut config, &env::MISE_BIN_NAME, &args).await?;
    }
    let (bin, wrapper) = match command {
        ShimCommand::Tool(bin) => (bin, None),
        ShimCommand::Wrapper(wrapper) => (PathBuf::from(wrapper.command()), Some(wrapper)),
        ShimCommand::SystemFallback(bin) => {
            #[cfg(unix)]
            fallback_chain.record(bin_name, &file::canonicalize_or_self(&bin))?;
            (bin, None)
        }
        ShimCommand::PathLookup(bin) => {
            #[cfg(unix)]
            fallback_chain.record(bin_name, &file::canonicalize_or_self(&bin))?;
            return pass_on(bin, args).await;
        }
    };
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
        deny_unix: false,
        deny_env: false,
        allow_read: vec![],
        allow_write: vec![],
        allow_net: vec![],
        allow_connect_port: vec![],
        allow_unix: vec![],
        allow_env: vec![],
        secrets: vec![],
        secrets_all: false,
        native_shim_completion: false,
    };
    time!("shim exec");
    if let Some(wrapper) = wrapper {
        exec.run_with_command_wrapper(
            config,
            ts,
            with_active_command_wrapper(
                shim_name,
                wrapper
                    .env()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            ),
        )
        .await?;
    } else {
        exec.run_with_toolset(config, ts).await?;
    }
    Err(request_exit(0))
}

/// Runs what a wrapper-directory shim passes its command on to as if the shim were not there:
/// with the arguments and the environment the shim was given. That environment is the caller's,
/// so it holds a task's `_.path` and env or the tools of a `mise x tool@version`; the one mise
/// would build for this directory holds neither.
async fn pass_on(bin: PathBuf, args: Vec<String>) -> Result<()> {
    time!("shim pass on");
    exec_program(
        bin,
        args.into_iter().skip(1),
        BTreeMap::new(),
        BTreeSet::new(),
        &SandboxConfig::default(),
        false,
        &ExecExtras::default(),
    )
    .await?;
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

/// Whether a selected but uninstalled tool version may provide `shim_name`: it is named after the
/// command, the registry lists the command as one of its bins, or another installed version of
/// the same tool provides it. Lookup errors count as "no" so a leftover wrapper still falls
/// through.
async fn missing_version_may_provide(
    config: &Arc<Config>,
    ts: &Toolset,
    shim_name: &str,
    bin_name: &str,
) -> bool {
    let missing = ts.list_missing_versions(config).await;
    if missing.is_empty() {
        return false;
    }
    let installed = ts.list_installed_versions(config).await.unwrap_or_default();
    for tv in &missing {
        if tv.ba().matches_bin_name(shim_name)
            || crate::registry::REGISTRY
                .get(&tv.ba().short)
                .is_some_and(|tool| tool.provides_bin(shim_name))
        {
            return true;
        }
        for (p, installed_tv) in installed.iter().filter(|(p, _)| **p.ba() == *tv.ba()) {
            for name in [shim_name, bin_name].into_iter().unique() {
                if matches!(p.which(config, installed_tv, name).await, Ok(Some(_))) {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether this process was started through a shim in the command-wrappers dir: by its path when
/// it has one, otherwise the first entry of `path` that holds `bin_name`.
fn invoked_from_wrapper_dir(bin_name: &str, path: &[PathBuf]) -> bool {
    let argv0 = Path::new(&*env::ARGV0);
    if let Some(dir) = argv0.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        return file::is_command_wrapper_dir(dir);
    }
    first_on_path(bin_name, path.iter())
        .is_some_and(|bin| bin.parent().is_some_and(file::is_command_wrapper_dir))
}

/// The first `bin_name` a lookup on `dirs` finds, looked up as the platform does it. On Windows
/// that is with the `PATHEXT` extensions: a shim run as `cargo` is the file `cargo.exe`, and a
/// file named `cargo` with no extension is not a command.
fn first_on_path<'a>(bin_name: &str, dirs: impl Iterator<Item = &'a PathBuf>) -> Option<PathBuf> {
    let path = std::env::join_paths(dirs).ok()?;
    let cwd = crate::dirs::CWD.clone().unwrap_or_default();
    which::which_in_all(bin_name, Some(path), cwd).ok()?.next()
}

/// The first `bin_name` on this process's `PATH` outside the command-wrappers directories: what a
/// `PATH` lookup finds when a shim in those directories is not there. Only asked for a command
/// this project does not wrap.
///
/// Every project shares the wrapper directory, and it leads the `PATH` mise gives a project with
/// wrappers. A shim another project left there must not decide what runs here, so it passes the
/// command on to whatever comes next: a directory the user lists before the shims, a task's
/// `_.path`, a tool's own directory.
///
/// The lookup repeats the one that reached the shim, see [`looked_up_name`].
///
/// None when that is one of mise's shims, or nothing: the command then resolves as it does
/// through mise's ordinary shim.
fn bin_behind_wrapper_dirs(bin_name: &str) -> Option<PathBuf> {
    // `env::PATH` is this `PATH` without what a parent mise added, the wrapper directory and
    // the tool directories included, so it is not what the lookup ran on.
    let bin = first_on_path(
        looked_up_name(bin_name),
        env::PATH_NON_PRISTINE
            .iter()
            .filter(|dir| !file::is_command_wrapper_dir(dir)),
    )?;
    let mise_bin = file::canonicalize_or_self(&env::MISE_BIN);
    let is_mise_shim = is_shim_file(&bin)
        || file::is_active_mise_shim(&bin)
        || file::canonicalize_cached(&bin).is_some_and(|bin| bin == mise_bin);
    (!is_mise_shim).then_some(bin)
}

/// The name the lookup that reached this shim was for, where `bin_name` is the shim's file name
/// as it was started. It only differs from `bin_name` on Windows, where `cargo` and `cargo.exe`
/// are two lookups: `cargo` runs the first `cargo.exe`, `cargo.cmd` and so on that `PATH` holds,
/// directory by directory, and `cargo.exe` runs the first file of that name.
///
/// A shim started by a bare name was started by the name its caller looked up, which `cmd` and
/// a program that spawns `cargo` pass on as they got it. A shim started by its path was not:
/// PowerShell passes the path of the file it found for either name. That is taken for a lookup
/// of `cargo`, the usual one, so a `cargo.exe` typed in PowerShell runs what `cargo` does.
fn looked_up_name(bin_name: &str) -> &str {
    let started_by_path = Path::new(&*env::ARGV0)
        .parent()
        .is_some_and(|dir| !dir.as_os_str().is_empty());
    if started_by_path {
        command_name_without_exe_suffix(bin_name)
    } else {
        bin_name
    }
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
    if !same_command_name(shim_name, program) {
        return Ok(None);
    }
    let install_command = !is_native_shim_completion(command);
    let Some(wrapper) =
        command_wrapper_for(config, ts, shim_name, &command[1..], install_command).await?
    else {
        return Ok(None);
    };
    trace!("shim[{shim_name}] WRAPPER command: {}", wrapper.command());
    command[0] = wrapper.command().to_string();
    command.splice(1..1, wrapper.args().iter().cloned());
    Ok(Some(with_active_command_wrapper(
        shim_name,
        wrapper
            .env()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )))
}

/// Install the tool that provides `mise x -- <name>` run by a native shim.
///
/// `which_shim` never runs for such a shim, and `mise x` on its own installs every missing tool
/// the config lists. A config that only lists tools loads without trust, so this installs no
/// more than `which_shim` does: the configured tool that provides the command, and nothing
/// while completing.
pub(crate) async fn install_native_shim_command(
    config: &mut Arc<Config>,
    ts: &mut Toolset,
    command: &[String],
) -> Result<()> {
    if env::MISE_SHIM_PATH.read().unwrap().is_none() || is_native_shim_completion(command) {
        return Ok(());
    }
    let Some(program) = command.first() else {
        return Ok(());
    };
    install_missing_wrapper_command(config, ts, command_name_without_exe_suffix(program)).await
}

/// Whether `mise x -- <command>` is a native shim completing a word, which installs nothing,
/// as [`is_offline_completion`] keeps a shim that is mise itself from installing.
pub(crate) fn is_native_shim_completion(command: &[String]) -> bool {
    env::MISE_SHIM_PATH.read().unwrap().is_some()
        && command.first().is_some_and(|program| {
            is_offline_completion(command_name_without_exe_suffix(program), command)
        })
}

/// The command wrapper configured for `shim_name`, with its command installed first when
/// `install_command` is set and a tool provides it. None while that wrapper's own command is
/// running, so the command finds the real tool instead of the wrapper again.
async fn command_wrapper_for(
    config: &mut Arc<Config>,
    ts: &mut Toolset,
    shim_name: &str,
    call_args: &[String],
    install_command: bool,
) -> Result<Option<CommandWrapper>> {
    if is_active_command_wrapper(shim_name) {
        return Ok(None);
    }
    let wrappers = load_command_wrappers(
        &config.config_files,
        ts.versions.values().flat_map(|versions| &versions.requests),
    )?;
    validate_wrapper_names(wrappers.keys())?;
    // An exact match wins. macOS collisions are rejected above; Windows ones cannot both be shims.
    let wrapper = wrappers.get_key_value(shim_name).or_else(|| {
        wrappers
            .iter()
            .find(|(name, _)| same_command_name(name, shim_name))
    });
    let Some((name, wrapper)) = wrapper else {
        return Ok(None);
    };
    // Like the file's other values, a wrapper renders with its file's `config_root` and resolves
    // relative paths from the file's directory.
    let source = command_wrapper_source(&config.config_files, name)?;
    let dir = match source.as_deref().and_then(Path::parent) {
        Some(dir) => dir.to_path_buf(),
        None => env::current_dir()?,
    };
    let with_source = |ctx: &tera::Context| {
        let mut ctx = ctx.clone();
        if let Some(source) = &source {
            insert_config_file_template_vars(&mut ctx, source);
        }
        ctx
    };
    // `command` renders from `vars` and the process environment only: the full toolset context
    // needs the project's tools installed, and the command may be what installs them.
    let command = render_template(
        wrapper.command(),
        &with_source(&config.tera_ctx),
        &dir,
        source.as_deref(),
    )?;
    if command.trim().is_empty() {
        bail!(
            "command wrapper command rendered blank: {:?}",
            wrapper.command()
        );
    }
    if same_command_name(&command, shim_name) {
        bail!("command wrapper for {shim_name} cannot delegate to itself");
    }
    if install_command {
        install_missing_wrapper_command(config, ts, &command).await?;
    }
    let templated = wrapper
        .args()
        .iter()
        .chain(wrapper.env().values())
        .any(|value| crate::tera::contains_template_syntax(value));
    if !templated {
        return Ok(Some(CommandWrapper::Detailed(CommandWrapperOptions {
            command,
            args: wrapper.args().to_vec(),
            env: wrapper.env().clone(),
        })));
    }
    // The toolset context carries the resolved project `[env]`, which `config.tera_ctx` lacks.
    let mut ctx = with_source(ts.tera_ctx(config).await?);
    // What the user typed after the command name, for `args` and `env` only.
    ctx.insert("argv", call_args);
    let args = wrapper
        .args()
        .iter()
        .map(|arg| render_template(arg, &ctx, &dir, source.as_deref()))
        .collect::<Result<Vec<_>>>()?;
    let env = wrapper
        .env()
        .iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                render_template(value, &ctx, &dir, source.as_deref())?,
            ))
        })
        .collect::<Result<_>>()?;
    Ok(Some(CommandWrapper::Detailed(CommandWrapperOptions {
        command,
        args,
        env,
    })))
}

/// Render a wrapper value as a template, so a wrapper can use `vars` and the path functions
/// like a task. Values without template syntax are left untouched. `source` is the file that
/// declares the wrapper; one from global or system config keeps `env` in safe mode.
fn render_template(
    input: &str,
    ctx: &tera::Context,
    dir: &Path,
    source: Option<&Path>,
) -> Result<String> {
    if !crate::tera::contains_template_syntax(input) {
        return Ok(input.to_string());
    }
    let mut tera = crate::tera::get_tera(Some(dir));
    Ok(match source {
        Some(source) => crate::tera::render_config_str(&mut tera, input, ctx, source)?,
        None => crate::tera::render_str(&mut tera, input, ctx)?,
    })
}

/// Shell completion invokes `usage complete-word` through the `usage` shim, and a usage-rs
/// tool's generated script invokes `<tool> __complete_word__` through that tool's shim. Either
/// should use the installed CLI or fail locally, never resolve a floating tool version,
/// auto-install, or update over the network while the user is pressing tab. `shim_name` has
/// the Windows executable suffix stripped.
pub(crate) fn is_offline_completion(shim_name: &str, args: &[String]) -> bool {
    let is_usage = if cfg!(windows) {
        shim_name.eq_ignore_ascii_case("usage")
    } else {
        shim_name == "usage"
    };
    (is_usage && args.get(1).is_some_and(|arg| arg == "complete-word"))
        || args.get(1).is_some_and(|arg| arg == "__complete_word__")
}

async fn which_shim(
    config: &mut Arc<Config>,
    bin_name: &str,
    args: &[String],
) -> Result<(ShimCommand, Toolset)> {
    let shim_name = command_name_without_exe_suffix(bin_name);
    let completion_offline = is_offline_completion(shim_name, args);
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
        .with_deferred_lazy_resolution()
        .build(config)
        .await?;
    if let Some(wrapper) =
        command_wrapper_for(config, &mut ts, shim_name, &args[1..], !completion_offline).await?
    {
        trace!("shim[{bin_name}] WRAPPER command: {}", wrapper.command());
        return Ok((ShimCommand::Wrapper(wrapper), ts));
    }
    // No wrapper here, so a shim in the wrapper directory is another project's: pass the command
    // on. `mise x -- <name>` never picks such a shim, so this is a lookup on `PATH`.
    if !is_active_command_wrapper(shim_name)
        && invoked_from_wrapper_dir(bin_name, &env::PATH_NON_PRISTINE)
        && let Some(bin) = bin_behind_wrapper_dirs(bin_name)
    {
        trace!("shim[{bin_name}] PATH {bin}", bin = display_path(&bin));
        return Ok((ShimCommand::PathLookup(bin), ts));
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
                return Ok((ShimCommand::Tool(bin), ts));
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
            return Ok((ShimCommand::Tool(bin), ts));
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
                return Ok((ShimCommand::Tool(bin), ts));
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
                return Ok((ShimCommand::Tool(bin), ts));
            }
        }
    }
    // fallback for "system"
    // A wrapper shim left by another project must not turn a plain command into an error.
    // Only when no missing tool version could provide this command: that must still fail.
    // Installed tools were tried above.
    let inactive_wrapper = invoked_from_wrapper_dir(bin_name, &env::PATH)
        && !missing_version_may_provide(config, &ts, shim_name, bin_name).await;
    if Settings::get().not_found_system_fallback || inactive_wrapper {
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
                return Ok((ShimCommand::SystemFallback(bin), ts));
            }
        }
    }
    let mut tvs = ts.list_rtvs_with_bin(config, shim_name).await?;
    if tvs.is_empty() && shim_name != bin_name {
        tvs = ts.list_rtvs_with_bin(config, bin_name).await?;
    }
    // A configured tool with a missing version that may provide this command keeps its own error:
    // another installed tool shipping the same executable must not stand in for a pinned one.
    if !missing_version_may_provide(config, &ts, shim_name, bin_name).await
        && let Some(bin) =
            crate::shims::installed_fallback_bin(config, &mut ts, &tvs, shim_name, bin_name).await?
    {
        return Ok((ShimCommand::Tool(bin), ts));
    }
    match err_no_version_set(config, ts, shim_name, tvs).await {
        Ok(_) => unreachable!("err_no_version_set always returns an error"),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod completion_tests {
    use super::is_offline_completion;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn usage_complete_word_is_offline() {
        assert!(is_offline_completion(
            "usage",
            &args(&["usage", "complete-word"])
        ));
    }

    #[test]
    fn a_forwarded_usage_rs_request_is_offline_for_any_shim() {
        assert!(is_offline_completion(
            "aube",
            &args(&["aube", "__complete_word__"])
        ));
    }

    #[test]
    fn ordinary_shim_invocations_are_not_offline() {
        assert!(!is_offline_completion("aube", &args(&["aube", "install"])));
        assert!(!is_offline_completion(
            "node",
            &args(&["node", "complete-word"])
        ));
    }
}
