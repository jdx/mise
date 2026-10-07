use std::process::{Command, Stdio};
use std::sync::Arc;

use eyre::Result;

use crate::args::ToolArg;
use crate::config::{Config, Settings, SettingsExt};
use crate::toolset::Toolset;
use crate::{dirs, env, tool_update};

/// Upgrade one globally configured tool for `auto_update`.
///
/// Run by a shim or `mise x` before it launches a tool whose global `[tools]`
/// entry sets `auto_update` and whose check is due. Updates run one at a time.
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true)]
pub(crate) struct ToolUpdate {
    #[usage(value_name = "TOOL")]
    tool: ToolArg,

    /// The tool id the launch claimed the check under; the result is recorded
    /// there, so `mise doctor` finds it under the same id
    #[usage(long, hide = true)]
    id: Option<String>,
}

impl ToolUpdate {
    pub(crate) async fn run(self) -> Result<()> {
        if Settings::get().locked {
            debug!("tool-update: skipped, `locked` is set");
            return Ok(());
        }
        let tool_id = self.id.unwrap_or_else(|| self.tool.ba.full_without_opts());
        let _lock = tool_update::lock_for_update()?;
        // Another update may have changed the global lockfile while this waited.
        Config::reset().await?;
        let result = super::upgrade::upgrade_global_tool(self.tool).await;
        tool_update::record_result(&tool_id, &result);
        result
    }
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
    let Some(tool_id) = tool_update::claim_due(&tv) else {
        return false;
    };
    // A separate process with the environment mise's activation started from,
    // run from the filesystem root so no project config (not even one in $HOME)
    // is above it, loads only global config: a project's config, lockfile, and
    // `[env]` (PATH included) can't steer or be rewritten by the upgrade. The
    // launch's environments (`-E`) carry over, since they choose which global
    // files apply. Its progress goes to stderr; stdout stays the launched tool's.
    let mut command = Command::new(&*env::MISE_BIN);
    command
        // The config's spelling selects the request to upgrade; the id the
        // check was claimed under is where the result is recorded.
        .args(["__tool-update", &tv.ba().short, "--id", &tool_id])
        .env_clear()
        .envs(
            env::PRISTINE_ENV
                .iter()
                .filter(|(key, _)| !key.starts_with("__MISE_")),
        )
        .env(tool_update::UPDATING_ENV, "1")
        .env("MISE_ENV", env::mise_env().join(","))
        .current_dir(dirs::HOME.ancestors().last().unwrap_or(*dirs::HOME))
        .stdin(Stdio::null())
        .stdout(std::io::stderr());
    // `--no-hooks` on the launch applies to its update too.
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        command.env("MISE_NO_HOOKS", "1");
    }
    let status = command.status();
    match status {
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
