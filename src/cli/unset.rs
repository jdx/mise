use std::path::{Path, PathBuf};

use eyre::Result;

use crate::config::config_file::mise_toml::MiseToml;
use crate::config::config_file::ConfigFile;
use crate::env;

/// Remove environment variable(s) from the config file
///
/// By default this command modifies ".mise.toml" in the current directory.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Unset {
    /// Environment variable(s) to remove
    /// e.g.: NODE_ENV
    #[clap(verbatim_doc_comment)]
    keys: Vec<String>,

    /// Specify a file to use instead of ".mise.toml"
    #[clap(short, long, value_hint = clap::ValueHint::FilePath)]
    file: Option<PathBuf>,

    /// Use the global config file
    #[clap(short, long, overrides_with = "file")]
    global: bool,
}

impl Unset {
    pub fn run(self) -> Result<()> {
        let filename = self.file.unwrap_or_else(|| match self.global {
            true => env::MISE_GLOBAL_CONFIG_FILE.clone(),
            false => env::MISE_DEFAULT_CONFIG_FILENAME.clone().into(),
        });

        let mut mise_toml = get_mise_toml(&filename)?;

        for name in self.keys.iter() {
            mise_toml.remove_env(name)?;
        }

        mise_toml.save()
    }
}

fn get_mise_toml(filename: &Path) -> Result<MiseToml> {
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
    use std::path::PathBuf;

    use crate::{env, file};

    fn remove_config_file(filename: &str) -> PathBuf {
        let cf_path = env::current_dir().unwrap().join(filename);
        let _ = file::write(&cf_path, "");
        cf_path
    }

    #[test]
    fn test_unset_remove() {
        // Using the default file
        let filename = ".test.mise.toml";
        let cf_path = remove_config_file(filename);
        assert_cli_snapshot!("env-vars", "BAZ=quux", @"");
        assert_cli_snapshot!("set", "BAZ", @"quux");
        assert_cli_snapshot!("unset", "BAZ", @"");
        assert_snapshot!(file::read_to_string(cf_path).unwrap());
        remove_config_file(filename);
        file::remove_file(filename).unwrap();
    }
}
