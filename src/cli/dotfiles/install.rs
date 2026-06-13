use eyre::Result;

use crate::config::{Config, Settings};
use crate::system;

/// Apply dotfiles from `[dotfiles]`
///
/// Applies configured whole-file entries and edits that aren't in their
/// desired state. Whole-file entries may symlink, copy, or render templates.
/// Edit entries manage a marker-delimited block or a single line in a file
/// mise doesn't otherwise own.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct DotfilesInstall {
    /// Overwrite existing files that conflict with whole-file dotfile entries
    #[clap(long, short)]
    force: bool,

    /// Print the actions that would run without writing anything
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

impl DotfilesInstall {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise dotfiles")?;
        let config = Config::get().await?;
        let files = system::files::files_from_config(&config);
        let edits = system::edits::edits_from_config(&config);
        if files.is_empty() && edits.is_empty() {
            info!("no dotfiles configured in [dotfiles]");
            return Ok(());
        }
        if !files.is_empty() {
            let opts = system::files::ApplyOpts {
                dry_run: self.dry_run,
                force: self.force,
                yes: self.yes,
            };
            system::files::apply(&config, &files, &opts)?;
        }
        if !edits.is_empty() {
            let opts = system::edits::ApplyOpts {
                dry_run: self.dry_run,
                yes: self.yes,
            };
            system::edits::apply(&config, &edits, &opts)?;
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise dotfiles install</bold>
    $ <bold>mise dotfiles install --dry-run</bold>
    $ <bold>mise dotfiles install --force --yes</bold>
"#
);
