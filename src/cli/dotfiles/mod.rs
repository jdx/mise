use eyre::Result;
use futures_util::future::LocalBoxFuture;
use std::path::Path;

mod add;
mod apply;
mod capture;
pub(crate) mod capture_health;
mod conflicts;
mod diff;
mod edit;
mod exclude;
pub(crate) mod history;
mod history_status;
mod origin;
mod paths;
mod pull;
mod recover;
mod rollback;
mod save;
mod status;
mod sync;
pub(crate) mod track;
mod unapply;
mod undo;
mod untrack;
mod watch;

pub(crate) use apply::{DotfilesApply, write_and_reload};

/// Load, validate, and filter whole-file and edit requests with the same
/// target semantics for every command that acts on both kinds of entry.
fn select_requests(
    config: &crate::config::Config,
    targets: &[String],
) -> Result<(
    Vec<crate::system::files::FileRequest>,
    Vec<crate::system::edits::EditRequest>,
)> {
    let all_files = crate::system::files::files_from_config(config)?;
    crate::system::files::validate_composed_file_footprints(&all_files)?;
    let files = all_files
        .iter()
        .filter(|req| crate::system::files::matches_target(&req.target, &req.target_raw, targets))
        .cloned()
        .collect::<Vec<_>>();
    let all_edits = crate::system::edits::edits_from_config(config)?;
    let edits = all_edits
        .iter()
        .filter(|req| crate::system::edits::matches_target(req, targets))
        .cloned()
        .collect::<Vec<_>>();
    if files.is_empty()
        && edits.is_empty()
        && !targets.is_empty()
        && (!all_files.is_empty() || !all_edits.is_empty())
    {
        eyre::bail!("no dotfiles matched target filter: {}", targets.join(", "));
    }
    Ok((files, edits))
}

/// Manage dotfiles from `[dotfiles]`
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct Dotfiles {
    #[usage(subcommand)]
    command: Commands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Add(add::DotfilesAdd),
    Apply(apply::DotfilesApply),
    Capture(capture::DotfilesCapture),
    Conflicts(conflicts::DotfilesConflicts),
    Diff(diff::DotfilesDiff),
    Edit(edit::DotfilesEdit),
    Exclude(exclude::DotfilesExclude),
    History(history::DotfilesHistory),
    Include(exclude::DotfilesInclude),
    Origin(origin::DotfilesOrigin),
    Paths(paths::DotfilesPaths),
    Pull(pull::DotfilesPull),
    Recover(recover::DotfilesRecover),
    Rollback(rollback::DotfilesRollback),
    Save(save::DotfilesSave),
    Status(status::DotfilesStatus),
    Sync(sync::DotfilesSync),
    Track(track::DotfilesTrack),
    Unapply(unapply::DotfilesUnapply),
    Undo(undo::DotfilesUndo),
    Untrack(untrack::DotfilesUntrack),
    Watch(watch::DotfilesWatch),
}

impl Dotfiles {
    /// The watcher, which runs as a service and has no terminal reading it.
    pub(crate) fn is_watch(&self) -> bool {
        matches!(self.command, Commands::Watch(_))
    }

    pub(crate) async fn run(self) -> Result<()> {
        // Anything a background save had to say is said here, to the
        // person who is now present, before the command they asked for
        // runs. The watcher itself is where those notices come from, so
        // it is not where they are delivered.
        let deliver = !matches!(self.command, Commands::Watch(_));
        if deliver {
            crate::system::history::notices::drain();
        }
        let outcome = self.dispatch().await;
        // **And again afterwards.** A command that captures — `mise dot
        // sync` applying incoming changes, say — can write a notice
        // while it runs, and making the user wait for their next command
        // to hear about their own is not delivering it.
        if deliver {
            crate::system::history::notices::drain();
        }
        outcome
    }

    /// Boxed rather than `async` to keep debug builds' main stack small;
    /// see `cli::Commands::run`.
    fn dispatch(self) -> LocalBoxFuture<'static, Result<()>> {
        match self.command {
            Commands::Add(cmd) => Box::pin(cmd.run()),
            Commands::Apply(cmd) => Box::pin(crate::cli::bootstrap::run_dotfiles_apply(cmd)),
            Commands::Capture(cmd) => Box::pin(cmd.run()),
            Commands::Conflicts(cmd) => Box::pin(cmd.run()),
            Commands::Diff(cmd) => Box::pin(cmd.run()),
            Commands::Edit(cmd) => Box::pin(cmd.run()),
            Commands::Exclude(cmd) => Box::pin(cmd.run()),
            Commands::History(cmd) => Box::pin(cmd.run()),
            Commands::Include(cmd) => Box::pin(cmd.run()),
            Commands::Origin(cmd) => Box::pin(cmd.run()),
            Commands::Paths(cmd) => Box::pin(cmd.run()),
            Commands::Pull(cmd) => Box::pin(cmd.run()),
            Commands::Recover(cmd) => Box::pin(cmd.run()),
            Commands::Rollback(cmd) => Box::pin(cmd.run()),
            Commands::Save(cmd) => Box::pin(cmd.run()),
            Commands::Status(cmd) => Box::pin(cmd.run()),
            Commands::Sync(cmd) => Box::pin(cmd.run()),
            Commands::Track(cmd) => Box::pin(cmd.run()),
            Commands::Unapply(cmd) => Box::pin(cmd.run()),
            Commands::Undo(cmd) => Box::pin(cmd.run()),
            Commands::Untrack(cmd) => Box::pin(cmd.run()),
            Commands::Watch(cmd) => Box::pin(cmd.run()),
        }
    }
}

/// Config files mise skipped because they're untrusted (declining the trust
/// prompt adds them to the ignore list) but that do declare `[dotfiles]`.
/// Their entries never reach these commands, so "nothing configured" reads as
/// a config mistake when the real answer is that the file wasn't loaded.
///
/// Reading and parsing the TOML here is inert — nothing is templated or
/// executed, we only look for the table's presence.
pub(crate) fn ignored_configs_with_dotfiles() -> Vec<&'static Path> {
    crate::config::IGNORED_CONFIG_FILES
        .iter()
        .filter(|path| {
            crate::file::read_to_string(path)
                .ok()
                .and_then(|body| body.parse::<toml::Table>().ok())
                .is_some_and(|table| table.contains_key("dotfiles"))
        })
        .map(|path| path.as_path())
        .collect()
}

/// Explain the empty `[dotfiles]` when it's really an untrusted config.
pub(crate) fn warn_if_dotfiles_ignored() {
    let ignored = ignored_configs_with_dotfiles();
    if ignored.is_empty() {
        return;
    }
    warn!(
        "[dotfiles] in these config files was skipped because they are not trusted:\n{}\nRun `mise trust` in that directory to use them.",
        ignored
            .iter()
            .map(|p| format!("  {}", crate::file::display_path(p)))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
