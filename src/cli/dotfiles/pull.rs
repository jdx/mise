use std::path::PathBuf;

use eyre::{Result, bail};

use crate::system::history::sync::apply::{self, ApplyRequest};

/// Pull incoming shared changes into the live files (experimental)
///
/// Writes the changes the last `mise bootstrap dotfiles sync` recorded as pending
/// (`apply` keeps deploying your own `[dotfiles]` declarations; `pull` writes
/// what other machines shared),
/// as one recoverable transaction: a protective checkpoint first, every
/// file written and journaled one at a time, reload hooks only afterwards,
/// and `mise bootstrap dotfiles undo` to reverse it. Configuration and the sources it
/// references apply together; an incoming configuration file that does not
/// parse, a path with unsaved local edits, staged git changes in your own
/// checkout, or a genuine local edit pauses the complete application.
/// Conflicts pause both publication and application for the whole setup;
/// local history and fetching continue. Decisions are recorded per path
/// with `--take-remote` or `--keep-local`; sharing resumes only after all
/// conflicts are resolved and the plan has been recomputed.
///
/// In `sync` mode the watcher pulls conflict-free setups on its
/// own; this command writes what is pending right now and decides
/// conflicts. When an incoming configuration declares more tracked files,
/// their shared versions follow in the same run.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct DotfilesPull {
    /// Paths must cover the complete pending setup; partial pulls are rejected
    #[usage(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Show the plan without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Pull without prompting
    #[usage(long, short = 'y')]
    yes: bool,

    /// Resolve a conflict with the repository's version
    #[usage(long, value_name = "PATH")]
    take_remote: Vec<PathBuf>,

    /// Resolve a conflict by keeping this machine's version (published next)
    #[usage(long, value_name = "PATH")]
    keep_local: Vec<PathBuf>,
}

impl DotfilesPull {
    pub(crate) async fn run(self) -> Result<()> {
        crate::config::Settings::get().ensure_experimental("dotfile tracking")?;
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
                yes: self.yes,
                take_remote: self.take_remote.clone(),
                keep_local: self.keep_local.clone(),
                automatic: false,
                plan_only: false,
            },
        )
        .await?;
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles pull --dry-run</bold>
    $ <bold>mise bootstrap dotfiles pull --yes</bold>
    $ <bold>mise bootstrap dotfiles pull ~/.config/hypr</bold>
    $ <bold>mise bootstrap dotfiles pull --take-remote ~/.zshrc</bold>
    $ <bold>mise bootstrap dotfiles pull --keep-local ~/.zshrc</bold>
"#
);
