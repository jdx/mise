use color_eyre::eyre::Result;

use crate::config::Config;

use crate::toolset::ToolsetBuilder;

/// List all the active runtime bin paths
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct BinPaths {}

impl BinPaths {
    pub fn run(self, config: &Config) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config)?;
        for p in ts.list_paths() {
            rtxprintln!("{}", p.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_bin_paths() {
        assert_cli!("i");
        assert_cli_snapshot!("bin-paths");
    }
}
