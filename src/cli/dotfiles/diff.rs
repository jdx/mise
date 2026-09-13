use eyre::Result;

use crate::config::Config;
use crate::system;

/// Show the changes needed to apply dotfiles from `[dotfiles]`
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise bootstrap dotfiles diff
mise bootstrap dotfiles diff ~/.zshrc"###
    )
)]
pub(crate) struct DotfilesDiff {
    /// Only show these targets
    #[usage(value_name = "TARGET")]
    targets: Vec<String>,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

impl DotfilesDiff {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let (files, edits) = super::select_requests(&config, &self.targets)?;
        if files.is_empty() && edits.is_empty() {
            super::warn_if_dotfiles_ignored();
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
