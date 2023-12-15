use clap::Subcommand;
use eyre::Result;

mod generate;
mod ls;

/// [experimental] Manage config files
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "cfg")]
pub struct Config {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Do not print table header
    #[clap(long, alias = "no-headers", verbatim_doc_comment)]
    no_header: bool,

    /// List all possible config filenames
    #[clap(long, verbatim_doc_comment, conflicts_with = "directories")]
    pub filenames: bool,

    /// List all possible config directories
    #[clap(long, verbatim_doc_comment, conflicts_with = "filenames")]
    pub directories: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Ls(ls::ConfigLs),
    Generate(generate::ConfigGenerate),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Ls(cmd) => cmd.run(),
            Self::Generate(cmd) => cmd.run(),
        }
    }
}

impl Config {
    pub fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::ConfigLs {
            no_header: self.no_header,
            filenames: self.filenames,
            directories: self.directories,
        }));

        cmd.run()
    }
}
