use color_eyre::Result;

use crate::cli::command::Command;
use crate::config::config_file::rtx_toml::RtxToml;
use crate::config::config_file::{self, ConfigFile};
use crate::config::Config;
use crate::dirs;
use crate::env::RTX_DEFAULT_CONFIG_FILENAME;
use crate::output::Output;

use super::args::env_var::{EnvVarArg, EnvVarArgParser};

/// Manage environment variables
///
/// By default this command modifies '.rtx.toml' in the current directory.
/// Use the --file option to specify another file.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct EnvVars {
    #[clap(long, verbatim_doc_comment, default_value = ".rtx.toml")]
    file: String,

    /// Environment variable(s) to set
    /// e.g.: NODE_ENV=production
    #[clap(value_parser = EnvVarArgParser, verbatim_doc_comment, required = true)]
    env_vars: Vec<EnvVarArg>,
}

impl Command for EnvVars {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let mut rtx_toml = get_local_rtx_toml(&config)?;
        for ev in self.env_vars {
            rtx_toml.update_env(&ev.key, ev.value);
        }
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
