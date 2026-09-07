use std::path::PathBuf;

use eyre::Result;

use crate::system::history::replay::{self, RollbackRequest};

/// Return files to the version a checkpoint holds
///
/// Without `--to`, each path returns to its most recent saved version that
/// differs from what is on disk; unrelated checkpoints never influence the
/// choice. With `--to <ref>`, the named checkpoint is the source, and
/// `--all` selects everything it covers. The current state is saved in a
/// protective checkpoint first, so `mise bootstrap dotfiles undo` can reverse it.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct DotfilesRollback {
    /// Paths to roll back (files or directories)
    #[usage(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// The checkpoint to roll back to: id, `latest`, `latest~N`, or a uuid prefix
    #[usage(long, value_name = "REF")]
    to: Option<String>,

    /// With --to: everything the checkpoint covers
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
        replay::rollback(RollbackRequest {
            paths: self.paths,
            to: self.to,
            all: self.all,
            dry_run: self.dry_run,
            yes: self.yes,
            force: self.force,
        })
        .await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles rollback ~/.config/hypr/bindings.lua</bold>
    $ <bold>mise bootstrap dotfiles rollback ~/.zshrc --to 42</bold>
    $ <bold>mise bootstrap dotfiles rollback --to latest~3 --all --dry-run</bold>
"#
);
