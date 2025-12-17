use color_eyre::eyre::{Result, eyre};

use crate::cli::args::BackendArg;
use crate::config::Config;

/// Show an alias for a plugin
///
/// This is the contents of a tool_alias.<PLUGIN> entry in ~/.config/mise/config.toml
///
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct ToolAliasGet {
    /// The plugin to show the alias for
    pub plugin: BackendArg,
    /// The alias to show
    pub alias: String,
}

impl ToolAliasGet {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        match config.all_aliases.get(&self.plugin.short) {
            Some(alias) => match alias.versions.get(&self.alias) {
                Some(alias) => {
                    miseprintln!("{alias}");
                    Ok(())
                }
                None => Err(eyre!("Unknown alias: {}", &self.alias)),
            },
            None => Err(eyre!("Unknown plugin: {}", &self.plugin)),
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise tool-alias get node lts-hydrogen</bold>
    20.0.0
"#
);
