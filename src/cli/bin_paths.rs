use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::toolset::ToolsetBuilder;
use eyre::Result;

/// List all the active runtime bin paths
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct BinPaths {
    /// Tool(s) to look up
    /// e.g.: ruby@3
    #[clap(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Option<Vec<ToolArg>>,
}

impl BinPaths {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let mut tsb = ToolsetBuilder::new();
        if let Some(tool) = &self.tool {
            tsb = tsb.with_args(tool);
        }
        let mut ts = tsb.build(&config).await?;
        if let Some(tool) = &self.tool {
            ts.versions.retain(|k, _| tool.iter().any(|t| *t.ba == **k));
        }
        ts.notify_if_versions_missing(&config).await;
        for p in ts.list_paths(&config).await {
            miseprintln!("{}", p.display());
        }
        Ok(())
    }
}
