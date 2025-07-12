use vfox::VfoxResult;

mod list;

#[derive(clap::Subcommand)]
pub(crate) enum Commands {
    // Install(install::Install),
    List(list::List),
}

#[derive(clap::Args)]
pub(crate) struct Plugins {
    #[clap(subcommand)]
    command: Commands,
}

impl Plugins {
    pub(crate) async fn run(&self) -> VfoxResult<()> {
        match &self.command {
            Commands::List(list) => list.run().await,
        }
    }
}
