use eyre::Result;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::errors::Error;
use crate::toolset::ToolsetBuilder;

/// Display the installation path for a tool
///
/// The tool must be installed for this to work.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct Where {
    /// Tool to look up
    /// e.g.: ruby@3
    /// With "@<PREFIX>", shows the latest installed version matching the prefix.
    /// Otherwise, shows the current, active installed version.
    #[usage(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: ToolArg,

    /// the version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[usage(hide = true, verbatim_doc_comment)]
    asdf_version: Option<String>,
}

impl Where {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let tvr = match self.tool.tvr {
            Some(tvr) => tvr,
            None => match self.asdf_version {
                Some(version) => self.tool.with_version(&version).tvr.unwrap(),
                None => {
                    let ts = ToolsetBuilder::new().build(&config).await?;
                    match ts.versions.get(self.tool.ba.as_ref()) {
                        Some(tvl) => {
                            tvl.os_supported_requests().next().cloned().ok_or_else(|| {
                                eyre::eyre!("{} does not have an active version", self.tool.ba)
                            })?
                        }
                        None => self.tool.with_version("latest").tvr.unwrap(),
                    }
                }
            },
        };

        let tv = tvr.resolve(&config, &Default::default()).await?;

        if tv.backend()?.is_version_installed(&config, &tv, true) {
            miseprintln!("{}", tv.install_path().to_string_lossy());
            Ok(())
        } else {
            Err(Error::VersionNotInstalled(
                Box::new(tv.ba().clone()),
                tv.version,
            ))?
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Show the latest installed node 20.x
    # Errors if no matching version is installed
    $ <bold>mise where node@20</bold>
    /home/jdx/.local/share/mise/installs/node/20.0.0

    # Show the install directory of the active node, or of the latest
    # installed version if no config requests node
    # Errors if no matching version is installed
    $ <bold>mise where node</bold>
    /home/jdx/.local/share/mise/installs/node/20.0.0
"#
);
