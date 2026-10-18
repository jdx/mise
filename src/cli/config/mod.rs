use crate::config::{
    ConfigPathOptions, override_config_path, resolve_target_config_path, system_config_path,
    top_toml_config,
};
use eyre::Result;
use std::path::PathBuf;

mod explain;
mod get;
mod keys;
mod ls;
mod set;

/// Manage config files
///
/// With no subcommand, lists the config files mise loads, from lowest to highest
/// precedence, like `mise config ls`; the flags below apply to that listing.
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
    Explain(explain::ConfigExplain),
    Get(get::ConfigGet),
    #[usage(visible_alias = "list")]
    Ls(ls::ConfigLs),
    Set(set::ConfigSet),
}

impl Commands {
    pub(crate) async fn run(self) -> Result<()> {
        match self {
            Self::Explain(cmd) => cmd.run(),
            Self::Get(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run().await,
            Self::Set(cmd) => cmd.run(),
        }
    }
}

impl Config {
    /// Whether this only reads a named file, so it must work when the settings that file sets
    /// would fail to load: `config explain --file` describes a broken file rather than refusing.
    /// `dir` is the directory `--cd` selects, which a relative `--file` is in.
    pub(crate) fn is_file_only(&self, dir: Option<&std::path::Path>) -> bool {
        matches!(&self.command, Some(Commands::Explain(cmd)) if cmd.reads_named_file(dir))
    }

    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(self.ls));

        cmd.run().await
    }
}

/// The TOML file `config get` and `config set` act on.
///
/// Only an explicitly named target goes through the shared resolver — the default is a
/// different rule (the top TOML config of the loaded set, not the nearest writable one).
fn target_file(
    file: Option<PathBuf>,
    global: bool,
    system: bool,
    override_file: bool,
) -> Result<Option<PathBuf>> {
    Ok(match file {
        Some(path) => Some(resolve_target_config_path(ConfigPathOptions {
            path: Some(path),
            prefer_toml: true,
            ..Default::default()
        })?),
        None if override_file => Some(override_config_path(None)?),
        None if global => Some(resolve_target_config_path(ConfigPathOptions {
            global: true,
            prefer_toml: true,
            ..Default::default()
        })?),
        None if system => Some(system_config_path()),
        None => top_toml_config(),
    })
}
