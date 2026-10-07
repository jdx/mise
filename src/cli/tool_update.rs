use eyre::Result;

use crate::args::ToolArg;
use crate::config::Config;
use crate::{dirs, lock_file, tool_update};

/// The background half of a tool update. It intentionally owns the lock for
/// the whole `mise upgrade` invocation, not merely the decision to start one:
/// a second foreground command must never start a concurrent updater for the
/// same tool while the first one is installing.
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true)]
pub(crate) struct ToolUpdate {
    #[usage(value_name = "TOOL")]
    tool: ToolArg,

    /// Foreground version already selected before this detached process was started.
    #[usage(long, hide = true)]
    current: String,

    /// Configured selector whose install options must accompany the bounded update.
    ///
    /// `request` is reserved by usage-rs, so use an explicit internal flag
    /// name rather than silently dropping this value in the child process.
    #[usage(long, hide = true)]
    selector: String,

    /// Boundary selected from trusted global configuration by the foreground process.
    #[usage(long, hide = true)]
    policy: tool_update::UpdatePolicy,
}

impl ToolUpdate {
    pub(crate) async fn run(self) -> Result<()> {
        let tool_id = self.tool.ba.full_without_opts();

        let config = Config::get().await?;
        if tool_update::tool_has_lockfile(&config, &tool_id).await? {
            debug!(
                "skipping background update for {} because its configuration has a lockfile",
                tool_id
            );
            return Ok(());
        }

        let key = tool_update::tool_update_key(&tool_id);
        let state_dir = dirs::STATE.join(tool_update::STATE_DIR);
        let lock_path = state_dir.join(format!("{key}.lock"));
        let Some(_lock) = lock_file::LockFile::at(&lock_path).with_pid().try_lock()? else {
            debug!(
                "skipping background update for {} because another updater is running",
                tool_id
            );
            return Ok(());
        };

        super::upgrade::run_background_tool_update(
            &config,
            self.tool,
            &self.current,
            &self.selector,
            self.policy,
        )
        .await
    }
}
