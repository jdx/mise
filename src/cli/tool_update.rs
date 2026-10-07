use std::process::{Command, Stdio};
use std::sync::Arc;

use eyre::Result;

use crate::args::ToolArg;
use crate::config::Config;
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
}

impl ToolUpdate {
    pub(crate) async fn run(self) -> Result<()> {
        let tool_id = self.tool.ba.full_without_opts();
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
    // A separate process from $HOME, with the environment mise's activation
    // started from, loads only global config: the project's config, lockfile,
    // and `[env]` (PATH included) can't steer or be rewritten by the upgrade.
    // Its progress goes to stderr, and stdout stays the launched tool's alone.
    let status = Command::new(&*env::MISE_BIN)
        .args(["__tool-update", &tv.ba().short])
        .env_clear()
        .envs(
            env::PRISTINE_ENV
                .iter()
                .filter(|(key, _)| !key.starts_with("__MISE_")),
        )
        .env(tool_update::UPDATING_ENV, "1")
        .current_dir(*dirs::HOME)
        .stdin(Stdio::null())
        .stdout(std::io::stderr())
        .status();
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
