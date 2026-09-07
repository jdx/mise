use std::path::PathBuf;

use eyre::Result;
use serde::Serialize;

use crate::file::display_path;
use crate::system::files::FileMode;
use crate::system::history::tracked::{EntryKind, Policy, TrackedEntry, TrackedSet};
use crate::ui::table::MiseTable;

/// Show what history tracks and under which policies
///
/// Every entry is listed with the file that declared it, its policies, and
/// how many files it currently covers. Declarations that history could not
/// honour are listed as invalid, omitted, or incomplete, so a failed
/// enrollment is never mistaken for protection.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct DotfilesPaths {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Show what tracking this path would capture
    #[usage(long, value_name = "PATH")]
    preview: Option<PathBuf>,

    /// List the paths the watcher found changing constantly
    #[usage(long)]
    noisy: bool,
}

#[derive(Serialize)]
struct PathRow {
    path: String,
    mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    autosave: bool,
    share: bool,
    backup: bool,
    files: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_in: Option<String>,
}

impl DotfilesPaths {
    pub(crate) async fn run(self) -> Result<()> {
        if self.noisy {
            return self.print_noisy();
        }
        let tracked = match &self.preview {
            Some(path) => {
                let mut set = TrackedSet {
                    exclude: crate::system::history::config::exclude_globs()?,
                    ..Default::default()
                };
                set.push(TrackedEntry {
                    path: crate::system::history::tracked::normalize(path),
                    kind: EntryKind::Track,
                    mode: "track".into(),
                    policy: Policy::for_mode(FileMode::Track),
                    variant: None,
                    source: None,
                    note: None,
                    declared_in: None,
                });
                set
            }
            None => TrackedSet::effective().await?,
        };
        let walk = tracked.walk()?;
        let mut counts = vec![0u64; tracked.entries.len()];
        for (owner, _) in walk.files.values() {
            if let Some(count) = counts.get_mut(*owner) {
                *count += 1;
            }
        }
        let rows: Vec<PathRow> = tracked
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| PathRow {
                path: entry.display(),
                mode: entry.mode.clone(),
                variant: entry.variant.clone(),
                source: entry.source.as_deref().map(display_path),
                autosave: entry.policy.autosave,
                share: entry.policy.share,
                backup: entry.policy.backup,
                files: counts[index],
                note: entry.note.clone(),
                declared_in: entry.declared_in.as_deref().map(display_path),
            })
            .collect();
        if self.json {
            let out = serde_json::json!({
                "entries": rows,
                "exclude": tracked.exclude,
                "derived": walk.derived,
                "private": walk.private.iter().map(|private| serde_json::json!({
                    "path": display_path(&private.path),
                    "reason": private.reason,
                    "share": private.policy.share,
                    "backup": private.policy.backup,
                })).collect::<Vec<_>>(),
                "invalid": tracked.invalid,
                "omitted": walk.omitted,
                "incomplete": walk.incomplete,
            });
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }
        if let Some(path) = &self.preview {
            miseprintln!("Tracking {} would capture:", display_path(path));
            for root in &walk.roots {
                for rel in &root.files {
                    miseprintln!("  {}", display_path(root.path.join(rel)));
                }
            }
        } else {
            let mut table =
                MiseTable::new(false, &["Path", "Mode", "Policy", "Files", "Declared in"]);
            for row in &rows {
                let mode = match &row.variant {
                    Some(variant) => format!("{} ({variant})", row.mode),
                    None => row.mode.clone(),
                };
                let mut policy = super::history::show::policy(row.autosave, row.share, row.backup);
                if let Some(note) = &row.note {
                    policy = format!("{policy}: {note}");
                }
                table.add_row(vec![
                    row.path.clone(),
                    mode,
                    policy,
                    row.files.to_string(),
                    row.declared_in.clone().unwrap_or_else(|| "-".into()),
                ]);
            }
            table.print()?;
            for glob in &tracked.exclude {
                miseprintln!("  exclude: {glob}");
            }
            for derived in &walk.derived {
                miseprintln!("  derived: {} (target of {})", derived.path, derived.from);
            }
            for private in &walk.private {
                miseprintln!(
                    "  private: {} ({}; {})",
                    display_path(&private.path),
                    private.reason,
                    super::history::show::policy(
                        private.policy.autosave,
                        private.policy.share,
                        private.policy.backup
                    )
                );
            }
        }
        for invalid in &tracked.invalid {
            miseprintln!("  invalid: {} ({})", invalid.path, invalid.reason);
        }
        for omitted in &walk.omitted {
            miseprintln!("  omitted: {} ({})", omitted.path, omitted.reason);
        }
        for incomplete in &walk.incomplete {
            miseprintln!("  incomplete: {} ({})", incomplete.path, incomplete.reason);
        }
        Ok(())
    }
}

impl DotfilesPaths {
    fn print_noisy(&self) -> Result<()> {
        use crate::system::history::watch::{noise, runtime};
        let record = noise::read(&runtime::noisy_path_in(
            &crate::system::history::store::state_dir(),
        ));
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&record.paths)?);
            return Ok(());
        }
        if record.paths.is_empty() {
            info!("the watcher is not throttling any path");
            return Ok(());
        }
        let mut table = MiseTable::new(
            false,
            &["Path", "Saving every", "Unsaved changes", "Last seen"],
        );
        for (path, noisy) in &record.paths {
            table.add_row(vec![
                path.clone(),
                crate::system::history::watch::runtime::humantime(std::time::Duration::from_secs(
                    noisy.interval_secs,
                )),
                noisy.pending_changes.to_string(),
                noisy.last_seen.clone(),
            ]);
        }
        table.print()?;
        miseprintln!(
            "These paths keep changing, so the watcher saves them ever more rarely (never excluded or switched to manual saving on its own). Exclude a log, cache, or database with `mise bootstrap dotfiles exclude '<glob>'`; track configuration that changes constantly with `--no-autosave` and save it explicitly."
        );
        Ok(())
    }
}

pub(crate) fn edit_exclude(glob: &str, add: bool) -> Result<()> {
    let glob = glob.trim();
    if glob.is_empty() {
        eyre::bail!("a glob is required");
    }
    let global = crate::config::global_config_path();
    let changed = crate::cli::dotfiles::track::edit_exclude(glob, add)?;
    match (add, changed) {
        (true, true) => info!(
            "history: {glob} is excluded from capture ({})",
            display_path(&global)
        ),
        (true, false) => info!("history: {glob} was already excluded"),
        (false, true) => info!(
            "history: {glob} is captured again ({})",
            display_path(&global)
        ),
        (false, false) => info!("history: {glob} was not excluded"),
    }
    Ok(())
}
