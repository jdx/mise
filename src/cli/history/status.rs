use eyre::Result;
use serde::Serialize;

use super::local_time;
use crate::system::history::store::OperationStatus;

/// Report capture health: what is tracked, the latest checkpoint, and
/// whether anything is saving automatically
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct HistoryStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,
}

#[derive(Serialize)]
struct StatusReport {
    enabled: bool,
    tracked_entries: usize,
    tracked_files: u64,
    checkpoints: usize,
    latest: Option<LatestReport>,
    pending_operations: usize,
    watcher: super::capture_health::Watcher,
    unavailable: Option<String>,
}

#[derive(Serialize)]
struct LatestReport {
    id: u64,
    created_at: String,
    trigger: String,
    description: String,
}

impl HistoryStatus {
    pub(crate) async fn run(self) -> Result<()> {
        let enabled = crate::config::Settings::get().history.enabled;
        let (store, tracked, entries) = super::open().await?;
        let walk = tracked.walk()?;
        let tracked_files = walk.roots.iter().map(|root| root.files.len() as u64).sum();
        let pending_operations = entries
            .iter()
            .filter(|entry| entry.checkpoint.status() == Some(OperationStatus::Pending))
            .count();
        let latest = entries.last().map(|entry| LatestReport {
            id: entry.id,
            created_at: entry.checkpoint.created_at.clone(),
            trigger: entry.checkpoint.trigger.as_str().to_string(),
            description: entry.checkpoint.description.clone(),
        });
        let report = StatusReport {
            enabled,
            tracked_entries: tracked.entries.len(),
            tracked_files,
            checkpoints: entries.len(),
            latest,
            pending_operations,
            watcher: super::capture_health::watcher(),
            unavailable: store.unavailable().map(str::to_string),
        };
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        if !report.enabled {
            miseprintln!("History is disabled (history.enabled = false).");
            return Ok(());
        }
        if let Some(reason) = &report.unavailable {
            miseprintln!("Capture is unavailable: {reason}");
        }
        miseprintln!(
            "Tracking {} entries ({} files); {} checkpoints recorded.",
            report.tracked_entries,
            report.tracked_files,
            report.checkpoints
        );
        match &report.latest {
            Some(latest) => miseprintln!(
                "Latest checkpoint {} ({}, {}): {}",
                latest.id,
                latest.trigger,
                local_time(&latest.created_at),
                latest.description
            ),
            None => miseprintln!("No checkpoint recorded yet; `mise history save` records one."),
        }
        if report.pending_operations > 0 {
            miseprintln!(
                "{} operation(s) did not finish; `mise history --pending` lists them.",
                report.pending_operations
            );
        }
        miseprintln!(
            "Automatic capture: {} ({}).",
            report.watcher.as_str(),
            super::capture_health::advice(report.watcher)
        );
        Ok(())
    }
}
