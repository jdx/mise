use std::path::PathBuf;

use eyre::Result;

use crate::system::history::replay::{self, RollbackRequest};

/// Return files to the version a checkpoint holds
///
/// Without `--to`, each path returns to its most recent saved version that
/// differs from what is on disk; other checkpoints do not affect the choice.
/// With `--to REF`, the named checkpoint is the source, and `--all` restores
/// everything it covers. mise saves the current state in a checkpoint first,
/// so `mise dot undo` can reverse the rollback.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        "mise dot rollback ~/.config/hypr/bindings.lua",
        help = "Restore the latest saved version that differs from the file"
    ),
    example(
        "mise dot rollback ~/.zshrc --to 42",
        help = "Restore ~/.zshrc as checkpoint 42 holds it"
    ),
    example(
        "mise dot rollback --to latest~3 --all --dry-run",
        help = "Preview restoring every file from three checkpoints ago"
    )
)]
pub(crate) struct DotfilesRollback {
    /// Paths to roll back (files or directories)
    #[usage(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// The checkpoint to roll back to: numeric ID, `latest`, `latest~N`, or `commit:<sha>`
    #[usage(long, value_name = "REF")]
    to: Option<String>,

    /// With `--to`, restore everything the checkpoint covers
    #[usage(long)]
    all: bool,

    /// Show the plan without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Apply without prompting
    #[usage(long, short)]
    yes: bool,

    /// Replace a path whose type changed (file, symlink, directory)
    #[usage(long)]
    force: bool,
}

impl DotfilesRollback {
    pub(crate) async fn run(self) -> Result<()> {
        if super::route_local(&self.paths).await? {
            return Ok(());
        }
        replay::rollback(RollbackRequest {
            paths: self.paths,
            to: self.to,
            all: self.all,
            dry_run: self.dry_run,
            yes: self.yes || crate::config::Settings::get().yes,
            force: self.force,
        })
        .await
    }
}
