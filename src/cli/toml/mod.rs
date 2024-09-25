use crate::config::{load_config_paths, DEFAULT_CONFIG_FILENAMES};
use clap::Subcommand;
use eyre::Result;
use once_cell::sync::Lazy;
use std::path::PathBuf;

mod get;
mod set;

#[derive(Debug, clap::Args)]
#[clap(about = "Edit mise.toml files")]
pub struct Toml {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Get(get::TomlGet),
    Set(set::TomlSet),
}

pub static TOML_CONFIG_FILENAMES: Lazy<Vec<String>> = Lazy::new(|| {
    DEFAULT_CONFIG_FILENAMES
        .iter()
        .filter(|s| s.ends_with(".toml"))
        .map(|s| s.to_string())
        .collect()
});

fn top_toml_config() -> Option<PathBuf> {
    load_config_paths(&TOML_CONFIG_FILENAMES)
        .iter()
        .find(|p| p.to_string_lossy().ends_with(".toml"))
        .map(|p| p.to_path_buf())
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            // TODO: implement add for appending to arrays
            Self::Get(cmd) => cmd.run(),
            Self::Set(cmd) => cmd.run(),
        }
    }
}

impl Toml {
    pub fn run(self) -> Result<()> {
        let cmd = self.command.unwrap();

        cmd.run()
    }
}
