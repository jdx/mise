//! `mise dot history`: the checkpoint browser for the tracked
//! configuration files, and the helpers every history command shares.

use eyre::Result;

pub(crate) use crate::system::history::{open, resolve, short};

mod describe;
mod diff;
mod ls;
pub(crate) mod show;

/// Browse saved versions of your tracked files
///
/// A checkpoint is a saved version of every `[dotfiles]` entry with
/// `mode = "track"`. `mise dot save`, the watcher, and every command that
/// changes tracked files (such as `mise bootstrap`, `mise dot apply`,
/// `rollback`, `pull`, and `undo`) record one. Checkpoints hold file contents
/// only, not packages or services. Files deployed in other modes are saved only
/// if you also track them. With no subcommand, lists checkpoints newest first.
///
/// See https://mise.jdx.dev/dotfiles/history.html
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise dot history", help = "List recent checkpoints"),
    example(
        "mise dot history --path ~/.config/hypr/bindings.lua",
        help = "List checkpoints that changed one file"
    ),
    example(
        "mise dot history --trigger bootstrap -n 5",
        help = "List the last five checkpoints from mise bootstrap or mise dot apply"
    ),
    example("mise dot history show latest", help = "Show the newest checkpoint"),
    example("mise dot history diff", help = "Show unsaved changes")
)]
pub(crate) struct DotfilesHistory {
    #[usage(subcommand)]
    command: Option<HistoryCommands>,

    #[usage(flatten)]
    ls: ls::HistoryLs,
}

#[derive(Debug, usage_rs::Subcommands)]
enum HistoryCommands {
    Describe(describe::HistoryDescribe),
    Diff(diff::HistoryDiff),
    Ls(ls::HistoryLs),
    Show(show::HistoryShow),
}

impl DotfilesHistory {
    pub(crate) async fn run(self) -> Result<()> {
        match self.command {
            Some(HistoryCommands::Describe(cmd)) => cmd.run().await,
            Some(HistoryCommands::Diff(cmd)) => cmd.run().await,
            Some(HistoryCommands::Ls(cmd)) => cmd.run().await,
            Some(HistoryCommands::Show(cmd)) => cmd.run().await,
            None => self.ls.run().await,
        }
    }
}

/// The display form of a path argument (`~/…` under `$HOME`).
pub(crate) fn display_arg(path: &str) -> String {
    // Use the checkpoint's portable root mapping. Normalizing the target and
    // abbreviating against the unnormalized HOME loses root aliases. The tree
    // conversion also preserves a symlink leaf rather than following it.
    use crate::system::history::tracked::{display_to_tree_path, tree_path_to_display};
    tree_path_to_display(&display_to_tree_path(path))
}

pub(crate) fn local_time(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| rfc3339.to_string())
}
