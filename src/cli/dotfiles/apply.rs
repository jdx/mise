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
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
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

    pub(crate) async fn run(self) -> Result<bool> {
        let config = Config::get().await?;
        let (files, edits) = self.requests(&config)?;
        if files.is_empty() && edits.is_empty() {
            super::warn_if_dotfiles_ignored();
            info!("no dotfiles configured in [dotfiles]");
            return Ok(true);
        }
        if !files.is_empty() {
            let opts = system::files::ApplyOpts {
                dry_run: self.dry_run,
                verbose: Settings::get().verbose,
                force: self.force,
                force_hint: "use --force",
                yes: self.yes,
            };
            if !system::files::apply(&config, &files, &opts)? {
                return Ok(false);
            }
        }
        if !edits.is_empty() {
            let opts = system::edits::ApplyOpts {
                dry_run: self.dry_run,
                verbose: Settings::get().verbose,
                yes: self.yes,
            };
            if !system::edits::apply(&config, &edits, &opts)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles apply</bold>
    $ <bold>mise bootstrap dotfiles apply --dry-run</bold>
    $ <bold>mise bootstrap dotfiles apply --force --yes</bold>
"#
);
