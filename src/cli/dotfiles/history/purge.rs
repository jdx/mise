use std::path::PathBuf;

use eyre::{Result, bail};

use super::short;
use crate::system::history::purge::{self, Options, Outcome, Target};

/// Erase a path's saved versions from history
///
/// Rewrites the history without the path, for a file you tracked by mistake
/// that holds a secret. `untrack`, `rollback`, and `undo` only add
/// checkpoints; they leave the old versions readable.
///
/// Checkpoints that changed only this path are removed. The others stay, with
/// their descriptions, dates, and IDs, minus the path. Without `--yes`, the
/// command lists what would change and asks first.
///
/// mise never rewrites history that is on the origin: if a saved version of
/// the path was ever published, the command refuses. Rotate the secret, then
/// see https://mise.jdx.dev/dotfiles/encryption.html#remove-plaintext-from-history.
///
/// This does not stop tracking the path. If the file is still tracked, the
/// next save records its current contents again, so run `mise dot untrack`
/// first (or encrypt it). Stop the watcher service while this runs.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        "mise dot history purge ~/.aws/credentials --dry-run",
        help = "List the checkpoints that hold the file"
    ),
    example(
        "mise dot history purge ~/.aws/credentials --yes",
        help = "Erase every saved version of the file"
    )
)]
pub(crate) struct HistoryPurge {
    /// Files or directories to erase from history
    #[usage(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,

    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Rewrite without prompting
    #[usage(long, short)]
    yes: bool,

    /// Keep the replaced history in the repository under `refs/mise/purge-backup/`
    ///
    /// The backup still holds the content you are erasing; delete it with
    /// `git update-ref -d` once you are sure.
    #[usage(long)]
    backup: bool,

    /// Leave the replaced objects on disk instead of deleting them
    #[usage(long)]
    keep_objects: bool,
}

impl HistoryPurge {
    pub(crate) async fn run(self) -> Result<()> {
        let paths: Vec<PathBuf> = self
            .paths
            .iter()
            .map(|path| crate::system::files::resolve_target_arg(&path.to_string_lossy()))
            .collect();
        if crate::cli::dotfiles::route_local(&paths).await? {
            return Ok(());
        }
        let targets = paths
            .iter()
            .map(|path| Target::parse(&path.to_string_lossy()))
            .collect::<Result<Vec<_>>>()?;
        let (store, tracked, _entries) = super::open().await?;
        let preview = tokio::task::block_in_place(|| {
            purge::purge(&store, &tracked, &targets, Options::default())
        })?;
        if preview.is_noop() {
            info!("history: no saved version holds this path");
            return Ok(());
        }
        print_plan(&preview)?;
        if self.dry_run {
            return Ok(());
        }
        if !self.yes
            && !crate::config::Settings::get().yes
            && !crate::ui::prompt::confirm_destructive(
                "Erase these versions from history? This cannot be undone",
                "mise dot history purge",
            )?
        {
            bail!("cancelled");
        }
        let outcome = tokio::task::block_in_place(|| {
            purge::purge(
                &store,
                &tracked,
                &targets,
                Options {
                    apply: true,
                    backup: self.backup,
                    gc: !self.keep_objects,
                },
            )
        })?;
        info!(
            "history: erased {} checkpoint(s) and removed the path from {} more",
            outcome.dropped.len(),
            outcome.rewritten.len()
        );
        if let Some(backup) = &outcome.backup_ref {
            warn!("history: the replaced history is kept as {backup} and still holds the content");
        }
        for name in &outcome.other_refs {
            warn!("history: {name} still reaches the erased content");
        }
        Ok(())
    }
}

fn print_plan(outcome: &Outcome) -> Result<()> {
    let line = |verb: &str, affected: &[purge::Affected]| -> Result<()> {
        for item in affected {
            let id = item
                .id
                .map_or_else(|| short(&item.commit), |id| id.to_string());
            miseprintln!("  {verb} {id}  {}", item.description);
        }
        Ok(())
    };
    miseprintln!(
        "{} of {} checkpoints hold this path:",
        outcome.dropped.len() + outcome.rewritten.len(),
        outcome.total
    );
    line("remove ", &outcome.dropped)?;
    line("rewrite", &outcome.rewritten)?;
    if outcome.rehashed > 0 {
        miseprintln!(
            "{} later checkpoint(s) keep their contents and IDs but get new commit hashes",
            outcome.rehashed
        );
    }
    Ok(())
}
