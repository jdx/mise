//! The history half of `mise dot status`: what is tracked,
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
    /// Entries kept in this machine's local-only history instead.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub local_entries: usize,
    /// Files under a tracked entry that every save leaves out, with why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<crate::system::history::store::PathReason>,
    /// Nested repositories under a tracked entry, saved as a commit pointer
    /// without their files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested: Vec<crate::system::history::store::PathReason>,
    pub checkpoints: usize,
    pub latest: Option<LatestReport>,
    /// Tracked paths edited since the latest checkpoint; `None` when they
    /// could not be determined.
    pub unsaved: Option<Vec<String>>,
    /// Tracked entries another machine saved while they were not selected
    /// here: saves leave them alone until a pull applies them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_applied: Vec<String>,
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
    pub last_publish: Option<String>,
    pub last_fetch: Option<String>,
    pub last_apply: Option<String>,
    pub pending_applications: Vec<String>,
    pub conflicts: Vec<(String, String)>,
    pub declarations_changed: bool,
    pub last_error: Option<String>,
    /// When the current run of failed syncs began, and how many so far.
    pub failing_since: Option<String>,
    pub consecutive_failures: u32,
    pub application_failure: Option<String>,
    pub validation_error: Option<String>,
    /// Paths sync neither applies nor removes, with why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<(String, String)>,
    /// Whether sync-conflict notifications can reach the user.
    pub notifications: String,
}

pub(crate) fn sync_report(
    store: &crate::system::history::checkpoint::Store,
) -> Result<Option<SyncReport>> {
    use crate::system::history::sync::{SyncMode, apply, run};
    let Some(origin) = crate::system::history::config::origin()?
        .map(|(_, origin)| origin)
        .or_else(|| run::origin().ok())
    else {
        return Ok(None);
    };
    let status = run::read_status(store.state_dir())?;
    let roots = crate::system::history::sync::layout::Roots::current();
    Ok(Some(SyncReport {
        origin: origin.url,
        branch: origin.branch,
        mode: SyncMode::current()?.as_str().to_string(),
        last_publish: status.last_publish.clone(),
        last_fetch: status.last_fetch.clone(),
        last_apply: status.last_apply.clone(),
        pending_applications: status
            .pending_applications
            .iter()
            .filter_map(|pending| {
                roots
                    .locate(&pending.branch_path)
                    .path()
                    .map(crate::file::display_path)
            })
            .chain(
                status
                    .pending_repository
                    .then(|| "repository enrollment or inactive files".to_string()),
            )
            .collect(),
        conflicts: apply::describe_conflicts(&status.conflicts),
        declarations_changed: status.declarations_changed,
        last_error: status.last_error.clone(),
        failing_since: status.failing_since.clone(),
        consecutive_failures: status.consecutive_failures,
        application_failure: status.application_failure.clone(),
        validation_error: status.validation_error.clone(),
        skipped: status
            .skipped
            .iter()
            .filter_map(|skipped| {
                roots
                    .locate(&skipped.branch_path)
                    .path()
                    .map(|path| (crate::file::display_path(path), skipped.reason.clone()))
            })
            .collect(),
        notifications: crate::system::history::notify::summary(),
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
    let enabled = settings.history.enabled;
    if !enabled {
        return Ok(HistoryReport {
            enabled: false,
            tracked_entries: 0,
            tracked_files: 0,
            local_entries: 0,
            omitted: vec![],
            nested: vec![],
            checkpoints: 0,
            latest: None,
            unsaved: None,
            not_applied: vec![],
            pending_operations: 0,
            watcher: super::capture_health::Watcher::NotDeclared,
            unavailable: None,
            health: None,
            sync: None,
        });
    }
    let (store, tracked, entries) = super::history::open().await?;
    let walk = tracked.walk()?;
    walk.report_warnings();
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
    // edits nothing has saved yet, which a stopped watcher or a manual-save
    // entry leaves behind; worked out like `mise dot history diff`
    let snapshot = entries
        .iter()
        .rev()
        .find_map(|entry| entry.checkpoint.tree.snapshot.as_deref());
    let not_applied = match (store.repo(), snapshot) {
        (Some(repo), Some(snapshot)) => {
            match crate::system::history::held::unheld_entries(repo, snapshot, &walk) {
                Ok(entries) => entries.into_iter().map(|(_, display)| display).collect(),
                Err(err) => {
                    debug!("history: could not compare the working tree: {err:#}");
                    vec![]
                }
            }
        }
        _ => vec![],
    };
    let unsaved = match (store.repo(), snapshot) {
        (Some(repo), Some(snapshot)) => {
            match crate::system::history::replay::unsaved_paths(repo, &tracked, &walk, snapshot) {
                Ok(unsaved) => unsaved,
                Err(err) => {
                    debug!("history: could not compare the working tree: {err:#}");
                    None
                }
            }
        }
        _ => None,
    };
    Ok(HistoryReport {
        enabled,
        tracked_entries: tracked.entries.len(),
        tracked_files,
        local_entries: tracked.local.len(),
        omitted: walk.omitted,
        nested: walk.nested,
        checkpoints: entries.len(),
        latest,
        unsaved,
        not_applied,
        pending_operations,
        watcher: super::capture_health::watcher().await?,
        unavailable: store.unavailable().map(str::to_string),
        health: crate::system::history::health::read(store.state_dir()),
        sync: sync_report(&store)?,
    })
}

fn plural(count: impl Into<u64>, noun: &str) -> String {
    let count = count.into();
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

fn push_step(next: &mut Vec<String>, step: &str) {
    if !next.iter().any(|existing| existing == step) {
        next.push(step.to_string());
    }
}

const OK: &str = "✓";
const WARN: &str = "⚠";
const BAD: &str = "✗";

/// Prints the history half of `mise dot status` as sections, each led by its
/// state, ending with the commands that would clear what needs attention.
pub(crate) fn print(report: &HistoryReport) -> Result<()> {
    if !report.enabled {
        miseprintln!("History: disabled (history.enabled = false).");
        return Ok(());
    }
    let mut next: Vec<String> = vec![];
    print_tracking(report, &mut next)?;
    if let Some(sync) = &report.sync {
        print_repository(sync, &mut next)?;
        print_conflicts(sync, &mut next)?;
    } else {
        miseprintln!();
        miseprintln!("Repository");
        miseprintln!("  none (`mise dot origin set <url>` synchronizes committed history).");
    }
    print_capture(report, &mut next)?;
    if let Some(sync) = &report.sync
        && sync.declarations_changed
    {
        miseprintln!();
        miseprintln!("Bootstrap {WARN}");
        miseprintln!("  declarations changed: run `mise bootstrap` (dry-run available)");
        push_step(&mut next, "mise bootstrap");
    }
    if !next.is_empty() {
        miseprintln!();
        miseprintln!("Next steps");
        for step in &next {
            miseprintln!("  → {step}");
        }
    }
    Ok(())
}

fn print_tracking(report: &HistoryReport, next: &mut Vec<String>) -> Result<()> {
    miseprintln!();
    miseprintln!("Tracking");
    if let Some(reason) = &report.unavailable {
        miseprintln!("  {BAD} capture is unavailable: {reason}");
    }
    let omitted = report.omitted.len();
    miseprintln!(
        "  {OK} {} · {}{}",
        if report.tracked_entries == 1 {
            "1 entry".to_string()
        } else {
            format!("{} entries", report.tracked_entries)
        },
        plural(report.tracked_files, "file"),
        if omitted > 0 {
            format!(" · {omitted} omitted")
        } else {
            String::new()
        }
    );
    if report.local_entries > 0 {
        miseprintln!(
            "  local-only: {} entries kept on this machine (`mise dot --local status`)",
            report.local_entries
        );
    }
    match &report.latest {
        Some(latest) => miseprintln!(
            "  {OK} {} · latest {} ({}, {}): {}",
            plural(report.checkpoints as u64, "checkpoint"),
            latest.id,
            latest.trigger,
            local_time(&latest.created_at),
            latest.description
        ),
        None => {
            miseprintln!("  {WARN} no checkpoint recorded yet");
            if report.tracked_entries > 0 {
                push_step(next, "mise dot save");
            }
        }
    }
    if let Some(unsaved) = report
        .unsaved
        .as_ref()
        .filter(|unsaved| !unsaved.is_empty())
    {
        const SHOWN: usize = 5;
        let mut listed = unsaved.iter().take(SHOWN).cloned().collect::<Vec<_>>();
        if unsaved.len() > SHOWN {
            listed.push(format!("and {} more", unsaved.len() - SHOWN));
        }
        miseprintln!(
            "  {WARN} unsaved changes: {} path(s) since the latest checkpoint ({}); `mise dot history diff` shows them",
            unsaved.len(),
            listed.join(", ")
        );
        push_step(next, "mise dot save");
    }
    if !report.not_applied.is_empty() {
        miseprintln!(
            "  {WARN} not applied here: {} (saved by another machine while not selected here); saves resume after a pull applies them",
            report.not_applied.join(", ")
        );
        push_step(next, "mise dot pull");
    }
    if omitted > 0 || !report.nested.is_empty() {
        miseprintln!(
            "  {}.",
            crate::system::history::tracked::omission_summary(&report.omitted, &report.nested)
        );
    }
    if report.pending_operations > 0 {
        miseprintln!(
            "  {WARN} {} operation(s) did not finish; `mise dot history --pending` lists them",
            report.pending_operations
        );
        push_step(next, "mise dot recover");
    }
    Ok(())
}

fn print_repository(sync: &SyncReport, next: &mut Vec<String>) -> Result<()> {
    let when = |at: &Option<String>| {
        at.as_deref()
            .map(local_time)
            .unwrap_or_else(|| "never".into())
    };
    miseprintln!();
    miseprintln!("Repository");
    miseprintln!("  {}", sync.origin);
    miseprintln!("  Branch: {} · Mode: {}", sync.branch, sync.mode);
    miseprintln!("  ↑ Last published: {}", when(&sync.last_publish));
    miseprintln!("  ↓ Last fetched: {}", when(&sync.last_fetch));
    miseprintln!("  ↓ Last pulled: {}", when(&sync.last_apply));
    miseprintln!("  notifications: {}", sync.notifications);
    if !sync.pending_applications.is_empty() {
        miseprintln!(
            "  {WARN} {} incoming change(s) pending ({})",
            sync.pending_applications.len(),
            sync.pending_applications.join(", ")
        );
        push_step(next, "mise dot pull");
    }
    if let Some(error) = &sync.last_error {
        match &sync.failing_since {
            Some(since) => miseprintln!(
                "  {BAD} last error: {error} (failing since {}, {} attempt(s))",
                local_time(since),
                sync.consecutive_failures
            ),
            None => miseprintln!("  {BAD} last error: {error}"),
        }
    }
    if let Some(error) = &sync.application_failure {
        miseprintln!("  {BAD} sync paused: {error}");
    }
    if let Some(error) = &sync.validation_error {
        miseprintln!("  {BAD} incoming setup is invalid: {error}");
    }
    for (path, reason) in &sync.skipped {
        miseprintln!("  not shared: {path} ({reason})");
    }
    Ok(())
}

fn print_conflicts(sync: &SyncReport, next: &mut Vec<String>) -> Result<()> {
    if sync.conflicts.is_empty() {
        return Ok(());
    }
    miseprintln!();
    miseprintln!("Conflicts ({}) {BAD}", sync.conflicts.len());
    for (path, reason) in &sync.conflicts {
        let advice = crate::system::history::sync::apply::resolution_advice(path, reason);
        miseprintln!("  {BAD} {path}");
        miseprintln!("    {reason}");
        miseprintln!("    {advice}");
    }
    miseprintln!("  Sharing is paused for the entire setup; local history continues.");
    next.insert(
        0,
        format!(
            "Resolve {} conflict(s) as described under Conflicts",
            sync.conflicts.len()
        ),
    );
    Ok(())
}

fn print_capture(report: &HistoryReport, next: &mut Vec<String>) -> Result<()> {
    use super::capture_health::Watcher;
    let failing = report
        .health
        .as_ref()
        .is_some_and(|health| health.failing_capture().is_some());
    miseprintln!();
    if failing && report.watcher == Watcher::Running {
        miseprintln!("Automatic capture {BAD}");
        miseprintln!(
            "  {}, but failing (edits are not being saved until a capture succeeds).",
            report.watcher.as_str()
        );
    } else if report.watcher == Watcher::Running {
        miseprintln!("Automatic capture {OK}");
        miseprintln!(
            "  {} ({}).",
            report.watcher.as_str(),
            super::capture_health::advice(report.watcher)
        );
    } else {
        miseprintln!("Automatic capture {WARN}");
        miseprintln!(
            "  {} ({}).",
            report.watcher.as_str(),
            super::capture_health::advice(report.watcher)
        );
        if report.watcher == Watcher::NotDeclared {
            next.push(
                "declare `history-watch` under [bootstrap.services] and run `mise bootstrap`"
                    .to_string(),
            );
        } else {
            next.push("mise bootstrap services apply".to_string());
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
            if report.watcher == Watcher::Running {
                ""
            } else {
                "; the watcher is not running now, so this is what it last reported"
            }
        );
        let running = report.watcher == Watcher::Running;
        // a running watcher starts from its predecessor's record: an error
        // dated before this run is not its failure
        let error = if running {
            health.failing_capture()
        } else {
            w.last_error.as_deref()
        };
        if let Some(error) = error {
            miseprintln!(
                "  last capture failure: {error} ({} consecutive; at {}). Edits since then are not protected.",
                w.consecutive_failures,
                w.last_error_at
                    .as_deref()
                    .map(local_time)
                    .unwrap_or_else(|| "unknown".into())
            );
            if running && crate::system::history::health::is_stale_watcher_schema_error(error) {
                miseprintln!(
                    "  outdated: {}.",
                    crate::system::history::health::STALE_WATCHER_SCHEMA_ADVICE
                );
            }
        }
        if running && w.executable_gone {
            miseprintln!(
                "  outdated: {}.",
                crate::system::history::health::EXECUTABLE_GONE_ADVICE
            );
        }
        for degraded in &w.degraded {
            miseprintln!("  degraded: {degraded}");
        }
        for throttled in &health.throttled {
            miseprintln!(
                "  throttled: {} changes constantly; saved every {} ({} unsaved change(s), last saved {}). Not a failure: `mise dot exclude '{}'` if it is a log, cache, or database, or track it with --no-autosave and save explicitly.",
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

fn is_zero(count: &usize) -> bool {
    *count == 0
}
