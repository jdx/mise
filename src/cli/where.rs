use eyre::Result;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::errors::Error::VersionNotInstalled;
use crate::toolset::ToolsetBuilder;

/// Display the installation path for a tool
///
/// The tool must be installed for this to work.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Where {
    /// Tool(s) to look up
    /// e.g.: ruby@3
    /// if "@<PREFIX>" is specified, it will show the latest installed version
    /// that matches the prefix
    /// otherwise, it will show the current, active installed version
    #[clap(required = true, value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: ToolArg,

    /// the version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[clap(hide = true, verbatim_doc_comment)]
    asdf_version: Option<String>,
}

impl Where {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let tvr = match self.tool.tvr {
            Some(tvr) => tvr,
            None => match self.asdf_version {
                Some(version) => self.tool.with_version(&version).tvr.unwrap(),
                None => {
                    let ts = ToolsetBuilder::new().build(&config).await?;
                    ts.versions
                        .get(self.tool.ba.as_ref())
                        .and_then(|tvr| tvr.requests.first().cloned())
                        .unwrap_or_else(|| self.tool.with_version("latest").tvr.unwrap())
                }
            },
        };

        let tv = tvr.resolve(&config, &Default::default()).await?;

        if tv.backend()?.is_version_installed(&config, &tv, true) {
            miseprintln!("{}", tv.install_path().to_string_lossy());
            Ok(())
        } else {
            Err(VersionNotInstalled(tv.ba().clone(), tv.version))?
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Show the latest installed version of node
    # If it is is not installed, errors
    $ <bold>mise where node@20</bold>
    /home/jdx/.local/share/mise/installs/node/20.0.0

    # Show the current, active install directory of node
    # Errors if node is not referenced in any .tool-version file
    $ <bold>mise where node</bold>
    /home/jdx/.local/share/mise/installs/node/20.0.0
"#
);
