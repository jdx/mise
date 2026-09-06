use eyre::{Result, bail};

use super::display_arg;
use crate::system::history::shadow::DiffOpts;
use crate::system::history::tracked::display_to_tree_path;

/// Compare checkpoints, or the working tree against one
///
/// Without arguments, shows what changed by hand since the latest
/// checkpoint. With one reference, shows what that checkpoint changed
/// against the one before it. With two, compares the two states.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct HistoryDiff {
    /// Checkpoint id, `latest`, `latest~N`, or a uuid prefix
    #[usage(value_name = "A")]
    a: Option<String>,

    /// Compare `A` with this checkpoint instead of its predecessor
    #[usage(value_name = "B")]
    b: Option<String>,

    /// Compare an operation with its recorded protective checkpoint
    ///
    /// With no reference, use the newest operation, ignoring later saves.
    /// With one reference, use that operation. Fails if its before checkpoint
    /// is unavailable instead of comparing an unrelated preceding save.
    #[usage(long)]
    operation: bool,

    /// Print the full patch instead of a per-file summary
    #[usage(long, short)]
    patch: bool,

    /// Restrict to one path (a file or a directory)
    #[usage(long, value_name = "PATH")]
    path: Option<String>,

    /// Exit 1 when the two sides differ
    #[usage(long)]
    exit_code: bool,
}

impl HistoryDiff {
    pub(crate) async fn run(self) -> Result<()> {
        let (store, tracked, entries) = super::open().await?;
        let Some(repo) = store.repo() else {
            bail!("comparing checkpoints requires git");
        };
        let path = self.path.as_deref().map(display_arg);
        let (from, to, label) =
            if self.operation {
                if self.b.is_some() {
                    bail!("--operation takes at most one checkpoint reference");
                }
                let outcome = match &self.a {
                    Some(reference) => super::resolve(reference, &entries, None)?,
                    None => entries
                        .iter()
                        .rev()
                        .find(|entry| entry.checkpoint.operation.is_some())
                        .cloned()
                        .ok_or_else(|| eyre::eyre!("no recorded operation"))?,
                };
                let operation =
                    outcome.checkpoint.operation.as_ref().ok_or_else(|| {
                        eyre::eyre!("checkpoint {} is not an operation", outcome.id)
                    })?;
                let before = operation
                    .before
                    .as_ref()
                    .and_then(|uuid| entries.iter().find(|entry| &entry.checkpoint.uuid == uuid))
                    .ok_or_else(|| {
                        eyre::eyre!("operation's protective checkpoint is unavailable or pruned")
                    })?;
                (
                    tree_of(before)?,
                    tree_of(&outcome)?,
                    format!(
                        "operation {}: checkpoint {} -> {}",
                        outcome.id, before.id, outcome.id
                    ),
                )
            } else {
                match (&self.a, &self.b) {
                    (None, _) => {
                        let latest = super::resolve("latest", &entries, path.as_deref())?;
                        let tree = tree_of(&latest)?;
                        let walk = tracked.walk()?;
                        let current = repo.capture(&walk.roots)?;
                        (
                            tree,
                            current.tree,
                            format!("checkpoint {} -> working tree", latest.id),
                        )
                    }
                    (Some(a), None) => {
                        let a = super::resolve(a, &entries, path.as_deref())?;
                        let previous = entries.iter().rev().find(|entry| {
                            entry.id < a.id && entry.checkpoint.tree.snapshot.is_some()
                        });
                        let previous = match previous {
                            Some(previous) => tree_of(previous)?,
                            None => repo.empty_object("tree")?,
                        };
                        (previous, tree_of(&a)?, format!("checkpoint {}", a.id))
                    }
                    (Some(a), Some(b)) => {
                        let a = super::resolve(a, &entries, path.as_deref())?;
                        let b = super::resolve(b, &entries, path.as_deref())?;
                        (
                            tree_of(&a)?,
                            tree_of(&b)?,
                            format!("checkpoint {} -> {}", a.id, b.id),
                        )
                    }
                }
            };
        let paths = path.as_deref().map(|path| {
            let tree_path = display_to_tree_path(path);
            (tree_path.clone(), tree_path)
        });
        let result = repo.diff(
            &from,
            &to,
            &DiffOpts {
                patch: self.patch,
                // a patch is written as git produces it; a summary is small
                stream: self.patch,
                color: console::colors_enabled(),
                paths,
            },
        )?;
        if result.changed {
            if !result.output.is_empty() {
                miseprint!("{}", String::from_utf8_lossy(&result.output))?;
            }
        } else {
            info!("{label}: no differences");
        }
        if self.exit_code && result.changed {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

fn tree_of(entry: &crate::system::history::store::Entry) -> Result<String> {
    entry
        .checkpoint
        .tree
        .snapshot
        .clone()
        .ok_or_else(|| eyre::eyre!("checkpoint {} has no content snapshot", entry.id))
}
