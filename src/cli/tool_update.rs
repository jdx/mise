use eyre::Result;

use crate::args::ToolArg;
use crate::config::{Config, Settings, SettingsExt};
use crate::{dirs, file, lock_file, tool_update};

const STATE_DIR: &str = "tool-update";

/// The background half of a tool update. It intentionally owns the lock for
/// the whole `mise upgrade` invocation, not merely the decision to start one:
/// a second foreground command must never start a concurrent updater for the
/// same tool while the first one is installing.
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true)]
pub(crate) struct ToolUpdate {
    #[usage(value_name = "TOOL")]
    tool: ToolArg,
}

impl ToolUpdate {
    pub(crate) async fn run(self) -> Result<()> {
        let settings = Settings::get();
        let tool_id = self.tool.ba.full_without_opts();
        let Some(policy) = tool_update::update_policy(&settings, self.tool.ba.as_ref()) else {
            return Ok(());
        };

        let config = Config::get().await?;
        if tool_update::tool_has_lockfile(&config, &tool_id).await? {
            debug!(
                "skipping background update for {} because its configuration has a lockfile",
                tool_id
            );
            return Ok(());
        }

        let key = tool_update::tool_update_key(&tool_id);
        let state_dir = dirs::STATE.join(STATE_DIR);
        let lock_path = state_dir.join(format!("{key}.lock"));
        let Some(_lock) = lock_file::LockFile::at(&lock_path).with_pid().try_lock()? else {
            debug!(
                "skipping background update for {} because another updater is running",
                tool_id
            );
            return Ok(());
        };

        let check_duration = settings.tool_update_check_duration()?;
        let last_check_path = state_dir.join(key);
        if !update_check_due(&last_check_path, check_duration) {
            return Ok(());
        }
        // Mark the attempt before doing network or installation work. A failed
        // update is non-fatal to the foreground invocation and must not cause
        // every subsequent command to retry it immediately.
        file::write_atomic(last_check_path, "")?;

        super::upgrade::run_background_tool_update(self.tool, policy).await
    }
}

fn update_check_due(path: &std::path::Path, duration: std::time::Duration) -> bool {
    file::modified_duration(path).map_or(true, |age| age >= duration)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::update_check_due;

    #[test]
    fn missing_check_is_due_and_fresh_check_is_not() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("last-check");
        assert!(update_check_due(&marker, Duration::from_secs(1)));
        std::fs::write(&marker, "").unwrap();
        assert!(!update_check_due(&marker, Duration::from_secs(3600)));
    }
}
