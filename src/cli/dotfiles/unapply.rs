use eyre::Result;

use crate::config::{Config, Settings};
use crate::system;

/// Remove dotfiles applied from `[dotfiles]`
///
/// Removes configured whole-file entries and edits while preserving files
/// mise cannot identify as managed. Modified copies, templates, and plain-line
/// edits require `--force`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct DotfilesUnapply {
    /// Only unapply these targets
    #[clap(value_name = "TARGET")]
    targets: Vec<String>,

    /// Remove modified or otherwise ambiguous managed files and lines
    #[clap(long, short)]
    force: bool,

    /// Print the actions that would run without writing anything
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

impl DotfilesUnapply {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let all_files = system::files::files_from_config(&config);
        let files = all_files
            .iter()
            .filter(|req| {
                system::files::matches_target(&req.target, &req.target_raw, &self.targets)
            })
            .cloned()
            .collect::<Vec<_>>();
        let all_edits = system::edits::edits_from_config(&config);
        let edits = all_edits
            .iter()
            .filter(|req| system::edits::matches_target(req, &self.targets))
            .cloned()
            .collect::<Vec<_>>();
        if files.is_empty()
            && edits.is_empty()
            && !self.targets.is_empty()
            && (!all_files.is_empty() || !all_edits.is_empty())
        {
            eyre::bail!(
                "no dotfiles matched target filter: {}",
                self.targets.join(", ")
            );
        }
        if files.is_empty() && edits.is_empty() {
            super::warn_if_dotfiles_ignored();
            info!("no dotfiles configured in [dotfiles]");
            return Ok(());
        }

        // Apply writes whole files before edits, so undo them in reverse.
        if !edits.is_empty() {
            let opts = system::edits::UnapplyOpts {
                dry_run: self.dry_run,
                verbose: Settings::get().verbose,
                force: self.force,
                yes: self.yes,
            };
            system::edits::unapply(&edits, &opts)?;
        }
        if !files.is_empty() {
            let opts = system::files::UnapplyOpts {
                dry_run: self.dry_run,
                verbose: Settings::get().verbose,
                force: self.force,
                yes: self.yes,
            };
            system::files::unapply(&config, &files, &opts)?;
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles unapply</bold>
    $ <bold>mise bootstrap dotfiles unapply ~/.zshrc</bold>
    $ <bold>mise bootstrap dotfiles unapply --dry-run</bold>
    $ <bold>mise bootstrap dotfiles unapply --force --yes</bold>
"#
);
