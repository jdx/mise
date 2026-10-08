use std::path::PathBuf;

use eyre::{Result, bail};

use crate::system::history::sync::apply::{self, ApplyRequest};

/// Apply incoming changes from the origin to your files
///
/// Writes the changes that the last `mise dot sync` fetched from your other
/// machines. (`mise dot apply` deploys your own `[dotfiles]` entries; `pull`
/// writes what other machines pushed.) mise saves a checkpoint first, so
/// `mise dot undo` reverses the whole pull, and runs the matching
/// `[history.reload]` commands after writing.
///
/// A pull applies every incoming change together, including config files and
/// the files they reference; it does not take paths. Nothing is written if an
/// incoming config file does not parse, a file has unsaved local edits, your
/// own checkout has staged Git changes, or a file changed on both sides. A file
/// that changed on both sides is a conflict: compare the two versions with
/// `mise dot conflicts`, then resolve it with `--take-remote` or `--keep-local`,
/// or resolve every conflict with `--take-remote-all` or `--keep-local-all`.
/// While any conflict remains, pushing and applying stop for all tracked files;
/// local history and fetching continue.
///
/// In `sync` mode the watcher pulls on its own when there are no conflicts.
/// When incoming config declares more tracked files, their versions from the
/// origin are written in the same pull.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise dot pull --dry-run", help = "Show what would be written"),
    example(
        "mise dot pull --take-remote ~/.zshrc",
        help = "Resolve a conflict with the origin's version"
    ),
    example(
        "mise dot pull --keep-local ~/.zshrc",
        help = "Resolve a conflict by keeping this machine's version"
    ),
    example(
        "mise dot pull --take-remote-all --keep-local ~/.zshrc",
        help = "Take the origin's version of every conflict except one"
    )
)]
pub(crate) struct DotfilesPull {
    /// Not supported; a pull always applies every incoming change
    #[usage(value_name = "PATH", hide = true)]
    paths: Vec<PathBuf>,

    /// Show the plan without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Accepted for compatibility; pull skips the apply confirmation
    #[usage(long, short = 'y')]
    yes: bool,

    /// Resolve a conflict with the origin's version
    #[usage(long, value_name = "PATH")]
    take_remote: Vec<PathBuf>,

    /// Resolve a conflict by keeping this machine's version; the next sync pushes it
    #[usage(long, value_name = "PATH")]
    keep_local: Vec<PathBuf>,

    /// Resolve every remaining conflict with the origin's version
    ///
    /// Paths named by `--keep-local` keep this machine's version; every other
    /// conflict takes the origin's. Use it on a newly connected machine, where
    /// each existing file that differs is a separate conflict.
    #[usage(long, conflicts = "keep_local_all")]
    take_remote_all: bool,

    /// Resolve every remaining conflict by keeping this machine's version
    ///
    /// Paths named by `--take-remote` take the origin's version; every other
    /// conflict keeps this machine's. Each kept path must already be saved.
    #[usage(long, conflicts = "take_remote_all")]
    keep_local_all: bool,
}

impl DotfilesPull {
    pub(crate) async fn run(self) -> Result<()> {
        if !crate::config::Settings::get().history.enabled {
            bail!("history is disabled (history.enabled = false)");
        }
        let (store, tracked, _) = super::history::open().await?;
        if let Some(reason) = store.unavailable() {
            bail!("cannot apply: {reason}");
        }
        apply::apply(
            &store,
            &tracked,
            &ApplyRequest {
                paths: self.paths.clone(),
                dry_run: self.dry_run,
                take_remote: self.take_remote.clone(),
                keep_local: self.keep_local.clone(),
                take_remote_all: self.take_remote_all,
                keep_local_all: self.keep_local_all,
                automatic: false,
                plan_only: false,
            },
        )
        .await?;
        Ok(())
    }
}
