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
/// By default this command modifies ".rtx.toml" in the current directory.
/// You can specify the file name by either setting the RTX_DEFAULT_CONFIG_FILENAME environment variable, or using the --file option.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct EnvVars {
    /// The TOML file to update
    ///
    /// Defaults to RTX_DEFAULT_CONFIG_FILENAME environment variable, or ".rtx.toml".
    #[clap(long, verbatim_doc_comment, required = false)]
    file: Option<String>,

    /// Environment variable(s) to set
    /// e.g.: NODE_ENV=production
    #[clap(value_parser = EnvVarArgParser, verbatim_doc_comment, required = true)]
    env_vars: Vec<EnvVarArg>,
}

impl Command for EnvVars {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let filename = self
            .file
            .unwrap_or_else(|| RTX_DEFAULT_CONFIG_FILENAME.to_string());

        let mut rtx_toml = get_rtx_toml(&config, filename.as_str())?;
        for ev in self.env_vars {
            rtx_toml.update_env(&ev.key, ev.value);
        }
        rtx_toml.save()
    }
}

fn get_rtx_toml(config: &Config, filename: &str) -> Result<RtxToml> {
    let path = dirs::CURRENT.join(filename);
    let is_trusted = config_file::is_trusted(&config.settings, &path);
    let rtx_toml = if path.exists() {
        RtxToml::from_file(&path, is_trusted)?
    } else {
        RtxToml::init(&path, is_trusted)
    };

    Ok(rtx_toml)
}
