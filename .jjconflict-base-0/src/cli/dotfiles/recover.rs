use std::collections::BTreeSet;

use eyre::{Result, bail};

use crate::system::history::{
    checkpoint::Store, journal::JournalEntry, scope, store, tracked::TrackedSet,
};

/// Recover an interrupted dotfile operation
///
/// Retries safe recovery without overwriting later edits. If recovery cannot
/// determine what is safe, inspect the listed files first. `--keep-current`
/// explicitly accepts their live contents and discards only the selected
/// operation's temporary recovery copies; it does not erase Git history.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct DotfilesRecover {
    /// Pending numeric ID or an unambiguous operation UUID prefix
    #[usage(value_name = "OPERATION")]
    operation: Option<String>,

    /// Accept live files instead of restoring temporary recovery copies
    #[usage(long)]
    keep_current: bool,

    /// Confirm discarding the selected operation's temporary recovery copies
    #[usage(long, short = 'y')]
    yes: bool,
}

impl DotfilesRecover {
    pub(crate) async fn run(self) -> Result<()> {
        if !store::store_dir_in(&crate::dirs::STATE).exists() {
            info!("dotfiles: no interrupted operations");
            return Ok(());
        }
        let tracked = TrackedSet::effective().await?;
        tokio::task::spawn_blocking(move || self.recover(&tracked)).await?
    }

    fn recover(self, tracked: &TrackedSet) -> Result<()> {
        let store = Store::open()?;
        let _lock = scope::recovery_lock(&store)?;
        let pending = store::list_pending_in(store.state_dir())?;
        if pending.is_empty() {
            info!("dotfiles: no interrupted operations");
            return Ok(());
        }
        let selected = pending
            .iter()
            .filter(|(_, record)| {
                self.operation.as_ref().is_none_or(|selector| {
                    matches_operation(selector, record.id, &record.checkpoint.uuid)
                })
            })
            .collect::<Vec<_>>();
        if selected.is_empty() {
            bail!(
                "no matching interrupted operation; `mise bootstrap dotfiles history --pending` lists them"
            );
        }
        for (_, record) in &selected {
            miseprintln!("Interrupted operation {}", record.checkpoint.uuid);
            if let Some(operation) = &record.checkpoint.operation {
                let paths = operation
                    .journal
                    .iter()
                    .filter_map(|entry| match entry {
                        JournalEntry::PathChanged { path, .. } => Some(path),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>();
                for path in paths {
                    miseprintln!("  {}", crate::file::display_path(path));
                }
            }
        }
        if selected.len() > 1 && (self.keep_current || self.operation.is_some()) {
            bail!("select one operation by its identifier before accepting current files");
        }
        if self.keep_current
            && !self.yes
            && !crate::ui::prompt::confirm(
                "Keep these live files and discard this operation's temporary recovery copies?",
            )?
            .is_yes()
        {
            info!("dotfiles: recovery copies preserved");
            return Ok(());
        }
        for (_, record) in selected {
            scope::recover_operation(&store, tracked, &record.checkpoint.uuid, self.keep_current)?;
            if store::list_pending_in(store.state_dir())?
                .iter()
                .any(|(_, pending)| pending.checkpoint.uuid == record.checkpoint.uuid)
            {
                bail!(
                    "operation {} still needs attention; its pending record was preserved",
                    record.checkpoint.uuid
                );
            }
            info!("dotfiles: recovered operation {}", record.checkpoint.uuid);
        }
        Ok(())
    }
}

fn matches_operation(selector: &str, id: u64, uuid: &str) -> bool {
    selector.parse::<u64>().ok() == Some(id) || uuid.starts_with(selector)
}

#[cfg(test)]
mod tests {
    use super::matches_operation;

    #[test]
    fn accepts_listed_numeric_ids_and_operation_uuid_prefixes() {
        assert!(matches_operation("42", 42, "abc-123"));
        assert!(matches_operation("abc", 42, "abc-123"));
        assert!(!matches_operation("43", 42, "abc-123"));
        // Matching both records is intentionally possible: the caller rejects
        // an ambiguous selection rather than discarding the wrong copies.
        assert!(matches_operation("42", 43, "42abc-123"));
    }
}
