use eyre::Result;

use crate::config::Config;
use crate::system;

/// Show the changes needed to apply dotfiles from `[dotfiles]`
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct DotfilesDiff {
    /// Only show these targets
    #[usage(value_name = "TARGET")]
    targets: Vec<String>,
}

impl DotfilesDiff {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let (files, edits) = super::select_requests(&config, &self.targets)?;
        if files.is_empty() && edits.is_empty() {
            super::warn_if_dotfiles_ignored();
            info!("no dotfiles configured in [dotfiles]");
            return Ok(());
        }

        if !files.is_empty() {
            system::files::print_diffs(&config, &files)?;
        }
        if !edits.is_empty() {
            system::edits::print_diffs(&config, &edits)?;
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles diff</bold>
    $ <bold>mise bootstrap dotfiles diff ~/.zshrc</bold>
"#
);
