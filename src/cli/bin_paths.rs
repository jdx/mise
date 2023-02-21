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
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().with_install_missing().build(&config);
        for p in ts.list_paths() {
            rtxprintln!(out, "{}", p.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;
    use crate::cli::tests::grep;

    #[test]
    fn test_bin_paths() {
        let stdout = assert_cli!("bin-paths");
        assert_str_eq!(grep(stdout, "tiny"), "~/data/installs/tiny/2.1.0/bin");
    }
}
