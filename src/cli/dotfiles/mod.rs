use eyre::Result;
use futures_util::future::LocalBoxFuture;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::{Config, Settings, SettingsExt, safe_mode_ignores_bootstrap};

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
mod notify;
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

/// Deploy, track, and sync dotfiles
///
/// A `[dotfiles]` entry deploys a file (from a source with `symlink`,
/// `symlink-each`, `copy`, or `template`, or from inline `content`), removes
/// one (`mode = "absent"`), sets an existing path's `permissions`, edits part
/// of a file, or tracks a live file where it is (`mode = "track"`). mise saves
/// tracked files to a local Git history that you can browse and roll back, and
/// syncs it with other machines through an origin repository. `mise dot` is a
/// shorter alias.
///
/// Deploy files with `add`, `apply`, `diff`, `status`, and `unapply`. Track and
/// restore files with `track`, `save`, `history`, `rollback`, and `undo`. Sync
/// them with `origin`, `sync`, and `pull`.
///
/// See https://mise.jdx.dev/dotfiles.html and
/// https://mise.jdx.dev/dotfiles/history.html
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        "mise dot add ~/.zshrc",
        help = "Start managing ~/.zshrc from your dotfiles directory"
    ),
    example("mise dot apply", help = "Deploy every [dotfiles] entry"),
    example(
        "mise dot status",
        help = "Show the state of your dotfiles and their history"
    ),
    example(
        "mise dot track ~/.config/nvim",
        help = "Save the history of a directory in place"
    ),
    example("mise dot history", help = "List saved checkpoints")
)]
pub(crate) struct Dotfiles {
    /// Work on this machine's local-only history (`mode = "track-local"`), which is never shared
    #[usage(long)]
    local: bool,

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
    Notify(notify::DotfilesNotify),
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
        let local_scope = crate::system::history::local::active();
        if self.local || local_scope {
            self.check_local()?;
        }
        if self.local && !local_scope {
            return run_local_argv(true);
        }
        // Anything a background save had to say is said here, to the
        // person who is now present, before the command they asked for
        // runs. The watcher itself is where those notices come from, so
        // it is not where they are delivered.
        let deliver = !matches!(self.command, Commands::Watch(_));
        // Before any result-based return, so the warning shows even when
        // global entries remain.
        if Settings::safe_mode()
            && let Ok(config) = Config::get().await
        {
            warn_if_ignored_in_safe_mode(&config);
        }
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

    /// Local-only history has checkpoints to browse, save, and restore, and
    /// nothing to share or deploy.
    fn check_local(&self) -> Result<()> {
        let name = match &self.command {
            Commands::Capture(_)
            | Commands::History(_)
            | Commands::Paths(_)
            | Commands::Recover(_)
            | Commands::Rollback(_)
            | Commands::Save(_)
            | Commands::Status(_)
            | Commands::Undo(_)
            | Commands::Watch(_) => return Ok(()),
            Commands::Conflicts(_) => "conflicts",
            Commands::Origin(_) => "origin",
            Commands::Pull(_) => "pull",
            Commands::Sync(_) => "sync",
            _ => {
                eyre::bail!(
                    "--local selects this machine's local-only history; declare entries with `mise dot track --local <path>`"
                )
            }
        };
        eyre::bail!("`mise dot {name}` does not apply to local-only history, which is never shared")
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
            Commands::Notify(cmd) => Box::pin(cmd.run()),
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

/// For a command naming paths: when every one of them is kept in
/// local-only history, runs the command there and returns `true`. Paths of
/// both histories cannot share one command, whose checkpoint references
/// would mean different things in each.
pub(crate) async fn route_local(paths: &[std::path::PathBuf]) -> Result<bool> {
    if paths.is_empty() || crate::system::history::local::active() {
        return Ok(false);
    }
    let config = crate::config::Config::get().await?;
    let local = crate::system::history::tracked::TrackedSet::from_config(&config)?.local;
    if local.is_empty() {
        return Ok(false);
    }
    let is_local = |path: &std::path::PathBuf| {
        let path = crate::system::history::tracked::normalize_target(path);
        local.iter().any(|local| path.starts_with(local))
    };
    match paths.iter().filter(|path| is_local(path)).count() {
        0 => Ok(false),
        count if count == paths.len() => run_local_argv(false).map(|()| true),
        _ => eyre::bail!(
            "these paths are in two histories; name local-only paths in a separate `mise dot --local` command"
        ),
    }
}

/// Runs this command line again in the local scope, without `--local`.
fn run_local_argv(remove_local: bool) -> Result<()> {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if remove_local && let Some(index) = args.iter().position(|arg| arg == "--local") {
        args.remove(index);
    }
    let status = crate::system::history::local::command(&args)?.status()?;
    if !status.success() {
        return Err(crate::request_exit(status.code().unwrap_or(1)));
    }
    Ok(())
}

/// Config files mise skipped because they're untrusted (declining the trust
/// prompt adds them to the ignore list) but that do declare `[dotfiles]`.
/// Their entries never reach these commands, so "nothing configured" reads as
/// a config mistake when the real answer is that the file wasn't loaded.
pub(crate) fn ignored_configs_with_dotfiles() -> Vec<&'static Path> {
    crate::config::IGNORED_CONFIG_FILES
        .iter()
        .filter(|path| declares_dotfiles(path))
        .map(|path| path.as_path())
        .collect()
}

/// Whether the config file at `path` declares `[dotfiles]` or `[dotfile_groups]`.
fn declares_dotfiles(path: &Path) -> bool {
    declares_any_table(path, &["dotfiles", "dotfile_groups"])
}

/// Whether the config file at `path` declares any of the top-level `tables`.
fn declares_any_table(path: &Path, tables: &[&str]) -> bool {
    parse_table(path).is_some_and(|table| tables.iter().any(|name| table.contains_key(*name)))
}

/// Whether the config file at `path` declares something that decides which
/// dotfiles apply: `[dotfiles]`, `[dotfile_groups]`, or the `[bootstrap]`
/// keys that select groups (`dotfile_groups`) or the roots that declare them
/// (`config_roots`). Other `[bootstrap]` tables, such as `repos`, do not.
fn declares_dotfile_selection(path: &Path) -> bool {
    parse_table(path).is_some_and(|table| {
        table.contains_key("dotfiles")
            || table.contains_key("dotfile_groups")
            || table
                .get("bootstrap")
                .and_then(|bootstrap| bootstrap.as_table())
                .is_some_and(|bootstrap| {
                    bootstrap.contains_key("dotfile_groups")
                        || bootstrap.contains_key("config_roots")
                })
    })
}

/// The config file at `path` as a TOML table.
///
/// Reading and parsing the TOML here is inert — nothing is templated or
/// executed, callers only look for a key's presence.
fn parse_table(path: &Path) -> Option<toml::Table> {
    crate::file::read_to_string(path)
        .ok()
        .and_then(|body| body.parse::<toml::Table>().ok())
}

fn path_list(paths: &[&Path]) -> String {
    paths
        .iter()
        .map(|p| format!("  {}", crate::file::display_path(p)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Explain the empty `[dotfiles]` when it's really an untrusted config, or a
/// project config whose `[dotfiles]` safe mode ignores.
pub(crate) fn warn_if_dotfiles_ignored(config: &Config) {
    let ignored = ignored_configs_with_dotfiles();
    if !ignored.is_empty() {
        warn!(
            "[dotfiles] in these config files was skipped because they are not trusted:\n{}\nRun `mise trust` in that directory to use them.",
            path_list(&ignored)
        );
    }
    warn_if_ignored_in_safe_mode(config);
}

/// The project config files whose `[bootstrap]`, `[dotfiles]` or
/// `[dotfile_groups]` safe mode drops. Empty outside safe mode.
fn ignored_in_safe_mode(config: &Config) -> Vec<&Path> {
    ignored_in_safe_mode_where(config, |path| {
        declares_any_table(path, &["bootstrap", "dotfiles", "dotfile_groups"])
    })
}

/// The project config files safe mode ignores that `declares` accepts.
/// Empty outside safe mode.
fn ignored_in_safe_mode_where(config: &Config, declares: fn(&Path) -> bool) -> Vec<&Path> {
    if !Settings::safe_mode() {
        return vec![];
    }
    config
        .config_files
        .keys()
        .filter(|path| safe_mode_ignores_bootstrap(path) && declares(path))
        .map(|path| path.as_path())
        .collect()
}

/// Refuse `--prune` while safe mode hides project config that decides which
/// dotfiles apply: files their groups deployed would look orphaned and be
/// removed. A project that only declares `[bootstrap.repos]` and the like
/// hides no dotfiles, so it does not block a prune.
pub(crate) fn ensure_prune_sees_every_group(config: &Config) -> Result<()> {
    let ignored = ignored_in_safe_mode_where(config, declares_dotfile_selection);
    if !ignored.is_empty() {
        eyre::bail!(
            "--prune is unavailable in safe mode (MISE_SAFE=1) while these project config files declare dotfiles it ignores, since their files would look orphaned:\n{}",
            path_list(&ignored)
        );
    }
    Ok(())
}

/// Name the project config files whose `[bootstrap]`, `[dotfiles]` and
/// `[dotfile_groups]` safe mode drops, so a run that applies less than the
/// files declare says why. Printed at most once per process.
pub(crate) fn warn_if_ignored_in_safe_mode(config: &Config) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if WARNED.load(Ordering::Relaxed) {
        return;
    }
    let ignored = ignored_in_safe_mode(config);
    if !ignored.is_empty() && !WARNED.swap(true, Ordering::Relaxed) {
        warn!(
            "[bootstrap], [dotfiles] and [dotfile_groups] in these config files were skipped because safe mode (MISE_SAFE=1) ignores project config:\n{}",
            path_list(&ignored)
        );
    }
}
