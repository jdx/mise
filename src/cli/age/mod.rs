use clap::Subcommand;
use eyre::Result;

mod config;
mod decrypt;
mod doctor;
mod edit;
mod encrypt;
mod import_ssh;
mod inspect;
mod keygen;
mod keys;
mod recipients;
mod rekey;

#[derive(Debug, clap::Args)]
#[clap(about = "[experimental] Age encryption commands for environment variables")]
pub struct Age {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Config(config::Config),
    #[clap(visible_alias = "reveal")]
    Decrypt(decrypt::Decrypt),
    Doctor(doctor::Doctor),
    Edit(edit::Edit),
    #[clap(visible_alias = "seal")]
    Encrypt(encrypt::Encrypt),
    ImportSsh(import_ssh::ImportSsh),
    Inspect(inspect::Inspect),
    Keys(keys::Keys),
    Keygen(keygen::Keygen),
    Recipients(recipients::Recipients),
    #[clap(visible_aliases = &["rewrap", "rotate"])]
    Rekey(rekey::Rekey),
}

impl Age {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Config(cmd) => cmd.run().await,
            Commands::Decrypt(cmd) => cmd.run().await,
            Commands::Doctor(cmd) => cmd.run().await,
            Commands::Edit(cmd) => cmd.run().await,
            Commands::Encrypt(cmd) => cmd.run().await,
            Commands::ImportSsh(cmd) => cmd.run().await,
            Commands::Inspect(cmd) => cmd.run().await,
            Commands::Keys(cmd) => cmd.run().await,
            Commands::Keygen(cmd) => cmd.run().await,
            Commands::Recipients(cmd) => cmd.run().await,
            Commands::Rekey(cmd) => cmd.run().await,
        }
    }
}
