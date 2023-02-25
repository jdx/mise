use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::cli::Cli;
use crate::config::Config;
use crate::dirs;
use crate::output::Output;

/// Generate man pages
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true)]
pub struct Mangen {}

impl Command for Mangen {
    fn run(self, _config: Config, _out: &mut Output) -> Result<()> {
        let cli = Cli::command().version(env!("CARGO_PKG_VERSION"));

        let man = clap_mangen::Man::new(cli);
        let mut buffer: Vec<u8> = Default::default();
        man.render(&mut buffer)?;

        let out_dir = dirs::CURRENT.join("man").join("man1");
        std::fs::create_dir_all(&out_dir)?;
        std::fs::write(out_dir.join("rtx.1"), buffer)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::assert_cli_snapshot;

    #[test]
    fn test_complete() {
        assert_cli_snapshot!("mangen");
    }
}
