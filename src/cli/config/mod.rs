use crate::config::{load_config_paths, DEFAULT_CONFIG_FILENAMES};
use clap::Subcommand;
use eyre::Result;
use once_cell::sync::Lazy;
use std::path::PathBuf;

mod generate;
mod get;
mod ls;
mod set;

/// Manage config files
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "cfg", alias = "toml")]
pub struct Config {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Do not print table header
    #[clap(long, alias = "no-headers", verbatim_doc_comment)]
    no_header: bool,

    /// Output in JSON format
    #[clap(short = 'J', long, verbatim_doc_comment)]
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

pub static TOML_CONFIG_FILENAMES: Lazy<Vec<String>> = Lazy::new(|| {
    DEFAULT_CONFIG_FILENAMES
        .iter()
        .filter(|s| s.ends_with(".toml"))
        .map(|s| s.to_string())
        .collect()
});

fn top_toml_config() -> Option<PathBuf> {
    load_config_paths(&TOML_CONFIG_FILENAMES, false)
        .iter()
        .find(|p| p.to_string_lossy().ends_with(".toml"))
        .map(|p| p.to_path_buf())
}
