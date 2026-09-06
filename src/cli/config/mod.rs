use eyre::Result;

mod get;
mod ls;
mod set;

/// Manage config files
#[derive(Debug, usage_rs::Args)]
#[usage(visible_alias = "cfg", alias = "toml")]
pub(crate) struct Config {
    #[usage(subcommand)]
    command: Option<Commands>,

    #[usage(flatten)]
    pub ls: ls::ConfigLs,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Get(get::ConfigGet),
    #[usage(visible_alias = "list")]
    Ls(ls::ConfigLs),
    Set(set::ConfigSet),
}

impl Commands {
    pub(crate) async fn run(self) -> Result<()> {
        match self {
            Self::Get(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run().await,
            Self::Set(cmd) => cmd.run(),
        }
    }
}

impl Config {
    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(self.ls));

        cmd.run().await
    }
}
