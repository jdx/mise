use eyre::Result;

use crate::args::BackendArg;
use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Set a backend alias or a version alias in the global config
///
/// `mise tool-alias set TOOL BACKEND` makes TOOL install from BACKEND, written as
/// `[tool_alias] TOOL = "BACKEND"`. `mise tool-alias set TOOL ALIAS VERSION` makes
/// `TOOL@ALIAS` mean VERSION, written under `[tool_alias.TOOL.versions]`. Writes
/// the global config (~/.config/mise/config.toml by default); add aliases to a
/// project's mise.toml to share them.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_aliases = ["add", "create"],
    example(
        r###"mise tool-alias set ripgrep aqua:BurntSushi/ripgrep"###,
        help = "Install ripgrep from its aqua backend"
    ),
    example(
        r###"mise tool-alias set node project 20"###,
        help = "Make node@project mean node@20"
    ),
    verbatim_doc_comment
)]
pub(super) struct ToolAliasSet {
    /// The tool to alias
    #[usage(value_name = "TOOL")]
    pub tool: BackendArg,
    /// The version alias to set, or the backend when VALUE is omitted
    pub alias: String,
    /// The version request the alias stands for
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
