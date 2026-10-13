use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::Result;
use indexmap::IndexMap;

use crate::config::{Config, Settings};
use crate::path::PathExt;
use crate::system;

/// Link, copy, and render `[dotfiles]` entries into place
///
/// Applies the whole-file entries and edits that are not in their desired
/// state. Whole-file entries symlink, copy, render templates, write inline
/// `content`, set `permissions`, or remove targets declared `mode = "absent"`.
/// Edit entries manage a marker-delimited block, a single line, or merged keys
/// in a file mise does not otherwise own.
///
/// Entries that deploy mise's own configuration are applied first and the
/// configuration is read again, so a rule the new file adds applies in the same
/// run.
///
/// `apply` works from the files on this machine. It never fetches or pulls: to
/// bring in changes from other machines, run `mise dot sync`, `mise dot pull`,
/// then `mise dot apply`.
///
/// The `pre-dotfiles` and `post-dotfiles` bootstrap hooks run before and after
/// the apply (with `--dry-run` they are printed, not run), and the matching
/// `[history.reload]` commands run for the targets it wrote.
///
/// With `--prune`, also removes files that dotfile groups deployed but no
/// active entry deploys now: files of a group that is no longer selected or
/// declared, and files a group stopped deploying because of a new `exclude` or
/// a deleted source.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise dot apply", help = "Deploy every entry that is out of date"),
    example("mise dot apply --dry-run", help = "Print what would change"),
    example(
        "mise dot apply --prune",
        help = "Also remove files left by dotfile groups"
    ),
    example(
        "mise dot apply --force --yes",
        help = "Overwrite conflicting files without prompting"
    )
)]
pub(crate) struct DotfilesApply {
    /// Only apply these targets
    #[usage(value_name = "TARGET")]
    targets: Vec<String>,

    /// Overwrite existing files that conflict with whole-file dotfile entries
    #[usage(long, short)]
    force: bool,

    /// Print the actions that would run without writing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,

    /// Also remove files that dotfile groups deployed but no active entry deploys
    /// now
    #[usage(long)]
    prune: bool,

    /// Prompt for `[bootstrap.secrets]` values that templates need and the
    /// environment does not set
    #[usage(long)]
    prompt_secrets: bool,
}

type Requests = (
    Vec<system::files::FileRequest>,
    Vec<system::edits::EditRequest>,
);

impl DotfilesApply {
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// Load and filter the configured whole-file and edit requests.
    pub(crate) fn requests(&self, config: &Config) -> Result<Requests> {
        self.select(config, true)
    }

    /// Like [`Self::requests`], but a target argument that no entry matches
    /// is not an error: the configuration was rewritten after the arguments
    /// matched, so the entry that named them may be gone.
    pub(crate) fn requests_after_config(&self, config: &Config) -> Result<Requests> {
        self.select(config, false)
    }

    fn select(&self, config: &Config, strict: bool) -> Result<Requests> {
        // orphans belong to no configured target, so a prune cannot be
        // narrowed to some; like `unapply --group`, it refuses targets
        if self.prune && !self.targets.is_empty() {
            eyre::bail!(
                "--prune removes orphaned files of every group and cannot be combined with target arguments"
            );
        }
        if self.prune {
            super::ensure_prune_sees_every_group(config)?;
        }
        super::select_requests(config, &self.targets, strict)
    }

    /// Apply only the entries and edits that write mise's own configuration,
    /// so the caller can reload before the rest are planned. Returns the
    /// targets it handled, or `None` when a prompt was declined.
    pub(crate) fn run_config_entries(
        &self,
        config: &Config,
        reload: &IndexMap<String, String>,
        is_config: impl Fn(&Path) -> bool,
    ) -> Result<Option<Vec<String>>> {
        let (mut files, mut edits) = self.requests(config)?;
        files.retain(|req| is_config(&req.target));
        edits.retain(|req| is_config(&req.path));
        // a dry run reads the live file to check an edit, which the copy
        // about to replace it may have changed; the edits the new config
        // keeps are previewed against the simulated one instead
        if self.dry_run {
            edits.clear();
        }
        if files.is_empty() && edits.is_empty() {
            return Ok(Some(vec![]));
        }
        let secrets = system::secrets::resolve(config, self.prompt_secrets)?;
        // what this pass writes, so the second one does not repeat it. A
        // template is not listed: the reloaded config may change what it
        // renders, though a dry run lists it because it never renders one.
        // Nor is an entry already in place, which the second pass still has
        // to record.
        let mut handled = vec![];
        let done = write_and_reload_with(reload, self.dry_run, |written| {
            if !files.is_empty() {
                let opts = self.file_opts();
                let plan = system::files::plan_apply(config, &files, &opts, &secrets)?;
                // an applied config reports nothing: the rest of the apply
                // may still have work, and "all files are applied" would
                // read as the end of the command
                if !plan.is_noop() {
                    handled.extend(
                        plan.planned_requests()
                            .into_iter()
                            .filter(|req| {
                                self.dry_run || req.mode != system::files::FileMode::Template
                            })
                            .map(|req| format!("{req:?}")),
                    );
                    if !system::files::execute_apply(config, plan, &opts, written)? {
                        return Ok(false);
                    }
                }
            }
            if edits.is_empty() {
                return Ok(true);
            }
            system::edits::apply(config, &edits, &self.edit_opts(), written)
        })?;
        Ok(done.then_some(handled))
    }

    /// The apply without an operation of its own, for a caller that already
    /// opened one. `done` describes the entries an earlier pass over mise's own
    /// configuration handled (or, on a dry run, previewed).
    pub(crate) async fn run_inner(
        self,
        config: Arc<Config>,
        reload: &IndexMap<String, String>,
        done: Option<&[String]>,
    ) -> Result<bool> {
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let after_config = done.is_some();
        let (mut files, edits) = if after_config {
            self.requests_after_config(&config)?
        } else {
            self.requests(&config)?
        };
        if let Some(done) = done {
            // a file the first pass wrote is not written again: a config edit
            // that pass made would otherwise be copied back over
            files.retain(|req| !done.contains(&format!("{req:?}")));
        }
        if files.is_empty() && edits.is_empty() && !self.prune {
            // nothing was configured at all, not just nothing left after the
            // config pass
            if done.is_none_or(<[String]>::is_empty) {
                super::warn_if_dotfiles_ignored(&config);
                info!("no dotfiles configured in [dotfiles]");
            }
            return Ok(true);
        }
        write_and_reload_with(reload, self.dry_run, |written| {
            if !self.write(&config, &files, &edits, &secrets, written)? {
                return Ok(false);
            }
            if self.prune {
                self.prune(&config, written)?;
            }
            Ok(true)
        })
    }

    /// Remove what deselected or undeclared groups left behind.
    fn prune(&self, config: &Config, written: &mut Vec<PathBuf>) -> Result<()> {
        let active = system::files::files_from_config(config)?;
        let opts = system::dotfile_groups::RemoveOpts {
            dry_run: self.dry_run,
            force: self.force,
        };
        let orphaned = system::dotfile_groups::orphaned(&active);
        let present = orphaned
            .iter()
            .flat_map(|record| &record.paths)
            .filter(|path| path.target.symlink_metadata().is_ok())
            .map(|path| path.target.display_user())
            .collect::<Vec<_>>();
        if present.is_empty() {
            return Ok(());
        }
        // removal is confirmed like any other apply change
        if !self.dry_run
            && !self.yes
            && console::user_attended_stderr()
            && !crate::ui::prompt::confirm(format!(
                "files: remove orphaned {}?",
                present.join(", ")
            ))?
            .is_yes()
        {
            info!("files: prune skipped");
            return Ok(());
        }
        for record in orphaned {
            written.extend(system::dotfile_groups::remove_recorded(&record, &opts)?);
        }
        Ok(())
    }

    fn file_opts(&self) -> system::files::ApplyOpts {
        system::files::ApplyOpts {
            dry_run: self.dry_run,
            verbose: Settings::get().verbose,
            force: self.force,
            force_hint: "use --force",
            yes: self.yes,
        }
    }

    fn edit_opts(&self) -> system::edits::ApplyOpts {
        system::edits::ApplyOpts {
            part: "dotfiles",
            dry_run: self.dry_run,
            verbose: Settings::get().verbose,
            yes: self.yes,
        }
    }

    /// Apply the whole-file entries, then the edits, appending each written
    /// target to `written` as it goes. Returns `false` when a prompt was
    /// declined.
    fn write(
        &self,
        config: &Config,
        files: &[system::files::FileRequest],
        edits: &[system::edits::EditRequest],
        secrets: &system::secrets::SecretValues,
        written: &mut Vec<PathBuf>,
    ) -> Result<bool> {
        if !files.is_empty() {
            let opts = self.file_opts();
            if !system::files::apply(config, files, &opts, secrets, written)? {
                return Ok(false);
            }
        }
        if !edits.is_empty() {
            let opts = self.edit_opts();
            if !system::edits::apply(config, edits, &opts, written)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Runs `write`, then the `[history.reload]` commands matching the targets it
/// recorded, and returns its result. Shared by `mise dot apply` and the
/// dotfiles phase of `mise bootstrap`.
pub(crate) fn write_and_reload(
    dry_run: bool,
    write: impl FnOnce(&mut Vec<PathBuf>) -> Result<bool>,
) -> Result<bool> {
    // resolved from the trusted layers before anything is written, so
    // nothing this apply writes can change which commands run afterwards
    let reload = system::history::config::reload_commands()?;
    write_and_reload_with(&reload, dry_run, write)
}

/// [`write_and_reload`] with the reload commands resolved by the caller, for
/// an apply that writes in more than one pass.
pub(crate) fn write_and_reload_with(
    reload: &IndexMap<String, String>,
    dry_run: bool,
    write: impl FnOnce(&mut Vec<PathBuf>) -> Result<bool>,
) -> Result<bool> {
    let mut written = vec![];
    let result = write(&mut written);
    // a dry run writes nothing, so nothing is reloaded. A declined edit
    // prompt or a failed later entry still leaves what was written before
    // it, so its applications are reloaded before the error is reported
    if !dry_run && !written.is_empty() {
        let touched = written
            .iter()
            .map(|path| system::history::replay::reload_path(path))
            .collect::<Vec<_>>();
        system::history::replay::run_reload(reload, &touched);
    }
    result
}
