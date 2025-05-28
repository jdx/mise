use eyre::Result;

use crate::cli::local::local;
use crate::config::Settings;
use crate::{
    cli::args::{BackendArg, ToolArg},
    config::Config,
};

/// Sets/gets the global tool version(s)
///
/// Displays the contents of global config after writing.
/// The file is `$HOME/.config/mise/config.toml` by default. It can be changed with `$MISE_GLOBAL_CONFIG_FILE`.
/// If `$MISE_GLOBAL_CONFIG_FILE` is set to anything that ends in `.toml`, it will be parsed as `mise.toml`.
/// Otherwise, it will be parsed as a `.tool-versions` file.
///
/// Use MISE_ASDF_COMPAT=1 to default the global config to ~/.tool-versions
///
/// Use `mise local` to set a tool version locally in the current directory.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true, after_long_help = AFTER_LONG_HELP)]
pub struct Global {
    /// Tool(s) to add to .tool-versions
    /// e.g.: node@20
    /// If this is a single tool with no version, the current value of the global
    /// .tool-versions will be displayed
    #[clap(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Save exact version to `~/.tool-versions`
    /// e.g.: `mise global --pin node@20` will save `node 20.0.0` to ~/.tool-versions
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,

    /// Save fuzzy version to `~/.tool-versions`
    /// e.g.: `mise global --fuzzy node@20` will save `node 20` to ~/.tool-versions
    /// this is the default behavior unless MISE_ASDF_COMPAT=1
    #[clap(long, verbatim_doc_comment, overrides_with = "pin")]
    fuzzy: bool,

    /// Remove the plugin(s) from ~/.tool-versions
    #[clap(long, value_name = "PLUGIN", aliases = ["rm", "unset"])]
    remove: Option<Vec<BackendArg>>,

    /// Get the path of the global config file
    #[clap(long)]
    path: bool,
}

impl Global {
    pub async fn run(self) -> Result<()> {
        let settings = Settings::try_get()?;
        let config = Config::get().await?;
        local(
            &config,
            &settings.global_tools_file(),
            self.tool,
            self.remove,
            self.pin,
            self.fuzzy,
            self.path,
        )
        .await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
    # set the current version of node to 20.x
    # will use a fuzzy version (e.g.: 20) in .tool-versions file
    $ <bold>mise global --fuzzy node@20</bold>

    # set the current version of node to 20.x
    # will use a precise version (e.g.: 20.0.0) in .tool-versions file
    $ <bold>mise global --pin node@20</bold>

    # show the current version of node in ~/.tool-versions
    $ <bold>mise global node</bold>
    20.0.0
"#
);
