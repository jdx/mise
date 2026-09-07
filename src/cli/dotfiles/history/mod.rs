//! `mise bootstrap dotfiles history`: the checkpoint browser for the tracked
//! configuration files, and the helpers every history command shares.

use eyre::Result;

use crate::system::history::checkpoint::Store;
use crate::system::history::store::{self, Entry};
use crate::system::history::tracked::TrackedSet;

mod describe;
mod diff;
mod ls;
pub(crate) mod show;

/// Browse the checkpoints of your dotfiles (experimental)
///
/// Every save, every mutating bootstrap command, and the watcher record a
/// checkpoint of the tracked files: the global mise config directory, the
/// dotfiles root, and every `[dotfiles]` entry. A checkpoint holds files,
/// never package or service state: restoring one restores files. Without a
/// subcommand this lists them, newest first.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
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
        crate::system::history::ensure_experimental()?;
        match self.command {
            Some(HistoryCommands::Describe(cmd)) => cmd.run().await,
            Some(HistoryCommands::Diff(cmd)) => cmd.run().await,
            Some(HistoryCommands::Ls(cmd)) => cmd.run().await,
            Some(HistoryCommands::Show(cmd)) => cmd.run().await,
            None => self.ls.run().await,
        }
    }
}

/// Opens the store and lists its checkpoints, oldest first. Reading never
/// changes the store: an operation that died is closed by the next one
/// that takes the operation lock (a save, a bootstrap, the watcher).
pub(crate) async fn open() -> Result<(Store, TrackedSet, Vec<Entry>)> {
    let store = Store::open()?;
    let tracked = TrackedSet::effective().await?;
    let entries = store.list()?;
    Ok((store, tracked, entries))
}

/// Resolves a checkpoint reference. With a path scope, `latest[~N]` counts
/// only the checkpoints where that path changed.
pub(crate) fn resolve(spec: &str, entries: &[Entry], path: Option<&str>) -> Result<Entry> {
    let scoped: Vec<Entry> = match path {
        Some(path) => entries
            .iter()
            .filter(|entry| entry.checkpoint.changes.touches(path))
            .cloned()
            .collect(),
        None => entries.to_vec(),
    };
    let id = if spec.starts_with("latest") {
        store::resolve_ref(spec, &scoped)?
    } else {
        store::resolve_ref(spec, entries)?
    };
    entries
        .iter()
        .find(|entry| entry.id == id)
        .cloned()
        .ok_or_else(|| eyre::eyre!("no history checkpoint {id}"))
}

/// The display form of a path argument (`~/…` under `$HOME`).
pub(crate) fn display_arg(path: &str) -> String {
    // the link itself, never its destination
    crate::file::display_path(crate::system::history::tracked::normalize_target(
        std::path::Path::new(path),
    ))
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

pub(crate) fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles history</bold>
    $ <bold>mise bootstrap dotfiles history --path ~/.config/hypr/bindings.lua</bold>
    $ <bold>mise bootstrap dotfiles history show latest</bold>
    $ <bold>mise bootstrap dotfiles history diff</bold>          # the working tree against the latest checkpoint
    $ <bold>mise bootstrap dotfiles history diff 11 12 --patch</bold>
    $ <bold>mise bootstrap dotfiles save --description "before the theme change"</bold>
    $ <bold>mise bootstrap dotfiles rollback ~/.config/hypr/bindings.lua</bold>
    $ <bold>mise bootstrap dotfiles undo</bold>
"#
);
