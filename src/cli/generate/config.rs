use crate::Result;
use crate::cli::edit;

/// Generate a mise.toml file
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = edit::AFTER_LONG_HELP)]
pub struct Config {
    #[clap(flatten)]
    edit: edit::Edit,
}

impl Config {
    pub async fn run(self) -> Result<()> {
        self.edit.run().await
    }
}
