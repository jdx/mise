use std::path::PathBuf;

use eyre::Result;

use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{ConfigPathOptions, resolve_target_config_path};

/// Remove environment variable(s) from the config file.
///
/// By default, this command modifies `mise.toml` in the current directory.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Unset {
    /// Environment variable(s) to remove
    /// e.g.: NODE_ENV
    #[clap(verbatim_doc_comment, value_name = "ENV_KEY")]
    keys: Vec<String>,

    /// Specify a file to use instead of `mise.toml`
    ///
    /// Defaults to [`MISE_DEFAULT_CONFIG_FILENAME`](https://mise.en.dev/configuration.html#mise_default_config_filename) environment variable, or `mise.toml`.
    /// Use [`MISE_GLOBAL_CONFIG_FILE`](https://mise.en.dev/configuration.html#mise_global_config_file) to choose a different global config path.
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
    pub async fn run(self) -> Result<()> {
        let filename = resolve_target_config_path(ConfigPathOptions {
            global: self.global,
            path: self.file.clone(),
            env: None,
            cwd: None,
            prefer_toml: true,
            prevent_home_local: true,
        })?;

        let mut config = MiseToml::from_file(&filename).unwrap_or_default();

        for name in self.keys.iter() {
            config.remove_env(name)?;
        }

        config.save()
    }
}
