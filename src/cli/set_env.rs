use std::path::PathBuf;

use crate::cli::command::Command;
use crate::config::config_file::rtx_toml::RtxToml;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::env::{RTX_DEFAULT_CONFIG_FILENAME, RTX_DEFAULT_TOOL_VERSIONS_FILENAME};
use crate::output::Output;
use crate::{dirs, env};
use color_eyre::eyre::Result;

/// Set environment variables for the current directory
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SetEnv {
    args: Vec<String>,
}

impl Command for SetEnv {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        // println!("{:?}", config);
        let rtx_toml = dirs::CURRENT.join(RTX_DEFAULT_CONFIG_FILENAME.as_str());
        let x = RtxToml::init(&rtx_toml, false);
        let s = x.dump();
        println!("{s}");

        Ok(())
    }
}
