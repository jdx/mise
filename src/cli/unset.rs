use std::path::{Path, PathBuf};

use eyre::Result;

use crate::config::config_file::mise_toml::MiseToml;
use crate::config::config_file::ConfigFile;
use crate::config::{is_global_config, LOCAL_CONFIG_FILENAMES};
use crate::env::{self, MISE_DEFAULT_CONFIG_FILENAME};
use crate::file;

/// Remove environment variable(s) from the config file.
///
/// By default, this command modifies `mise.toml` in the current directory.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Unset {
    /// Environment variable(s) to remove
    /// e.g.: NODE_ENV
    #[clap(verbatim_doc_comment)]
    keys: Vec<String>,

    /// Specify a file to use instead of `mise.toml`
    #[clap(short, long, value_hint = clap::ValueHint::FilePath)]
    file: Option<PathBuf>,

    /// Use the global config file
    #[clap(short, long, overrides_with = "file")]
    global: bool,
}

const AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Remove NODE_ENV from the current directory's config
    $ <bold>mise unset NODE_ENV</bold>

    # Remove NODE_ENV from the global config
    $ <bold>mise unset NODE_ENV -g</bold>
"#
);

impl Unset {
    pub fn run(self) -> Result<()> {
        let filename = if let Some(env) = &*env::MISE_PROFILE {
            config_file_from_dir(&env::current_dir()?.join(format!(".mise.{}.toml", env)))
        } else if self.global {
            env::MISE_GLOBAL_CONFIG_FILE.clone()
        } else if let Some(p) = &self.file {
            config_file_from_dir(p)
        } else {
            env::MISE_DEFAULT_CONFIG_FILENAME.clone().into()
        };

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

fn config_file_from_dir(p: &Path) -> PathBuf {
    if !p.is_dir() {
        return p.to_path_buf();
    }
    let mise_toml = p.join(&*MISE_DEFAULT_CONFIG_FILENAME);
    if mise_toml.exists() {
        return mise_toml;
    }
    let filenames = LOCAL_CONFIG_FILENAMES
        .iter()
        .rev()
        .filter(|f| is_global_config(Path::new(f)))
        .map(|f| f.to_string())
        .collect::<Vec<_>>();
    if let Some(p) = file::find_up(p, &filenames) {
        return p;
    }
    mise_toml
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use insta::assert_snapshot;

    use crate::test::reset;
    use crate::{env, file};

    fn remove_config_file(filename: &str) -> PathBuf {
        let cf_path = env::current_dir().unwrap().join(filename);
        let _ = file::write(&cf_path, "");
        cf_path
    }

    #[test]
    fn test_unset_remove() {
        reset();
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
