use std::sync::Arc;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::dirs::SHIMS;
use crate::file;
use crate::toolset::{Toolset, ToolsetBuilder};
use eyre::{Result, bail};
use itertools::Itertools;

/// Shows the path that a tool's bin points to.
///
/// Use this to figure out what version of a tool is currently active.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Which {
    /// The bin to look up
    #[clap(required_unless_present = "complete")]
    pub bin_name: Option<String>,

    #[clap(long, hide = true)]
    pub complete: bool,

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
        let config = Config::get().await?;
        if self.complete {
            return self.complete(&config).await;
        }
        let ts = self.get_toolset(&config).await?;

        let bin_name = self.bin_name.clone().unwrap();
        match ts.which(&config, &bin_name).await {
            Some((p, tv)) => {
                if self.version {
                    miseprintln!("{}", tv.version);
                } else if self.plugin {
                    miseprintln!("{p}");
                } else {
                    let path = p.which(&config, &tv, &bin_name).await?;
                    miseprintln!("{}", path.unwrap().display());
                }
                Ok(())
            }
            None => {
                if self.has_shim(&bin_name) {
                    bail!(
                        "{bin_name} is a mise bin however it is not currently active. Use `mise use` to activate it in this directory."
                    )
                } else {
                    bail!("{bin_name} is not a mise bin. Perhaps you need to install it first.",)
                }
            }
        }
    }
    async fn complete(&self, config: &Arc<Config>) -> Result<()> {
        let ts = self.get_toolset(config).await?;
        let bins = ts
            .list_paths(config)
            .await
            .into_iter()
            .flat_map(|p| file::ls(&p).unwrap_or_default())
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .unique()
            .sorted()
            .collect_vec();
        for bin in bins {
            println!("{bin}");
        }
        Ok(())
    }
    async fn get_toolset(&self, config: &Arc<Config>) -> Result<Toolset> {
        let mut tsb = ToolsetBuilder::new();
        if let Some(tool) = &self.tool {
            tsb = tsb.with_args(&[tool.clone()]);
        }
        let ts = tsb.build(config).await?;
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
