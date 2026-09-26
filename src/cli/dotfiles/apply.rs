use std::path::PathBuf;

use eyre::Result;

use crate::config::{Config, Settings};
use crate::system;

/// Apply dotfiles from `[dotfiles]`
///
/// Applies configured whole-file entries and edits that aren't in their
/// desired state. Whole-file entries may symlink, copy, or render templates.
/// Edit entries manage a marker-delimited block or a single line in a file
/// mise doesn't otherwise own.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise dot apply
mise dot apply --dry-run
mise dot apply --force --yes"###
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

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

impl DotfilesApply {
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// Load and filter the configured whole-file and edit requests.
    pub(crate) fn requests(
        &self,
        config: &Config,
    ) -> Result<(
        Vec<system::files::FileRequest>,
        Vec<system::edits::EditRequest>,
    )> {
        super::select_requests(config, &self.targets)
    }

    /// The apply without an operation of its own, for a caller that already
    /// opened one.
    pub(crate) async fn run_inner(self) -> Result<bool> {
        let config = Config::get().await?;
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let (files, edits) = self.requests(&config)?;
        if files.is_empty() && edits.is_empty() {
            super::warn_if_dotfiles_ignored();
            info!("no dotfiles configured in [dotfiles]");
            return Ok(true);
        }
        write_and_reload(self.dry_run, |written| {
            self.write(&config, &files, &edits, &secrets, written)
        })
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
            let opts = system::files::ApplyOpts {
                dry_run: self.dry_run,
                verbose: Settings::get().verbose,
                force: self.force,
                force_hint: "use --force",
                yes: self.yes,
            };
            if !system::files::apply(config, files, &opts, secrets, written)? {
                return Ok(false);
            }
        }
        if !edits.is_empty() {
            let opts = system::edits::ApplyOpts {
                part: "dotfiles",
                dry_run: self.dry_run,
                verbose: Settings::get().verbose,
                yes: self.yes,
            };
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
        system::history::replay::run_reload(&reload, &touched);
    }
    result
}
