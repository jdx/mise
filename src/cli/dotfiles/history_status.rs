//! The history half of `mise bootstrap dotfiles status`: what is tracked,
//! the latest checkpoint, unfinished operations, and whether edits are
//! being saved automatically.

use eyre::Result;
use serde::Serialize;

use super::history::local_time;

#[derive(Serialize)]
pub(crate) struct HistoryReport {
    pub enabled: bool,
    pub tracked_entries: usize,
    pub tracked_files: u64,
    pub checkpoints: usize,
    pub latest: Option<LatestReport>,
    pub pending_operations: usize,
    pub watcher: super::capture_health::Watcher,
    pub unavailable: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct LatestReport {
    pub id: u64,
    pub created_at: String,
    pub trigger: String,
    pub description: String,
}

pub(crate) async fn report() -> Result<HistoryReport> {
    let settings = crate::config::Settings::get();
    let enabled = settings.experimental && settings.history.enabled;
    if !settings.experimental {
        return Ok(HistoryReport {
            enabled: false,
            tracked_entries: 0,
            tracked_files: 0,
            checkpoints: 0,
            latest: None,
            pending_operations: 0,
            watcher: super::capture_health::watcher(),
            unavailable: Some("dotfile tracking requires experimental = true".into()),
        });
    }
    let (store, tracked, entries) = super::history::open().await?;
    let walk = tracked.walk()?;
    let tracked_files = walk.roots.iter().map(|root| root.files.len() as u64).sum();
    // an operation still running, or one that crashed: its record is in
    // the index only once it is closed
    let pending_operations =
        crate::system::history::store::peek_pending_in(store.state_dir())?.len();
    let latest = entries.last().map(|entry| LatestReport {
        id: entry.id,
        created_at: entry.checkpoint.created_at.clone(),
        trigger: entry.checkpoint.trigger.as_str().to_string(),
        description: entry.checkpoint.description.clone(),
    });
    Ok(HistoryReport {
        enabled,
        tracked_entries: tracked.entries.len(),
        tracked_files,
        checkpoints: entries.len(),
        latest,
        pending_operations,
        watcher: super::capture_health::watcher(),
        unavailable: store.unavailable().map(str::to_string),
    })
}

pub(crate) fn print(report: &HistoryReport) -> Result<()> {
    if !crate::config::Settings::get().experimental {
        miseprintln!("History: experimental; enable with `mise settings experimental=true`.");
        return Ok(());
    }
    if !report.enabled {
        miseprintln!("History: disabled (history.enabled = false).");
        return Ok(());
    }
    if let Some(reason) = &report.unavailable {
        miseprintln!("History: capture is unavailable: {reason}");
    }
    miseprintln!(
        "History: tracking {} entries ({} files); {} checkpoints recorded.",
        report.tracked_entries,
        report.tracked_files,
        report.checkpoints
    );
    match &report.latest {
        Some(latest) => miseprintln!(
            "  latest checkpoint {} ({}, {}): {}",
            latest.id,
            latest.trigger,
            local_time(&latest.created_at),
            latest.description
        ),
        None => miseprintln!(
            "  no checkpoint recorded yet; `mise bootstrap dotfiles save` records one."
        ),
    }
    if report.pending_operations > 0 {
        miseprintln!(
            "  {} operation(s) did not finish; `mise bootstrap dotfiles history --pending` lists them.",
            report.pending_operations
        );
    }
    miseprintln!(
        "  automatic capture: {} ({}).",
        report.watcher.as_str(),
        super::capture_health::advice(report.watcher)
    );
    Ok(())
}
