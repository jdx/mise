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

    /// Use a specific tool@version
    /// e.g.: `mise which npm --tool=node@20`
    #[clap(short, long, value_name = "TOOL@VERSION", verbatim_doc_comment)]
    pub tool: Option<ToolArg>,

    #[clap(long, hide = true)]
    pub complete: bool,

    /// Show the plugin name instead of the path
    #[clap(long, conflicts_with = "version")]
    pub plugin: bool,

    /// Show the version instead of the path
    #[clap(long, conflicts_with = "plugin")]
    pub version: bool,
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
                if let Some(msg) = self.uninstalled_tool_message(&config, &ts) {
                    bail!(msg);
                }
                if let Some(msg) =
                    crate::shims::unavailable_configured_tool_message(&config, &ts, &bin_name)
                {
                    bail!(msg);
                }
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
            tsb = tsb.with_args(std::slice::from_ref(tool));
        }
        let ts = tsb.build(config).await?;
        Ok(ts)
    }
    /// `--tool` replaces whatever the config requested, so a version that isn't
    /// installed leaves nothing for [`Toolset::which`] to search. Saying the bin
    /// "is not currently active" would blame the config instead of the flag.
    fn uninstalled_tool_message(&self, config: &Arc<Config>, ts: &Toolset) -> Option<String> {
        let tool = self.tool.as_ref()?;
        let tv = ts
            .list_current_versions()
            .into_iter()
            .find(|(b, tv)| {
                tv.ba() == tool.ba.as_ref() && !b.is_version_installed(config, tv, true)
            })
            .map(|(_, tv)| tv)?;
        let requested = tool.version.clone().unwrap_or_else(|| tv.version.clone());
        let resolved = if requested == tv.version {
            String::new()
        } else {
            format!(" (resolved to {})", tv.version)
        };
        Some(format!(
            "{}@{requested} is not installed{resolved}\n\
             hint: run `mise install {}@{requested}`, or `mise ls {}` to see installed versions",
            tool.short, tool.short, tool.short
        ))
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
