use std::collections::BTreeSet;

use eyre::{Result, bail};

use crate::system::history::{
    checkpoint::Store, journal::JournalEntry, scope, store, tracked::TrackedSet,
};

/// Recover an interrupted dotfile operation
///
/// Finishes recovering from an operation that was interrupted, for example by
/// a crash or Ctrl-C. With no argument, recovers every interrupted operation.
/// mise restores files from the operation's recovery copies unless you have
/// edited them since. When it cannot tell which version is safe, it lists the
/// files: check them, then rerun with `--keep-current` to keep what is on disk
/// and discard the operation's recovery copies. When several operations were
/// interrupted, `--keep-current` needs the ID of one. Git history is not
/// changed.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise dot recover", help = "Recover every interrupted operation"),
    example(
        "mise dot recover 42 --keep-current --yes",
        help = "Keep the files on disk for operation 42"
    )
)]
pub(crate) struct DotfilesRecover {
    /// The operation's checkpoint ID (see `mise dot history --pending`) or a unique
    /// prefix of its UUID
    #[usage(value_name = "OPERATION")]
    operation: Option<String>,

    /// Keep the files on disk instead of restoring the recovery copies
    #[usage(long)]
    keep_current: bool,

    /// Discard the operation's recovery copies without prompting
    #[usage(long, short = 'y')]
    yes: bool,
}

impl DotfilesRecover {
    pub(crate) async fn run(self) -> Result<()> {
        if !store::store_dir_in(&crate::system::history::local::root()).exists() {
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
            bail!("no matching interrupted operation; `mise dot history --pending` lists them");
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
            && !crate::config::Settings::get().yes
            && !crate::ui::prompt::confirm_destructive(
                "Keep these live files and discard this operation's temporary recovery copies?",
                "mise dot recover --keep-current",
            )?
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
