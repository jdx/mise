use eyre::Result;

use super::{display_arg, local_time};
use crate::system::history::store::Trigger;
use crate::ui::table::MiseTable;

/// List checkpoints, newest first
#[derive(Debug, usage_rs::Args)]
#[usage(visible_alias = "list", verbatim_doc_comment)]
pub(crate) struct HistoryLs {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Show at most this many checkpoints (0 for all)
    #[usage(long, short = 'n', default_value_t = 20, default = "20")]
    limit: usize,

    /// Only checkpoints where this path (or something under it) changed
    #[usage(long, value_name = "PATH")]
    path: Option<String>,

    /// Only checkpoints with this trigger (edit, save, bootstrap, …)
    #[usage(long, value_name = "TRIGGER")]
    trigger: Option<String>,

    /// Only checkpoints recorded by operations that did not finish
    #[usage(long)]
    pending: bool,
}

impl HistoryLs {
    pub(crate) async fn run(self) -> Result<()> {
        let (_store, _tracked, mut entries) = super::open().await?;
        entries.reverse();
        if let Some(path) = &self.path {
            let path = display_arg(path);
            entries.retain(|entry| entry.checkpoint.changes.touches(&path));
        }
        if let Some(trigger) = &self.trigger {
            let Some(trigger) = Trigger::parse(trigger) else {
                eyre::bail!("unknown trigger {trigger:?}");
            };
            entries.retain(|entry| entry.checkpoint.trigger == trigger);
        }
        if self.pending {
            entries.retain(|entry| {
                entry.checkpoint.status()
                    == Some(crate::system::history::store::OperationStatus::Pending)
            });
        }
        if self.limit > 0 {
            entries.truncate(self.limit);
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&entries)?);
            return Ok(());
        }
        if entries.is_empty() {
            info!("no history checkpoints recorded");
            return Ok(());
        }
        let mut table = MiseTable::new(false, &["ID", "When", "Trigger", "Description", "Files"]);
        for entry in &entries {
            let checkpoint = &entry.checkpoint;
            let files = if checkpoint.tree.available {
                let changed = checkpoint.changes.len();
                if changed == 0 {
                    "-".to_string()
                } else {
                    changed.to_string()
                }
            } else {
                "unavailable".to_string()
            };
            let status = match checkpoint.status() {
                Some(status)
                    if status != crate::system::history::store::OperationStatus::Completed =>
                {
                    format!(" ({})", status.as_str())
                }
                _ => String::new(),
            };
            table.add_row(vec![
                entry.id.to_string(),
                local_time(&checkpoint.created_at),
                format!("{}{status}", checkpoint.kind_label()),
                checkpoint.description.clone(),
                files,
            ]);
        }
        table.print()
    }
}
