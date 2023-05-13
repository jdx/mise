use color_eyre::Result;

use crate::cli::command::Command;
use crate::config::config_file::rtx_toml::RtxToml;
use crate::config::config_file::{self, ConfigFile};
use crate::config::Config;
use crate::dirs;
use crate::env::RTX_DEFAULT_CONFIG_FILENAME;
use crate::output::Output;

/// Set/update environment variables for the current directory
///
/// This modifies the contents of ./.rtx.toml
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SetEnv {
    args: Vec<String>,
}

impl Command for SetEnv {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let mut rtx_toml = get_local_rtx_toml(&config)?;
        rtx_toml.update_env("HEY", "1234");
        rtx_toml.save()
    }
}

fn get_local_rtx_toml(config: &Config) -> Result<RtxToml> {
    let path = dirs::CURRENT.join(RTX_DEFAULT_CONFIG_FILENAME.as_str());
    let is_trusted = config_file::is_trusted(&config.settings, &path);
    let rtx_toml = if path.exists() {
        RtxToml::from_file(&path, is_trusted)?
    } else {
        RtxToml::init(&path, is_trusted)
    };

    Ok(rtx_toml)
}
