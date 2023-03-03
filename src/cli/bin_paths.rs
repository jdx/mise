use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::toolset::ToolsetBuilder;

/// List all the active runtime bin paths
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct BinPaths {}

impl Command for BinPaths {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new()
            .with_install_missing()
            .build(&mut config);
        for p in ts.list_paths(&config) {
            rtxprintln!(out, "{}", p.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use crate::assert_cli_snapshot;

    #[test]
    fn test_bin_paths() {
        assert_cli_snapshot!("bin-paths");
    }
}
