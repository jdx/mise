use color_eyre::Result;

use crate::config::config_file::mise_toml::MiseToml;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::env;
use crate::env::MISE_DEFAULT_CONFIG_FILENAME;
use crate::file::display_path;

use super::args::env_var::{EnvVarArg, EnvVarArgParser};

/// Manage environment variables
///
/// By default this command modifies ".mise.toml" in the current directory.
/// You can specify the file name by either setting the MISE_DEFAULT_CONFIG_FILENAME environment variable, or by using the --file option.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "ev", verbatim_doc_comment)]
pub struct EnvVars {
    /// The TOML file to update
    ///
    /// Defaults to MISE_DEFAULT_CONFIG_FILENAME environment variable, or ".mise.toml".
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
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        if self.remove.is_none() && self.env_vars.is_none() {
            for (key, value) in &config.env {
                let source = config.env_sources.get(key).unwrap();
                miseprintln!("{key}={value} {}", display_path(source));
            }
            return Ok(());
        }

        let filename = self
            .file
            .unwrap_or_else(|| MISE_DEFAULT_CONFIG_FILENAME.to_string());

        let mut mise_toml = get_mise_toml(filename.as_str())?;

        if let Some(env_names) = &self.remove {
            for name in env_names {
                mise_toml.remove_env(name);
            }
        }

        if let Some(env_vars) = self.env_vars {
            for ev in env_vars {
                mise_toml.update_env(&ev.key, ev.value);
            }
        }
        mise_toml.save()
    }
}

fn get_mise_toml(filename: &str) -> Result<MiseToml> {
    let path = env::current_dir()?.join(filename);
    let mise_toml = if path.exists() {
        MiseToml::from_file(&path)?
    } else {
        MiseToml::init(&path)
    };

    Ok(mise_toml)
}

#[cfg(test)]
mod tests {
    use crate::{env, file};
    use std::path::PathBuf;

    fn remove_config_file(filename: &str) -> PathBuf {
        let cf_path = env::current_dir().unwrap().join(filename);
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
        let filename = ".test.mise.toml";
        let cf_path = remove_config_file(filename);
        assert_cli!("env-vars", "FOO=bar");
        assert_snapshot!(file::read_to_string(cf_path).unwrap());
        remove_config_file(filename);

        // Using a custom file
        let filename = ".test-custom.mise.toml";
        let cf_path = remove_config_file(filename);
        assert_cli!("env-vars", "--file", filename, "FOO=bar");
        assert_snapshot!(file::read_to_string(cf_path).unwrap());
        remove_config_file(filename);
    }

    #[test]
    fn test_env_vars_remove() {
        // Using the default file
        let filename = ".test.mise.toml";
        let cf_path = remove_config_file(filename);
        assert_cli!("env-vars", "BAZ=quux");
        assert_cli!("env-vars", "--remove", "BAZ");
        assert_snapshot!(file::read_to_string(cf_path).unwrap());
        remove_config_file(filename);

        // Using a custom file
        let filename = ".test-custom.mise.toml";
        let cf_path = remove_config_file(filename);
        assert_cli!("env-vars", "--file", filename, "BAZ=quux");
        assert_cli!("env-vars", "--file", filename, "--remove", "BAZ");
        assert_snapshot!(file::read_to_string(cf_path).unwrap());
        remove_config_file(filename);
    }
}
