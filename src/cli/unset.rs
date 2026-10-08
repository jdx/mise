use std::path::PathBuf;

use eyre::Result;

use crate::cli::set::get_mise_toml;
use crate::config::config_file::ConfigFile;
use crate::config::{ConfigPathOptions, resolve_target_config_path};

/// Remove environment variables from mise.toml
///
/// Edits the same file `mise set` writes: the lowest-precedence TOML file in the
/// nearest config directory, or the global config when run in your home
/// directory. Use `--global` or `--file` to pick another file.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise unset NODE_ENV"###,
        help = r###"Remove NODE_ENV from the project config"###
    ),
    example(
        r###"mise unset -g NODE_ENV"###,
        help = r###"Remove NODE_ENV from the global config"###
    )
)]
pub(crate) struct Unset {
    /// Environment variables to remove, such as NODE_ENV
    #[usage(verbatim_doc_comment, value_name = "ENV_KEY")]
    keys: Vec<String>,

    /// The TOML file to edit instead of the default target
    ///
    /// Can be a file or a directory. For a directory, mise uses the config file
    /// in it, or one named by MISE_DEFAULT_CONFIG_FILENAME (default `mise.toml`):
    /// https://mise.jdx.dev/configuration/settings.html#default_config_filename
    /// To move the global config file, set MISE_GLOBAL_CONFIG_FILE:
    /// https://mise.jdx.dev/configuration/settings.html#global_config_file
    #[usage(short, long, visible_alias = "path", value_hint = usage_rs::ValueHint::FilePath)]
    file: Option<PathBuf>,

    /// Use the global config file
    #[usage(short, long, overrides = "file")]
    global: bool,
}

impl Unset {
    pub(crate) async fn run(self) -> Result<()> {
        let filename = resolve_target_config_path(ConfigPathOptions {
            global: self.global,
            path: self.file.clone(),
            env: None,
            cwd: None,
            prefer_toml: true,
            prevent_home_local: true,
            ..Default::default()
        })?;

        let mut config = get_mise_toml(&filename).await?;

        for name in self.keys.iter() {
            config.remove_env(name)?;
        }

        config.save()
    }
}
