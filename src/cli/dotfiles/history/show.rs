use eyre::{Result, bail};

use super::{display_arg, local_time, short};
use crate::system::history::journal;
use crate::ui::table::MiseTable;

/// Show one checkpoint: what triggered it, what changed, and its journal
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct HistoryShow {
    /// Checkpoint id, `latest` (the default), `latest~N`, or a uuid prefix
    #[usage(value_name = "REF")]
    reference: Option<String>,

    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// List every file in the snapshot
    #[usage(long)]
    files: bool,

    /// Resolve `latest~N` among the checkpoints where this path changed
    #[usage(long, value_name = "PATH")]
    path: Option<String>,
}

impl HistoryShow {
    pub(crate) async fn run(self) -> Result<()> {
        let (store, _tracked, entries) = super::open().await?;
        let path = self.path.as_deref().map(display_arg);
        let entry = super::resolve(
            self.reference.as_deref().unwrap_or("latest"),
            &entries,
            path.as_deref(),
        )?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&entry)?);
            return Ok(());
        }
        let c = &entry.checkpoint;
        miseprintln!("Checkpoint {} ({})", entry.id, c.trigger.as_str());
        miseprintln!("  Description: {}", c.description);
        miseprintln!("  When:        {}", local_time(&c.created_at));
        miseprintln!("  UUID:        {}", c.uuid);
        miseprintln!("  Machine:     {}", c.machine.name);
        if !c.labels.is_empty() {
            miseprintln!("  Labels:      {}", c.labels.join(", "));
        }
        if c.pinned {
            miseprintln!("  Pinned:      yes");
        }
        if let Some(operation) = &c.operation {
            miseprintln!(
                "  Operation:   {} ({})",
                operation.kind.as_str(),
                operation.status.as_str()
            );
            miseprintln!("  Command:     mise {}", operation.command);
            if let Some(before) = &operation.before
                && let Some(before) = entries
                    .iter()
                    .find(|entry| &entry.checkpoint.uuid == before)
            {
                miseprintln!("  Before:      checkpoint {}", before.id);
            }
            if let Some(message) = &operation.message {
                miseprintln!("  Note:        {message}");
            }
            if let Some(error) = &operation.error {
                miseprintln!("  Error:       {error}");
            }
        }
        if c.tree.available {
            miseprintln!(
                "  Snapshot:    {} ({} files)",
                c.tree
                    .snapshot
                    .as_deref()
                    .map(short)
                    .unwrap_or_else(|| "-".into()),
                c.tree.roots.iter().map(|root| root.files).sum::<u64>()
            );
        } else {
            miseprintln!(
                "  Snapshot:    unavailable ({})",
                c.tree.reason.as_deref().unwrap_or("unknown reason")
            );
        }
        let changes = &c.changes;
        if !changes.is_empty() {
            miseprintln!("");
            miseprintln!("Changes since the previous checkpoint:");
            for path in &changes.modified {
                miseprintln!("  M {path}");
            }
            for path in &changes.added {
                miseprintln!("  A {path}");
            }
            for path in &changes.removed {
                miseprintln!("  D {path}");
            }
            if changes.truncated {
                miseprintln!("  … (list truncated)");
            }
        }
        let coverage = &c.tree.coverage;
        if !coverage.entries.is_empty() {
            miseprintln!("");
            let mut table = MiseTable::new(false, &["Tracked", "Mode", "Policy", "State"]);
            for entry in &coverage.entries {
                table.add_row(vec![
                    entry.path.clone(),
                    entry.mode.clone(),
                    policy(entry.autosave, entry.share, entry.backup),
                    entry.state.clone(),
                ]);
            }
            table.print()?;
        }
        for omitted in &coverage.omitted {
            miseprintln!("  omitted: {} ({})", omitted.path, omitted.reason);
        }
        for incomplete in &coverage.incomplete {
            miseprintln!("  incomplete: {} ({})", incomplete.path, incomplete.reason);
        }
        if let Some(operation) = &c.operation
            && !operation.journal.is_empty()
        {
            miseprintln!("");
            miseprintln!("Journal:");
            for line in journal::render(&operation.journal) {
                miseprintln!("  - {line}");
            }
        }
        if self.files {
            let Some(tree) = &c.tree.snapshot else {
                bail!("checkpoint {} has no content snapshot", entry.id);
            };
            let Some(repo) = store.repo() else {
                bail!("listing snapshot files requires git");
            };
            miseprintln!("");
            miseprintln!("Files:");
            for file in repo.ls_tree(tree)? {
                let size = file.size.map(|size| size.to_string()).unwrap_or_default();
                miseprintln!(
                    "  {} {:>8} {}",
                    file.mode,
                    size,
                    crate::system::history::tracked::tree_path_to_display(&file.path)
                );
            }
        }
        Ok(())
    }
}

pub(crate) fn policy(autosave: bool, share: bool, backup: bool) -> String {
    let mut parts = vec![];
    if !autosave {
        parts.push("manual-save");
    }
    if !share {
        parts.push("no-share");
    }
    if !backup {
        parts.push("no-backup");
    }
    if parts.is_empty() {
        "default".into()
    } else {
        parts.join(",")
    }
}
