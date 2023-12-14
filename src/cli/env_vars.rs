use color_eyre::Result;

use crate::config::config_file::rtx_toml::RtxToml;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::dirs;
use crate::env::RTX_DEFAULT_CONFIG_FILENAME;
use crate::file::display_path;

use super::args::env_var::{EnvVarArg, EnvVarArgParser};

/// Manage environment variables
///
/// By default this command modifies ".rtx.toml" in the current directory.
/// You can specify the file name by either setting the RTX_DEFAULT_CONFIG_FILENAME environment variable, or by using the --file option.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct EnvVars {
    /// The TOML file to update
    ///
    /// Defaults to RTX_DEFAULT_CONFIG_FILENAME environment variable, or ".rtx.toml".
    #[clap(long, verbatim_doc_comment, required = false, value_hint = clap::ValueHint::FilePath)]
    file: Option<String>,

    /// Remove the environment variable from config file
    ///
    /// Can be used multiple times.
    #[clap(long, value_name = "ENV_VAR", verbatim_doc_comment, aliases = ["rm", "unset"])]
    remove: Option<Vec<String>>,

    /// Environment variable(s) to set
    /// e.g.: NODE_ENV=production
    #[clap(value_parser = EnvVarArgParser, verbatim_doc_comment)]
    env_vars: Option<Vec<EnvVarArg>>,
}

impl EnvVars {
    pub fn run(self, config: &Config) -> Result<()> {
        if self.remove.is_none() && self.env_vars.is_none() {
            for (key, value) in &config.env {
                let source = config.env_sources.get(key).unwrap();
                rtxprintln!("{key}={value} {}", display_path(source));
            }
            return Ok(());
        }

        let filename = self
            .file
            .unwrap_or_else(|| RTX_DEFAULT_CONFIG_FILENAME.to_string());

        let mut rtx_toml = get_rtx_toml(filename.as_str())?;

        if let Some(env_names) = &self.remove {
            for name in env_names {
                rtx_toml.remove_env(name);
            }
        }

        if let Some(env_vars) = self.env_vars {
            for ev in env_vars {
                rtx_toml.update_env(&ev.key, ev.value);
            }
        }
        rtx_toml.save()
    }
}

fn get_rtx_toml(filename: &str) -> Result<RtxToml> {
    let path = dirs::CURRENT.join(filename);
    let rtx_toml = if path.exists() {
        RtxToml::from_file(&path)?
    } else {
        RtxToml::init(&path)
    };

    Ok(rtx_toml)
}

#[cfg(test)]
mod tests {
    use crate::{dirs, file};
    use std::path::PathBuf;

    fn remove_config_file(filename: &str) -> PathBuf {
        let cf_path = dirs::CURRENT.join(filename);
        let _ = file::remove_file(&cf_path);
        cf_path
    }

    #[test]
    fn test_show_env_vars() {
        assert_cli_snapshot!("env-vars");
    }

    #[test]
    fn test_env_vars() {
        // Using the default file
        let filename = ".test.rtx.toml";
        let cf_path = remove_config_file(filename);
        assert_cli!("env-vars", "FOO=bar");
        assert_snapshot!(file::read_to_string(cf_path).unwrap());
        remove_config_file(filename);

        // Using a custom file
        let filename = ".test-custom.rtx.toml";
        let cf_path = remove_config_file(filename);
        assert_cli!("env-vars", "--file", filename, "FOO=bar");
        assert_snapshot!(file::read_to_string(cf_path).unwrap());
        remove_config_file(filename);
    }

    #[test]
    fn test_env_vars_remove() {
        // Using the default file
        let filename = ".test.rtx.toml";
        let cf_path = remove_config_file(filename);
        assert_cli!("env-vars", "BAZ=quux");
        assert_cli!("env-vars", "--remove", "BAZ");
        assert_snapshot!(file::read_to_string(cf_path).unwrap());
        remove_config_file(filename);

        // Using a custom file
        let filename = ".test-custom.rtx.toml";
        let cf_path = remove_config_file(filename);
        assert_cli!("env-vars", "--file", filename, "BAZ=quux");
        assert_cli!("env-vars", "--file", filename, "--remove", "BAZ");
        assert_snapshot!(file::read_to_string(cf_path).unwrap());
        remove_config_file(filename);
    }
}
