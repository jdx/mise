use eyre::Result;

use crate::system::history::replay::{self, UndoRequest};

/// Reverse a rollback or undo
///
/// Restores exactly the paths that operation changed from the protective
/// checkpoint it took, leaving everything else as it is now. Without a
/// reference, the newest operation not yet undone is reversed.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct DotfilesUndo {
    /// The operation's checkpoint: id, `latest`, `latest~N`, or a uuid prefix
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
            yes: self.yes,
        })
        .await
    }
}
