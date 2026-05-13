//! `mise wings inventory` — upload the current security inventory snapshot.

use eyre::Result;

/// Upload current installed-tool inventory to mise-wings
///
/// Reports the current machine's installed tool versions, platform, and
/// Wings artifact digests when present. The snapshot is scoped to the
/// authenticated Wings org and intentionally excludes local paths, usernames,
/// hostnames, environment values, command arguments, and package-manager logs.
#[derive(Debug, Default, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Inventory {}

impl Inventory {
    pub async fn run(self) -> Result<()> {
        let config = crate::config::Config::get().await?;
        let summary = crate::wings::inventory::submit_current_snapshot(&config).await?;
        miseprintln!(
            "uploaded wings inventory: {} tools (device {})",
            summary.tools_count,
            summary.device_id
        );
        Ok(())
    }
}
