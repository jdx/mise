use eyre::Result;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::dirs::SHIMS;
use crate::toolset::{Toolset, ToolsetBuilder};

/// Shows the path that a bin name points to
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Which {
    /// The bin name to look up
    #[clap()]
    pub bin_name: String,

    /// Show the plugin name instead of the path
    #[clap(long, conflicts_with = "version")]
    pub plugin: bool,

    /// Show the version instead of the path
    #[clap(long, conflicts_with = "plugin")]
    pub version: bool,

    /// Use a specific tool@version
    /// e.g.: `mise which npm --tool=node@20`
    #[clap(short, long, value_name = "TOOL@VERSION", verbatim_doc_comment)]
    pub tool: Option<ToolArg>,
}

impl Which {
    pub async fn run(self) -> Result<()> {
        let ts = self.get_toolset().await?;

        match ts.which(&self.bin_name) {
            Some((p, tv)) => {
                if self.version {
                    miseprintln!("{}", tv.version);
                } else if self.plugin {
                    miseprintln!("{p}");
                } else {
                    let path = p.which(&tv, &self.bin_name)?;
                    miseprintln!("{}", path.unwrap().display());
                }
                Ok(())
            }
            None => {
                if self.has_shim(&self.bin_name) {
                    bail!("{} is a mise bin however it is not currently active. Use `mise use` to activate it in this directory.", self.bin_name)
                } else {
                    bail!(
                        "{} is not a mise bin. Perhaps you need to install it first.",
                        self.bin_name
                    )
                }
            }
        }
    }
    async fn get_toolset(&self) -> Result<Toolset> {
        let config = Config::try_get().await?;
        let mut tsb = ToolsetBuilder::new();
        if let Some(tool) = &self.tool {
            tsb = tsb.with_args(&[tool.clone()]);
        }
        let ts = tsb.build(&config)?;
        Ok(ts)
    }
    fn has_shim(&self, shim: &str) -> bool {
        SHIMS.join(shim).exists()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise which node</bold>
    /home/username/.local/share/mise/installs/node/20.0.0/bin/node
    $ <bold>mise which node --plugin</bold>
    node
    $ <bold>mise which node --version</bold>
    20.0.0
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;
    use test_log::test;

    #[test(tokio::test)]
    async fn test_which() {
        reset().await;
        assert_cli!("use", "dummy@1.0.0");
        assert_cli_snapshot!("which", "dummy");
        assert_cli!("use", "dummy@ref:master");
        assert_cli!("uninstall", "dummy@1.0.0");
        assert_cli!("use", "--rm", "dummy");
    }

    #[test(tokio::test)]
    async fn test_which_plugin() {
        reset().await;
        assert_cli!("use", "dummy@1.0.0");
        assert_cli_snapshot!("which", "--plugin", "dummy");
        assert_cli!("use", "dummy@ref:master");
        assert_cli!("uninstall", "dummy@1.0.0");
        assert_cli!("use", "--rm", "dummy");
    }

    #[test(tokio::test)]
    async fn test_which_version() {
        reset().await;
        assert_cli!("use", "dummy@1.0.0");
        assert_cli_snapshot!("which", "--version", "dummy");
        assert_cli!("use", "dummy@ref:master");
        assert_cli!("uninstall", "dummy@1.0.0");
        assert_cli!("use", "--rm", "dummy");
    }

    #[test(tokio::test)]
    async fn test_which_tool() {
        reset().await;
        assert_cli!("install", "dummy@1.0.1");
        assert_cli_snapshot!("which", "dummy", "--tool=dummy@1.0.1");
    }
}
