use color_eyre::eyre::{Result, eyre};

use crate::config::Config;

/// Show the command for a shell alias
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct ShellAliasGet {
    /// The alias to show
    #[clap(name = "shell_alias")]
    pub alias: String,
}

impl ShellAliasGet {
    pub async fn run(self) -> Result<()> {
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise shell-alias get ll</bold>
    ls -la
"#
);
