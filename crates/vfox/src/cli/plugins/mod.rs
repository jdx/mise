use vfox::VfoxResult;

mod list;

#[derive(usage_rs::Subcommands)]
pub(crate) enum Commands {
    #[usage(alias_hidden = "ls")]
    List(list::List),
}

#[derive(usage_rs::Args)]
pub(crate) struct Plugins {
    #[usage(subcommand)]
    command: Commands,
}

impl Plugins {
    pub(crate) async fn run(&self) -> VfoxResult<()> {
        match &self.command {
            Commands::List(list) => list.run().await,
        }
    }
}
