use eyre::Result;

use crate::config::Config;
use crate::system;

/// Show what `mise dot apply` would change
///
/// Prints a unified diff for every whole-file entry and edit that
/// `mise dot apply` would change. Templates are rendered first, so trusted
/// template functions may run. Tracked entries have nothing to apply; use
/// `mise dot history diff` to see their unsaved changes.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise dot diff", help = "Show every pending change"),
    example("mise dot diff ~/.zshrc", help = "Show the change for one target")
)]
pub(crate) struct DotfilesDiff {
    /// Only show these targets
    #[usage(value_name = "TARGET")]
    targets: Vec<String>,

    /// Prompt for `[bootstrap.secrets]` values that templates need and the
    /// environment does not set
    #[usage(long)]
    prompt_secrets: bool,
}

impl DotfilesDiff {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let (files, edits) = super::select_requests(&config, &self.targets)?;
        if files.is_empty() && edits.is_empty() {
            super::warn_if_dotfiles_ignored(&config);
            info!("no dotfiles configured in [dotfiles]");
            return Ok(());
        }

        if !files.is_empty() {
            system::files::print_diffs(&config, &files, &secrets)?;
        }
        if !edits.is_empty() {
            system::edits::print_diffs(&config, &edits)?;
        }
        Ok(())
    }
}
