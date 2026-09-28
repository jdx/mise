use crate::config::{
    ConfigPathOptions, resolve_target_config_path, system_config_path, top_toml_config,
};
use eyre::Result;
use std::path::PathBuf;

mod get;
mod keys;
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

/// The TOML file `config get` and `config set` act on.
///
/// Only an explicitly named target goes through the shared resolver — the default is a
/// different rule (the top TOML config of the loaded set, not the nearest writable one).
fn target_file(file: Option<PathBuf>, global: bool, system: bool) -> Result<Option<PathBuf>> {
    Ok(match file {
        Some(path) => Some(resolve_target_config_path(ConfigPathOptions {
            path: Some(path),
            prefer_toml: true,
            ..Default::default()
        })?),
        None if global => Some(resolve_target_config_path(ConfigPathOptions {
            global: true,
            prefer_toml: true,
            ..Default::default()
        })?),
        None if system => Some(system_config_path()),
        None => top_toml_config(),
    })
}
