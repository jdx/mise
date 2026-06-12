use clap::Subcommand;
use eyre::Result;

mod install;
mod status;

/// [experimental] Manage system packages from `[system.packages]`
///
/// System packages are machine-global packages installed by the OS package
/// manager (apt, dnf, pacman) or mise's Homebrew-bottle installer (brew).
/// Unlike `[tools]`, they are not version-pinned per-project and are only
/// ever installed when explicitly requested with `mise system install`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct System {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install(install::SystemInstall),
    Status(status::SystemStatus),
}

impl System {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Install(cmd) => cmd.run().await,
            Commands::Status(cmd) => cmd.run().await,
        }
    }
}
