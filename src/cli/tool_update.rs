use eyre::Result;

use crate::args::ToolArg;
use crate::{lock_file, tool_update};

/// Upgrade one globally configured tool in the background.
///
/// Started detached by `mise x`, shims, and tasks for tools whose global
/// `[tools]` entry sets `auto_update`. The per-tool lock keeps a second
/// updater for the same tool from running alongside it.
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true)]
pub(crate) struct ToolUpdate {
    #[usage(value_name = "TOOL")]
    tool: ToolArg,
}

impl ToolUpdate {
    pub(crate) async fn run(self) -> Result<()> {
        let tool_id = self.tool.ba.full_without_opts();
        let paths = tool_update::StatePaths::new(&tool_id);
        let Some(_lock) = lock_file::LockFile::at(&paths.lock).with_pid().try_lock()? else {
            debug!("tool-update: another update of {tool_id} is running");
            return Ok(());
        };
        let result = super::upgrade::upgrade_global_tool(self.tool).await;
        tool_update::record_result(&tool_id, &result);
        result
    }
}
