use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use eyre::{Result, bail};

use crate::args::ToolArg;
use crate::config::Config;
use crate::tool_update::{self, Updater};
use crate::toolset::{ConfigScope, Toolset, ToolsetBuilder};
use crate::{dirs, env};

/// A service tick that takes longer than this (a hook waiting on input, a
/// hung download) is stopped, so the next tick can update the other tools.
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

    /// Update every opted-in tool that is due, once (one `--watch` tick)
    #[usage(long, hide = true, conflicts = "tool")]
    due: bool,
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
        update_tool(tool).await
    }
}

async fn update_tool(tool: ToolArg) -> Result<()> {
    let tool_id = tool.ba.full_without_opts();
    let _lock = tool_update::lock_for_update()?;
    // Another update may have changed the global lockfile while this waited.
    Config::reset().await?;
    let result = super::upgrade::upgrade_global_tool(tool).await;
    tool_update::record_result(&tool_id, &result);
    result
}

/// The `tool-update` service loop. Launches leave updates to it while it holds
/// the service lock. Each tick runs in a child started like a launch's update,
/// so the service's working directory and environment never reach it.
async fn watch() -> Result<()> {
    let _service = tool_update::lock_service()?;
    info!("tool-update: checking opted-in tools every hour");
    loop {
        if let Err(err) = run_tick().await {
            warn!("tool-update: {err:#}");
        }
        tokio::time::sleep(tool_update::MIN_CHECK_DURATION).await;
    }
}

async fn run_tick() -> Result<()> {
    let mut command = tokio::process::Command::from(update_command(&["--due"]));
    command.stdout(Stdio::inherit()).kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    match tokio::time::timeout(TICK_TIMEOUT, child.wait()).await {
        Ok(status) => {
            let status = status?;
            if !status.success() {
                bail!("updating due tools failed ({status})");
            }
            Ok(())
        }
        Err(_) => {
            // Stop the hooks and downloads it started, not just the child.
            #[cfg(unix)]
            if let Some(pid) = child.id() {
                unsafe { nix::libc::killpg(pid as i32, nix::libc::SIGKILL) };
            }
            child.kill().await?;
            bail!("updating due tools took longer than {TICK_TIMEOUT:?}; stopped it")
        }
    }
}

/// One service tick: update each opted-in global tool whose check is due.
async fn update_due_tools() -> Result<()> {
    let config = Config::get().await?;
    let ts = ToolsetBuilder::new()
        .with_scope(ConfigScope::GlobalOnly)
        .build(&config)
        .await?;
    for (_, tv) in ts.list_current_versions() {
        let Some(tool_id) = tool_update::claim_due(&tv, Updater::Service) else {
            continue;
        };
        info!("tool-update: updating {tool_id}");
        let tool: ToolArg = tv.ba().short.parse()?;
        if let Err(err) = update_tool(tool).await {
            warn!("tool-update: could not update {tool_id}: {err:#}");
        }
    }
    Ok(())
}

/// `mise __tool-update <args>` from $HOME, with the environment mise's
/// activation started from: it loads only global config, so the project's
/// config, lockfile, and `[env]` (PATH included) can't steer or be rewritten
/// by the upgrade. Launches inside its hooks skip their own updates.
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
        .current_dir(*dirs::HOME)
        .stdin(Stdio::null());
    command
}

fn run_update(tool: &str) -> std::io::Result<ExitStatus> {
    update_command(&[tool]).stdout(std::io::stderr()).status()
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
    // Its progress goes to stderr, and stdout stays the launched tool's alone.
    match run_update(&tv.ba().short) {
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
