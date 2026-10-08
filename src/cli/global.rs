use eyre::Result;

use crate::cli::local::local;
use crate::config::{Settings, SettingsExt};
use crate::{
    cli::args::{BackendArg, ToolArg},
    config::Config,
};

/// Set or show the global tool versions (deprecated; use `mise use -g`)
///
/// With no arguments, prints the global config. With tools, writes them and
/// prints the file and the tools it changed. The file is
/// `~/.config/mise/config.toml` unless `MISE_GLOBAL_CONFIG_FILE` names another. A
/// path ending in `.toml` is parsed as `mise.toml`; any other path is parsed as a
/// `.tool-versions` file.
///
/// Set MISE_ASDF_COMPAT=1 to make ~/.tool-versions the default global config.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    hide = true,
    example(
        r###"mise global --fuzzy node@24"###,
        help = r###"Save node 24 as written, such as `node = "24"`, to the global config"###
    ),
    example(
        r###"mise global --pin node@24"###,
        help = r###"Save the exact version, such as `node = "24.11.0"`"###
    ),
    example(
        r###"mise global node
24.11.0"###,
        help = r###"Show the global node version"###
    )
)]
pub(crate) struct Global {
    /// Tools to add to the global config, such as `node@24`
    ///
    /// With a single tool and no version, prints that tool's global version.
    #[usage(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Save the version as written, such as `24`
    ///
    /// This is the default unless MISE_ASDF_COMPAT=1.
    #[usage(long, verbatim_doc_comment, overrides = "pin")]
    fuzzy: bool,

    /// Print the path of the global config file
    #[usage(long)]
    path: bool,

    /// Save the exact version, such as `24.11.0`
    #[usage(long, verbatim_doc_comment, overrides = "fuzzy")]
    pin: bool,

    /// Remove these tools from the global config
    #[usage(long, value_name = "TOOL", aliases = ["rm", "unset"])]
    remove: Option<Vec<BackendArg>>,
}

impl Global {
    pub(crate) async fn run(self) -> Result<()> {
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
