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
    /// What the watcher last persisted, if anything.
    pub health: Option<crate::system::history::health::Health>,
    /// The setup repository, when one is connected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncReport>,
}

#[derive(Serialize)]
pub(crate) struct SyncReport {
    pub origin: String,
    pub branch: String,
    pub mode: String,
    pub machine: String,
    pub last_publish: Option<String>,
    pub last_fetch: Option<String>,
    pub last_apply: Option<String>,
    pub pending_upload: usize,
    pub pending_applications: Vec<String>,
    pub conflicts: Vec<(String, String)>,
    pub declarations_changed: bool,
    pub last_error: Option<String>,
    pub application_failure: Option<String>,
    pub validation_error: Option<String>,
}

pub(crate) fn sync_report(
    store: &crate::system::history::checkpoint::Store,
    entries: &[crate::system::history::store::Entry],
) -> Result<Option<SyncReport>> {
    use crate::system::history::sync::{SyncMode, apply, backup, run};
    let Some((_, origin)) = crate::system::history::config::origin()? else {
        return Ok(None);
    };
    let status = run::read_status(store.state_dir());
    let roots = crate::system::history::sync::layout::Roots::current();
    let pending_upload = if SyncMode::current()?.publishes() {
        entries
            .iter()
            .filter(|entry| backup::eligible(entry))
            .filter(|entry| {
                status
                    .upload_since
                    .as_deref()
                    .is_none_or(|since| entry.checkpoint.created_at.as_str() >= since)
            })
            .filter(|entry| !status.uploaded.contains(&entry.checkpoint.uuid))
            .count()
    } else {
        0
    };
    Ok(Some(SyncReport {
        origin: origin.url,
        branch: origin.branch,
        mode: SyncMode::current()?.as_str().to_string(),
        machine: store.machine().name.clone(),
        last_publish: status.last_publish.clone(),
        last_fetch: status.last_fetch.clone(),
        last_apply: status.last_apply.clone(),
        pending_upload,
        pending_applications: status
            .pending_applications
            .iter()
            .filter_map(|pending| {
                roots
                    .locate(&pending.branch_path)
                    .path()
                    .map(crate::file::display_path)
            })
            .collect(),
        conflicts: apply::describe_conflicts(&status.conflicts),
        declarations_changed: status.declarations_changed,
        last_error: status.last_error.clone(),
        application_failure: status.application_failure.clone(),
        validation_error: status.validation_error.clone(),
    }))
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
            health: None,
            watcher: super::capture_health::watcher().await?,
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
        watcher: super::capture_health::watcher().await?,
        unavailable: store.unavailable().map(str::to_string),
        health: crate::system::history::health::read(store.state_dir()),
        sync: sync_report(&store, &entries)?,
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
    match &report.sync {
        None => miseprintln!(
            "Setup repository: none (`mise bootstrap dotfiles origin set <url>` shares this setup and backs it up)."
        ),
        Some(sync) => {
            miseprintln!(
                "Setup repository: {} (branch {}, mode {}, machine {}).",
                sync.origin,
                sync.branch,
                sync.mode,
                sync.machine
            );
            let when = |at: &Option<String>| {
                at.as_deref()
                    .map(local_time)
                    .unwrap_or_else(|| "never".into())
            };
            miseprintln!(
                "  last publish {}, last fetch {}, last pull {}; {} checkpoint(s) pending upload.",
                when(&sync.last_publish),
                when(&sync.last_fetch),
                when(&sync.last_apply),
                sync.pending_upload
            );
            if !sync.pending_applications.is_empty() {
                miseprintln!(
                    "  {} incoming change(s) pending: `mise bootstrap dotfiles pull` ({})",
                    sync.pending_applications.len(),
                    sync.pending_applications.join(", ")
                );
            }
            for (path, reason) in &sync.conflicts {
                miseprintln!(
                    "  sync paused: {path}: {reason}; sharing is paused for the entire setup; local history continues (`mise bootstrap dotfiles pull --take-remote|--keep-local {path}`)"
                );
            }
            if sync.declarations_changed {
                miseprintln!("  declarations changed: run `mise bootstrap` (dry-run available)");
            }
            if let Some(error) = &sync.last_error {
                miseprintln!("  last error: {error}");
            }
            if let Some(error) = &sync.application_failure {
                miseprintln!("  sync paused: {error}");
            }
            if let Some(error) = &sync.validation_error {
                miseprintln!("  incoming setup is invalid: {error}");
            }
        }
    }
    if let Some(health) = &report.health {
        use crate::system::history::health::age_secs;
        use crate::system::history::watch::runtime::humantime;
        let age = age_secs(health)
            .map(|secs| format!("{} ago", humantime(std::time::Duration::from_secs(secs))))
            .unwrap_or_else(|| "at an unknown time".into());
        let w = &health.watcher;
        miseprintln!(
            "  watcher health (updated {age}): last capture {}, last reconcile {}{}",
            w.last_capture
                .as_deref()
                .map(local_time)
                .unwrap_or_else(|| "never".into()),
            w.last_reconcile
                .as_deref()
                .map(local_time)
                .unwrap_or_else(|| "never".into()),
            if report.watcher == super::capture_health::Watcher::Running {
                ""
            } else {
                "; the watcher is not running now, so this is what it last reported"
            }
        );
        if let Some(error) = &w.last_error {
            miseprintln!(
                "  last capture failure: {error} ({} consecutive; at {}). Edits since then are not protected.",
                w.consecutive_failures,
                w.last_error_at
                    .as_deref()
                    .map(local_time)
                    .unwrap_or_else(|| "unknown".into())
            );
        }
        for degraded in &w.degraded {
            miseprintln!("  degraded: {degraded}");
        }
        for throttled in &health.throttled {
            miseprintln!(
                "  throttled: {} changes constantly; saved every {} ({} unsaved change(s), last saved {}). Not a failure: `mise bootstrap dotfiles exclude '{}'` if it is a log, cache, or database, or track it with --no-autosave and save explicitly.",
                throttled.path,
                humantime(std::time::Duration::from_secs(throttled.interval_secs)),
                throttled.pending_changes,
                throttled
                    .last_saved
                    .as_deref()
                    .map(local_time)
                    .unwrap_or_else(|| "never".into()),
                throttled.path
            );
        }
    }
    Ok(())
}
