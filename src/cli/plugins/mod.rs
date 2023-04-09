use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

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
    /// same as `rtx plugins ls-remote`
    #[clap(short, long, hide = true)]
    pub all: bool,

    /// The built-in plugins only
    /// Normally these are not shown
    #[clap(short, long, verbatim_doc_comment)]
    pub core: bool,

    /// show the git url for each plugin
    ///
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(short, long)]
    pub urls: bool,
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
    pub fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match self {
            Self::Install(cmd) => cmd.run(config, out),
            Self::Link(cmd) => cmd.run(config, out),
            Self::Ls(cmd) => cmd.run(config, out),
            Self::LsRemote(cmd) => cmd.run(config, out),
            Self::Uninstall(cmd) => cmd.run(config, out),
            Self::Update(cmd) => cmd.run(config, out),
        }
    }
}

impl Command for Plugins {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::PluginsLs {
            all: self.all,
            core: self.core,
            urls: self.urls,
        }));

        cmd.run(config, out)
    }
}
