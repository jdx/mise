use std::sync::Arc;

use clap::Subcommand;
use eyre::Result;

use crate::config::Config;

mod install;
mod link;
mod ls;
mod ls_remote;
mod uninstall;
mod update;

#[derive(Debug, clap::Args)]
#[clap(about = "Manage plugins", visible_alias = "p", aliases = ["plugin", "plugin-list"])]
pub struct Plugins {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// list all available remote plugins
    ///
    /// same as `mise plugins ls-remote`
    #[clap(short, long, hide = true)]
    pub all: bool,

    /// The built-in plugins only
    /// Normally these are not shown
    #[clap(short, long, verbatim_doc_comment, conflicts_with = "all")]
    pub core: bool,

    /// List installed plugins
    ///
    /// This is the default behavior but can be used with --core
    /// to show core and user plugins
    #[clap(long, verbatim_doc_comment, conflicts_with = "all")]
    pub user: bool,

    /// Show the git url for each plugin
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(short, long, alias = "url", verbatim_doc_comment)]
    pub urls: bool,

    /// Show the git refs for each plugin
    /// e.g.: main 1234abc
    #[clap(long, hide = true, verbatim_doc_comment)]
    pub refs: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install(install::PluginsInstall),
    Link(link::PluginsLink),
    Ls(ls::PluginsLs),
    LsRemote(ls_remote::PluginsLsRemote),
    Uninstall(uninstall::PluginsUninstall),
    Update(update::Update),
}

impl Commands {
    pub async fn run(self, config: &Arc<Config>) -> Result<()> {
        match self {
            Self::Install(cmd) => cmd.run(config).await,
            Self::Link(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run(config).await,
            Self::LsRemote(cmd) => cmd.run(config).await,
            Self::Uninstall(cmd) => cmd.run().await,
            Self::Update(cmd) => cmd.run().await,
        }
    }
}

impl Plugins {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let cmd = self.command.unwrap_or(Commands::Ls(ls::PluginsLs {
            all: self.all,
            core: self.core,
            refs: self.refs,
            urls: self.urls,
            user: self.user,
        }));

        cmd.run(&config).await
    }
}
