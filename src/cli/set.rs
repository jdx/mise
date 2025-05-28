use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::args::EnvVarArg;
use crate::config;
use crate::config::Config;
use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::env_directive::EnvDirective;
use crate::env::{self};
use crate::file::display_path;
use crate::ui::table;
use eyre::{Result, bail};
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

    /// Render completions
    #[clap(long, hide = true)]
    complete: bool,

    /// Set the environment variable in the global config file
    #[clap(short, long, verbatim_doc_comment, overrides_with = "file")]
    global: bool,

    /// Remove the environment variable from config file
    ///
    /// Can be used multiple times.
    #[clap(long, value_name = "ENV_KEY", verbatim_doc_comment, visible_aliases = ["rm", "unset"], hide = true)]
    remove: Option<Vec<String>>,

    /// Environment variable(s) to set
    /// e.g.: NODE_ENV=production
    #[clap(value_name = "ENV_VAR", verbatim_doc_comment)]
    env_vars: Option<Vec<EnvVarArg>>,
}

impl Set {
    pub async fn run(self) -> Result<()> {
        if self.complete {
            return self.complete().await;
        }
        match (&self.remove, &self.env_vars) {
            (None, None) => {
                return self.list_all().await;
            }
            (None, Some(env_vars)) if env_vars.iter().all(|ev| ev.value.is_none()) => {
                return self.get().await;
            }
            _ => {}
        }

        let filename = self.filename();
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
                    EnvDirective::Val(k, v, _) if &k == key => Some(v),
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

    async fn complete(&self) -> Result<()> {
        for ev in self.cur_env().await? {
            println!("{}", ev.key);
        }
        Ok(())
    }

    async fn list_all(self) -> Result<()> {
        let env = self.cur_env().await?;
        let mut table = tabled::Table::new(env);
        table::default_style(&mut table, false);
        miseprintln!("{table}");
        Ok(())
    }

    async fn get(self) -> Result<()> {
        let env = self.cur_env().await?;
        let filter = self.env_vars.unwrap();
        let vars = filter
            .iter()
            .filter_map(|ev| {
                env.iter()
                    .find(|r| r.key == ev.key)
                    .map(|r| (r.key.clone(), r.value.clone()))
            })
            .collect::<HashMap<String, String>>();
        for eva in filter {
            if let Some(value) = vars.get(&eva.key) {
                miseprintln!("{value}");
            } else {
                bail!("Environment variable {} not found", eva.key);
            }
        }
        Ok(())
    }

    async fn cur_env(&self) -> Result<Vec<Row>> {
        let rows = if let Some(file) = &self.file {
            let config = MiseToml::from_file(file).unwrap_or_default();
            config
                .env_entries()?
                .into_iter()
                .filter_map(|ed| match ed {
                    EnvDirective::Val(key, value, _) => Some(Row {
                        key,
                        value,
                        source: display_path(file),
                    }),
                    _ => None,
                })
                .collect()
        } else {
            Config::get()
                .await?
                .env_with_sources()
                .await?
                .iter()
                .map(|(key, (value, source))| Row {
                    key: key.clone(),
                    value: value.clone(),
                    source: display_path(source),
                })
                .collect()
        };
        Ok(rows)
    }

    fn filename(&self) -> PathBuf {
        if let Some(file) = &self.file {
            file.clone()
        } else if self.global {
            config::global_config_path()
        } else {
            config::local_toml_config_path()
        }
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

#[derive(Tabled, Debug, Clone)]
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
