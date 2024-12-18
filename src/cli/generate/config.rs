use crate::cli::config::generate;
use crate::Result;

/// [experimental] Generate a mise.toml file
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = generate::AFTER_LONG_HELP)]
pub struct Config {
    #[clap(flatten)]
    generate: generate::ConfigGenerate,
}

impl Config {
    pub fn run(self) -> Result<()> {
        self.generate.run()
    }
}
