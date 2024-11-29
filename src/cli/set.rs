use std::path::{Path, PathBuf};

use super::args::EnvVarArg;
use crate::config;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::config_file::ConfigFile;
use crate::config::env_directive::EnvDirective;
use crate::config::Config;
use crate::env::{self};
use crate::file::display_path;
use crate::ui::table;
use eyre::{bail, Result};
use tabled::Tabled;

/// Set environment variables in mise.toml
///
/// By default, this command modifies `mise.toml` in the current directory.
#[derive(Debug, clap::Args)]
#[clap(aliases = ["ev", "env-vars"], verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Set {
    /// The TOML file to update
    ///
    /// Defaults to MISE_DEFAULT_CONFIG_FILENAME environment variable, or `mise.toml`.
    #[clap(long, verbatim_doc_comment, required = false, value_hint = clap::ValueHint::FilePath)]
    file: Option<PathBuf>,

    /// Set the environment variable in the global config file
    #[clap(short, long, verbatim_doc_comment, overrides_with = "file")]
    global: bool,

    /// Remove the environment variable from config file
    ///
    /// Can be used multiple times.
    #[clap(long, value_name = "ENV_VAR", verbatim_doc_comment, aliases = ["rm", "unset"], hide = true)]
    remove: Option<Vec<String>>,

    /// Environment variable(s) to set
    /// e.g.: NODE_ENV=production
    #[clap(verbatim_doc_comment)]
    env_vars: Option<Vec<EnvVarArg>>,
}

impl Set {
    pub fn run(self) -> Result<()> {
        let filename = if let Some(file) = &self.file {
            file.clone()
        } else if self.global {
            config::global_config_path()
        } else {
            config::local_toml_config_path()
        };

        if self.remove.is_none() && self.env_vars.is_none() {
            let rows = Config::get()
                .env_with_sources()?
                .iter()
                .filter(|(_, (_, source))| {
                    if self.file.is_some() {
                        source == &filename
                    } else {
                        true
                    }
                })
                .map(|(key, (value, source))| Row {
                    key: key.clone(),
                    value: value.clone(),
                    source: display_path(source),
                })
                .collect::<Vec<_>>();
            let mut table = tabled::Table::new(rows);
            table::default_style(&mut table, false);
            miseprintln!("{table}");
            return Ok(());
        }

        let config = MiseToml::from_file(&filename).unwrap_or_default();

        let mut mise_toml = get_mise_toml(&filename)?;

        if let Some(env_names) = &self.remove {
            for name in env_names {
                mise_toml.remove_env(name)?;
            }
        }

        if let Some(env_vars) = self.env_vars {
            if env_vars.len() == 1 && env_vars[0].value.is_none() {
                let key = &env_vars[0].key;
                match config.env_entries()?.into_iter().find_map(|ev| match ev {
                    EnvDirective::Val(k, v) if &k == key => Some(v),
                    _ => None,
                }) {
                    Some(value) => miseprintln!("{value}"),
                    None => bail!("Environment variable {key} not found"),
                }
                return Ok(());
            }
            for ev in env_vars {
                match ev.value {
                    Some(value) => mise_toml.update_env(&ev.key, value)?,
                    None => bail!("{} has no value", ev.key),
                }
            }
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

#[derive(Tabled)]
struct Row {
    key: String,
    value: String,
    source: String,
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise set NODE_ENV=production</bold>

    $ <bold>mise set NODE_ENV</bold>
    production

    $ <bold>mise set</bold>
    key       value       source
    NODE_ENV  production  ~/.config/mise/config.toml
"#
);
