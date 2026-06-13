use clap::Subcommand;
use eyre::Result;

mod install;
mod status;

/// [experimental] Manage dotfiles from `[dotfiles]`
///
/// Dotfiles are config files symlinked, copied, or rendered to target paths,
/// plus marker-delimited blocks or single lines in files mise doesn't own.
/// Unlike `[tools]`, dotfiles are only acted on when explicitly requested with
/// `mise dotfiles install` or `mise bootstrap`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Dotfiles {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install(install::DotfilesInstall),
    Status(status::DotfilesStatus),
}

impl Dotfiles {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Install(cmd) => cmd.run().await,
            Commands::Status(cmd) => cmd.run().await,
        }
    }
}
