use eyre::Result;

use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Add/update an alias for a tool/backend
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, usage_rs::Args)]
#[usage(visible_aliases = ["add", "create"], example(r###"mise tool-alias set ripgrep aqua:BurntSushi/ripgrep
mise tool-alias set node project 20"###), verbatim_doc_comment)]
pub(super) struct ToolAliasSet {
    /// The tool/backend to set the alias for
    #[usage(value_name = "TOOL")]
    pub tool: BackendArg,
    /// The alias to set
    pub alias: String,
    /// The value to set the alias to
    pub value: Option<String>,
}

impl ToolAliasSet {
    pub(super) async fn run(self) -> Result<()> {
        let mut global_config = Config::get().await?.global_config()?;
        match &self.value {
            None => global_config.set_backend_alias(&self.tool, &self.alias)?,
            Some(val) => global_config.set_alias(&self.tool, &self.alias, val)?,
        }
        global_config.save()
    }
}
