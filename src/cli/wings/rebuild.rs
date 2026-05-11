//! `mise wings rebuild` — request a fresh server-side Wings artifact.

use eyre::Result;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::toolset::{ResolveOptions, ToolRequest, ToolSource};
use crate::wings::artifact;

/// Rebuild a Wings artifact for a tool version.
///
/// Resolves the tool exactly like an install would, asks the Wings API to evict
/// the current packaged catalog row for this org/tool/platform, and queues a
/// fresh server-side packaging job.
///
/// Examples:
///
/// ```sh
/// $ mise wings rebuild jq@1.7.1
/// ```
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Rebuild {
    /// Tool version to rebuild
    /// e.g.: node@20
    #[clap(value_name = "TOOL@VERSION")]
    tool: ToolArg,
}

impl Rebuild {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let request = self.tool_request();
        let backend = request.backend()?;
        let mut tv = request
            .resolve(
                &config,
                &ResolveOptions {
                    use_locked_version: false,
                    refresh_remote_versions: true,
                    ..Default::default()
                },
            )
            .await?;

        let job = artifact::rebuild(backend.as_ref(), &mut tv).await?;
        miseprintln!(
            "Queued wings rebuild for {} (job {}, {}%, {})",
            tv.style(),
            job.id,
            job.progress_percent,
            progress_message(&job),
        );
        if let Some(reason) = job.blocked_reason {
            miseprintln!("Install remains blocked by policy: {reason}");
        }
        Ok(())
    }

    fn tool_request(&self) -> ToolRequest {
        self.tool
            .tvr
            .clone()
            .unwrap_or_else(|| ToolRequest::Version {
                backend: self.tool.ba.clone(),
                version: "latest".into(),
                options: self.tool.ba.opts(),
                source: ToolSource::Argument,
            })
    }
}

fn progress_message(job: &artifact::RebuildJob) -> &str {
    if job.message.is_empty() {
        &job.status
    } else {
        &job.message
    }
}
