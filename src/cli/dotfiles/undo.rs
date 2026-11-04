use eyre::Result;

use crate::system::history::replay::{self, UndoRequest};

/// Reverse the tracked-file changes from an operation
///
/// Restores exactly the paths that an operation changed from the checkpoint it
/// saved first, and leaves everything else as it is now. Without a reference,
/// reverses the newest operation that has not been undone.
///
/// Works for operations that changed tracked files: `mise bootstrap`,
/// `mise dot add`, `apply`, `edit`, `unapply`, `capture`, `rollback`, `pull`
/// (including changes the watcher applied), and `undo` itself. Package
/// installations, service state, and untracked files are not reversed.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise dot undo", help = "Reverse the newest operation"),
    example("mise dot undo --dry-run", help = "Show what would be restored"),
    example(
        "mise dot undo 42",
        help = "Reverse the operation recorded as checkpoint 42"
    )
)]
pub(crate) struct DotfilesUndo {
    /// The operation's checkpoint: numeric ID, `latest`, `latest~N`, or `commit:<sha>`
    #[usage(value_name = "REF")]
    reference: Option<String>,

    /// Show the plan without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Apply without prompting
    #[usage(long, short)]
    yes: bool,
}

impl DotfilesUndo {
    pub(crate) async fn run(self) -> Result<()> {
        replay::undo(UndoRequest {
            reference: self.reference,
            dry_run: self.dry_run,
            yes: self.yes || crate::config::Settings::get().yes,
        })
        .await
    }
}
