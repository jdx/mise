use color_eyre::eyre::{Result, eyre};

use crate::config::Config;

/// Show the command for a shell alias
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise shell-alias get ll
ls -la"###
    ),
    verbatim_doc_comment
)]
pub(super) struct ShellAliasGet {
    /// The alias to show
    #[usage(name = "shell_alias")]
    pub alias: String,
}

impl ShellAliasGet {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        match config.shell_aliases.get(&self.alias) {
            Some((command, _path)) => {
                miseprintln!("{command}");
                Ok(())
            }
            None => Err(eyre!("Unknown shell alias: {}", &self.alias)),
        }
    }
}
