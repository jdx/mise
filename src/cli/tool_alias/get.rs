use color_eyre::eyre::{Result, eyre};

use crate::cli::args::BackendArg;
use crate::config::Config;

/// Show an alias for a tool
///
/// This is the contents of a tool_alias.<TOOL> entry in ~/.config/mise/config.toml
///
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct ToolAliasGet {
    /// The tool to show the alias for
    #[clap(value_name = "TOOL")]
    pub tool: BackendArg,
    /// The alias to show
    pub alias: String,
}

impl ToolAliasGet {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        match config.all_aliases.get(&self.tool.short) {
            Some(alias) => match alias.versions.get(&self.alias) {
                Some(alias) => {
                    miseprintln!("{alias}");
                    Ok(())
                }
                None => Err(eyre!("Unknown alias: {}", &self.alias)),
            },
            None => Err(eyre!("Unknown tool: {}", &self.tool)),
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise tool-alias get node lts-hydrogen</bold>
    20.0.0
"#
);
