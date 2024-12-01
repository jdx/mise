use clap::Subcommand;
use eyre::Result;
mod generate;
mod get;
mod ls;
mod set;

/// Manage config files
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "cfg", alias = "toml", next_display_order = 0)]
pub struct Config {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Do not print table header
    #[clap(long, alias = "no-headers", verbatim_doc_comment, display_order = 0)]
    no_header: bool,

    /// Output in JSON format
    #[clap(short = 'J', long, verbatim_doc_comment, display_order = 0)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Generate(generate::ConfigGenerate),
    Get(get::ConfigGet),
    Ls(ls::ConfigLs),
    Set(set::ConfigSet),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Generate(cmd) => cmd.run(),
            Self::Get(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run(),
            Self::Set(cmd) => cmd.run(),
        }
    }
}

impl Config {
    pub fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::ConfigLs {
            no_header: self.no_header,
            json: self.json,
        }));

        cmd.run()
    }
}
