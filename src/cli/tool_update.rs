use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use eyre::{Result, bail};

use crate::args::ToolArg;
use crate::config::{Config, Settings, SettingsExt};
use crate::tool_update::{self, Tick, Updater};
use crate::toolset::{ConfigScope, ResolveOptions, Toolset, ToolsetBuilder};
use crate::{dirs, env};

/// A service pass that takes longer than this (a hook waiting on input, a
/// hung download) is stopped, so the next pass can update the other tools.
const TICK_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Update globally configured tools for `auto_update`.
///
/// With a tool, upgrade it within its global config request. A shim or
/// `mise x` runs this before launching a tool whose check is due. Updates run
/// one at a time.
///
/// With `--watch`, run as the `tool-update` service: check every opted-in tool
/// once an hour and update the ones that are due. Declare the service with:
///
///     [bootstrap.services.mise-tool-update]
///     builtin = "tool-update"
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true, verbatim_doc_comment)]
pub(crate) struct ToolUpdate {
    #[usage(value_name = "TOOL")]
    tool: Option<ToolArg>,

    /// Run until stopped, updating due tools once an hour
    #[usage(long, conflicts = "tool")]
    watch: bool,

    /// Update every opted-in tool that is due, once (one `--watch` pass)
    #[usage(long, hide = true, conflicts = "tool")]
    due: bool,

    /// The tool id the launch claimed the check under; the result is recorded
    /// there, so `mise doctor` finds it under the same id
    #[usage(long, hide = true)]
    id: Option<String>,
}

impl ToolUpdate {
    pub(crate) async fn run(self) -> Result<()> {
        if self.watch {
            return watch().await;
        }
        if self.due {
            return update_due_tools().await;
        }
        let Some(tool) = self.tool else {
            bail!("pass a tool or --watch");
        };
        let tool_id = self.id.unwrap_or_else(|| tool.ba.full_without_opts());
        update_tool(tool, tool_id, false).await
    }
}

async fn update_tool(tool: ToolArg, tool_id: String, in_pass: bool) -> Result<()> {
    if Settings::get().locked {
        debug!("tool-update: skipped, `locked` is set");
        return Ok(());
    }
    let result = async {
        let _lock = tool_update::lock_for_update()?;
        // Another update may have changed the global lockfile while this waited.
        Config::reset().await?;
        if in_pass {
            // Replaced by the real result below; left in place if the pass is
            // stopped first (timed out, service stopped).
            tool_update::record_result(
                &tool_id,
                &Err(eyre::eyre!(
                    "the update did not finish: it was stopped after {TICK_TIMEOUT:?} or the service stopped"
                )),
            );
        }
        super::upgrade::upgrade_global_tool(tool).await
    }
    .await;
    tool_update::record_result(&tool_id, &result);
    result
}

/// The `tool-update` service loop. Launches leave updates to it while it holds
/// the service lock. Each pass runs in a child started like a launch's update,
/// so the service's working directory and environment never reach it.
async fn watch() -> Result<()> {
    let _service = tool_update::lock_service()?;
    info!("tool-update: checking opted-in tools every hour");
    let mut stop = std::pin::pin!(stop_signal());
    loop {
        // A pass that can't even start (the mise executable moved, say) won't
        // start next hour either: exit, so the lock is released and launches
        // update again, and let the service manager decide about restarting.
        let mut tick = Tick::start(update_command(&["--due"]))?;
        let finished = tokio::select! {
            result = tick.wait(TICK_TIMEOUT) => Some(result),
            _ = &mut stop => None,
        };
        match finished {
            Some(Ok(())) => {}
            Some(Err(err)) => warn!("tool-update: {err:#}"),
            None => {
                tick.kill();
                return Ok(());
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(tool_update::MIN_CHECK_DURATION) => {}
            _ = &mut stop => return Ok(()),
        }
    }
}

/// Resolves when the service manager asks the service to stop.
async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = terminate.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        // Stopping the service ends this process, and with it the job that
        // holds the pass's process tree.
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// One service pass: update each opted-in global tool whose check is due.
/// Each opted-in request resolves on its own, so a tool whose versions can't
/// be looked up doesn't stop the others; requests overridden by the
/// environment (`MISE_*_VERSION`) still count.
async fn update_due_tools() -> Result<()> {
    // Before claiming anything, so the tools are checked as soon as it's off.
    if Settings::get().locked {
        debug!("tool-update: skipped, `locked` is set");
        return Ok(());
    }
    let config = Config::get().await?;
    let global = ToolsetBuilder::new()
        .with_scope(ConfigScope::GlobalOnly)
        .without_runtime_env()
        .build_unresolved(&config)?;
    // One check per tool: the upgrade covers every opted-in version of it,
    // and they all share the tool's check marker.
    for (ba, versions) in global.versions.iter() {
        let requests = versions
            .requests
            .iter()
            .filter(|request| request.is_os_supported() && tool_update::enabled(request))
            .collect::<Vec<_>>();
        // The tool is checked as often as its most frequent version asks.
        let Some(first) = requests
            .iter()
            .copied()
            .min_by_key(|request| tool_update::interval(request))
        else {
            continue;
        };
        // Resolving can mean a remote lookup; only do it when due.
        if !tool_update::is_due(first, Updater::Service) {
            continue;
        }
        let mut floating = false;
        let mut lookup_error = None;
        for request in &requests {
            match request.resolve(&config, &ResolveOptions::default()).await {
                Ok(tv) => floating |= tool_update::updatable(&tv),
                Err(err) => {
                    warn!("tool-update: could not resolve {ba}: {err:#}");
                    lookup_error.get_or_insert(err);
                }
            }
        }
        let Some(tool_id) = tool_update::claim_due_request(first, Updater::Service) else {
            continue;
        };
        if !floating {
            // Only exact pins (now marked checked until the interval is up),
            // or versions that couldn't be looked up: say why in `mise doctor`.
            if let Some(err) = lookup_error {
                tool_update::record_result(&tool_id, &Err(err));
            }
            continue;
        }
        info!("tool-update: updating {tool_id}");
        let tool: ToolArg = match ba.short.parse() {
            Ok(tool) => tool,
            Err(err) => {
                warn!("tool-update: could not update {tool_id}: {err:#}");
                tool_update::record_result(&tool_id, &Err(err));
                continue;
            }
        };
        if let Err(err) = update_tool(tool, tool_id.clone(), true).await {
            warn!("tool-update: could not update {tool_id}: {err:#}");
        }
    }
    Ok(())
}

/// `mise __tool-update <args>` with the environment mise's activation started
/// from, run from the filesystem root so no project config (not even one in
/// $HOME) is above it: it loads only global config, so a project's config,
/// lockfile, and `[env]` (PATH included) can't steer or be rewritten by the
/// upgrade. The current environments (`-E`) carry over, since they choose
/// which global files apply. Launches inside its hooks skip their own updates.
fn update_command(args: &[&str]) -> Command {
    let mut command = Command::new(&*env::MISE_BIN);
    command
        .arg("__tool-update")
        .args(args)
        .env_clear()
        .envs(
            env::PRISTINE_ENV
                .iter()
                .filter(|(key, _)| !key.starts_with("__MISE_")),
        )
        .env(tool_update::UPDATING_ENV, "1")
        .env("MISE_ENV", env::mise_env().join(","))
        .current_dir(dirs::HOME.ancestors().last().unwrap_or(*dirs::HOME))
        .stdin(Stdio::null());
    // `--no-hooks` on the launch or watcher applies to its updates too.
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        command.env("MISE_NO_HOOKS", "1");
    }
    command
}

/// The config's spelling selects the request to upgrade; `tool_id`, the id
/// the check was claimed under, is where the result is recorded.
fn run_update(tool: &str, tool_id: &str) -> std::io::Result<ExitStatus> {
    update_command(&[tool, "--id", tool_id])
        .stdout(std::io::stderr())
        .status()
}

/// If the tool that provides `bin` opted into `auto_update` in global config
/// and its check is due, upgrade it now so this launch runs the new version.
/// Returns true when it was upgraded, so the caller resolves its toolset again.
/// A failed or offline update warns and leaves the installed version to run.
pub(crate) async fn update_before_launch(config: &Arc<Config>, ts: &Toolset, bin: &str) -> bool {
    if !tool_update::any_opted_in(ts) {
        return false;
    }
    let Some((_, tv)) = ts.which(config, bin).await else {
        return false;
    };
    let Some(tool_id) = tool_update::claim_due(&tv, Updater::Launch) else {
        return false;
    };
    // Its progress goes to stderr; stdout stays the launched tool's alone.
    match run_update(&tv.ba().short, &tool_id) {
        Ok(status) if status.success() => true,
        Ok(status) => {
            warn!("could not update {tool_id} ({status}), running the installed version");
            false
        }
        Err(err) => {
            warn!("could not update {tool_id}, running the installed version: {err}");
            false
        }
    }
}
