use eyre::Result;

mod node;
mod python;
mod reconcile;
mod ruby;

#[derive(Debug, usage_rs::Args)]
#[usage(about = "Synchronize tools from other version managers with mise")]
pub(crate) struct Sync {
    #[usage(subcommand)]
    command: Commands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Node(node::SyncNode),
    Python(python::SyncPython),
    Ruby(ruby::SyncRuby),
}

impl Commands {
    pub(crate) async fn run(self) -> Result<()> {
        match self {
            Self::Node(cmd) => cmd.run().await,
            Self::Python(cmd) => cmd.run().await,
            Self::Ruby(cmd) => cmd.run().await,
        }
    }
}

impl Sync {
    pub(crate) async fn run(self) -> Result<()> {
        self.command.run().await
    }
}
