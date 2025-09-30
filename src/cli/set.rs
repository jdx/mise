use std::path::{Path, PathBuf};

use super::args::EnvVarArg;
use crate::agecrypt;
use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::env_directive::EnvDirective;
use crate::config::{Config, ConfigPathOptions, Settings, resolve_target_config_path};
use crate::env::{self};
use crate::file::display_path;
use crate::ui::table;
use demand::Input;
use eyre::{Result, bail, eyre};
use tabled::Tabled;

/// Set environment variables in mise.toml
///
/// By default, this command modifies `mise.toml` in the current directory.
/// Use `-E <env>` to create/modify environment-specific config files like `mise.<env>.toml`.
#[derive(Debug, clap::Args)]
#[clap(aliases = ["ev", "env-vars"], verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Set {
    /// The TOML file to update
    ///
    /// Can be a file path or directory. If a directory is provided, will create/use mise.toml in that directory.
    /// Defaults to MISE_DEFAULT_CONFIG_FILENAME environment variable, or `mise.toml`.
    #[clap(long, verbatim_doc_comment, required = false, value_hint = clap::ValueHint::AnyPath)]
    file: Option<PathBuf>,

    /// Render completions
    #[clap(long, hide = true)]
    complete: bool,

    /// Set the environment variable in the global config file
    #[clap(short, long, verbatim_doc_comment, overrides_with_all = &["file", "env"])]
    global: bool,

    /// Create/modify an environment-specific config file like .mise.<env>.toml
    #[clap(short = 'E', long, overrides_with_all = &["global", "file"])]
    env: Option<String>,

    /// Remove the environment variable from config file
    ///
    /// Can be used multiple times.
    #[clap(long, value_name = "ENV_KEY", verbatim_doc_comment, visible_aliases = ["rm", "unset"], hide = true)]
    remove: Option<Vec<String>>,

    /// Prompt for environment variable values
    #[clap(long)]
    prompt: bool,

    /// [experimental] Encrypt the value with age before storing
    #[clap(long, requires = "env_vars")]
    age_encrypt: bool,

    /// [experimental] Age recipient (x25519 public key) for encryption
    ///
    /// Can be used multiple times. Requires --age-encrypt.
    #[clap(long, value_name = "RECIPIENT", requires = "age_encrypt")]
    age_recipient: Vec<String>,

    /// [experimental] SSH recipient (public key or path) for age encryption
    ///
    /// Can be used multiple times. Requires --age-encrypt.
    #[clap(long, value_name = "PATH_OR_PUBKEY", requires = "age_encrypt")]
    age_ssh_recipient: Vec<String>,

    /// [experimental] Age identity file for encryption
    ///
    /// Defaults to ~/.config/mise/age.txt if it exists
    #[clap(long, value_name = "PATH", requires = "age_encrypt", value_hint = clap::ValueHint::FilePath)]
    age_key_file: Option<PathBuf>,

    /// Environment variable(s) to set
    /// e.g.: NODE_ENV=production
    #[clap(value_name = "ENV_VAR", verbatim_doc_comment)]
    env_vars: Option<Vec<EnvVarArg>>,
}

impl Set {
    /// Decrypt a value if it's encrypted, otherwise return it as-is
    async fn decrypt_value_if_needed(
        key: &str,
        value: &str,
        directive: Option<&EnvDirective>,
    ) -> Result<String> {
        // If we have an Age directive, use the specialized decryption
        if let Some(EnvDirective::Age { .. }) = directive {
            agecrypt::decrypt_age_directive(directive.unwrap())
                .await
                .map_err(|e| eyre!("[experimental] Failed to decrypt {}: {}", key, e))
        }
        // Not encrypted, return as-is
        else {
            Ok(value.to_string())
        }
    }
    pub async fn run(mut self) -> Result<()> {
        if self.complete {
            return self.complete().await;
        }
        match (&self.remove, &self.env_vars) {
            (None, None) => {
                return self.list_all().await;
            }
            (None, Some(env_vars))
                if env_vars.iter().all(|ev| ev.value.is_none()) && !self.prompt =>
            {
                return self.get().await;
            }
            _ => {}
        }

        let filename = self.filename()?;
        let mut mise_toml = get_mise_toml(&filename)?;

        if let Some(env_names) = &self.remove {
            for name in env_names {
                mise_toml.remove_env(name)?;
            }
        }

        if let Some(env_vars) = &self.env_vars {
            if env_vars.len() == 1 && env_vars[0].value.is_none() && !self.prompt {
                let key = &env_vars[0].key;
                // Use Config's centralized env loading which handles decryption
                let full_config = Config::get().await?;
                let env = full_config.env().await?;
                match env.get(key) {
                    Some(value) => {
                        miseprintln!("{value}");
                    }
                    None => bail!("Environment variable {key} not found"),
                }
                return Ok(());
            }
        }

        if let Some(mut env_vars) = self.env_vars.take() {
            // Prompt for values if requested
            if self.prompt {
                for ev in &mut env_vars {
                    if ev.value.is_none() {
                        let prompt_msg = format!("Enter value for {}", ev.key);
                        let value = Input::new(&prompt_msg)
                            .password(self.age_encrypt) // Mask input if encrypting
                            .run()?;
                        ev.value = Some(value);
                    }
                }
            }

            // Handle age encryption if requested
            if self.age_encrypt {
                Settings::get().ensure_experimental("age encryption")?;
                // Collect recipients once before the loop to avoid repeated I/O
                let recipients = self.collect_age_recipients().await?;
                for ev in env_vars {
                    match ev.value {
                        Some(value) => {
                            let age_directive =
                                agecrypt::create_age_directive(ev.key.clone(), &value, &recipients)
                                    .await?;
                            if let crate::config::env_directive::EnvDirective::Age {
                                value: encrypted_value,
                                format,
                                ..
                            } = age_directive
                            {
                                mise_toml.update_env_age(&ev.key, &encrypted_value, format)?;
                            }
                        }
                        None => bail!("{} has no value", ev.key),
                    }
                }
            } else {
                for ev in env_vars {
                    match ev.value {
                        Some(value) => mise_toml.update_env(&ev.key, value)?,
                        None => bail!("{} has no value", ev.key),
                    }
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
        // Determine config file path before moving env_vars
        let config_path = if let Some(file) = &self.file {
            Some(file.clone())
        } else if self.env.is_some() {
            Some(self.filename()?)
        } else if !self.global {
            // Check for local config file when no specific file or environment is specified
            // Check for mise.toml in current directory first
            let cwd = env::current_dir()?;
            let mise_toml = cwd.join("mise.toml");
            if mise_toml.exists() {
                Some(mise_toml)
            } else {
                // Fall back to .mise.toml if mise.toml doesn't exist
                let dot_mise_toml = cwd.join(".mise.toml");
                if dot_mise_toml.exists() {
                    Some(dot_mise_toml)
                } else {
                    None // Fall back to global config if no local config exists
                }
            }
        } else {
            None
        };

        let filter = self.env_vars.unwrap();

        // Handle global config case first
        if config_path.is_none() {
            let config = Config::get().await?;
            let env_with_sources = config.env_with_sources().await?;
            // env_with_sources already contains decrypted values
            for eva in filter {
                if let Some((value, _source)) = env_with_sources.get(&eva.key) {
                    miseprintln!("{value}");
                } else {
                    bail!("Environment variable {} not found", eva.key);
                }
            }
            return Ok(());
        }

        // Get the config to access directives directly
        let config = MiseToml::from_file(&config_path.unwrap()).unwrap_or_default();

        // For local configs, check directives directly
        let env_entries = config.env_entries()?;
        for eva in filter {
            match env_entries.iter().find_map(|ev| match ev {
                EnvDirective::Val(k, v, _) if k == &eva.key => Some((v.clone(), Some(ev))),
                EnvDirective::Age {
                    key: k, value: v, ..
                } if k == &eva.key => Some((v.clone(), Some(ev))),
                _ => None,
            }) {
                Some((value, directive)) => {
                    let decrypted =
                        Self::decrypt_value_if_needed(&eva.key, &value, directive).await?;
                    miseprintln!("{decrypted}");
                }
                None => bail!("Environment variable {} not found", eva.key),
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
                    EnvDirective::Age { key, value, .. } => Some(Row {
                        key,
                        value,
                        source: display_path(file),
                    }),
                    _ => None,
                })
                .collect()
        } else if self.env.is_some() {
            // When -E flag is used, read from the environment-specific file
            let filename = self.filename()?;
            let config = MiseToml::from_file(&filename).unwrap_or_default();
            config
                .env_entries()?
                .into_iter()
                .filter_map(|ed| match ed {
                    EnvDirective::Val(key, value, _) => Some(Row {
                        key,
                        value,
                        source: display_path(&filename),
                    }),
                    EnvDirective::Age { key, value, .. } => Some(Row {
                        key,
                        value,
                        source: display_path(&filename),
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

    fn filename(&self) -> Result<PathBuf> {
        let opts = ConfigPathOptions {
            global: self.global,
            path: self.file.clone(),
            env: self.env.clone(),
            cwd: None,                // Use current working directory
            prefer_toml: true,        // mise set only works with TOML files
            prevent_home_local: true, // When in HOME, use global config
        };
        resolve_target_config_path(opts)
    }

    async fn collect_age_recipients(&self) -> Result<Vec<Box<dyn age::Recipient + Send>>> {
        use age::Recipient;

        let mut recipients: Vec<Box<dyn Recipient + Send>> = Vec::new();

        // Add x25519 recipients from command line
        for recipient_str in &self.age_recipient {
            if let Some(recipient) = agecrypt::parse_recipient(recipient_str)? {
                recipients.push(recipient);
            }
        }

        // Add SSH recipients from command line
        for ssh_arg in &self.age_ssh_recipient {
            let path = Path::new(ssh_arg);
            if path.exists() {
                // It's a file path
                recipients.push(agecrypt::load_ssh_recipient_from_path(path).await?);
            } else {
                // Try to parse as a direct SSH public key
                if let Some(recipient) = agecrypt::parse_recipient(ssh_arg)? {
                    recipients.push(recipient);
                }
            }
        }

        // If no recipients were provided, use defaults
        if recipients.is_empty()
            && (self.age_recipient.is_empty()
                && self.age_ssh_recipient.is_empty()
                && self.age_key_file.is_none())
        {
            recipients = agecrypt::load_recipients_from_defaults().await?;
        }

        // Load recipients from key file if specified
        if let Some(key_file) = &self.age_key_file {
            let key_file_recipients = agecrypt::load_recipients_from_key_file(key_file).await?;
            recipients.extend(key_file_recipients);
        }

        if recipients.is_empty() {
            bail!(
                "[experimental] No age recipients provided. Use --age-recipient, --age-ssh-recipient, or --age-key-file"
            );
        }

        Ok(recipients)
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

    $ <bold>mise set -E staging NODE_ENV=staging</bold>
    # creates or modifies mise.staging.toml

    $ <bold>mise set</bold>
    key       value       source
    NODE_ENV  production  ~/.config/mise/config.toml

    $ <bold>mise set --prompt PASSWORD</bold>
    Enter value for PASSWORD: [hidden input]

    <bold><underline>[experimental] Age Encryption:</underline></bold>

    $ <bold>mise set --age-encrypt API_KEY=secret</bold>

    $ <bold>mise set --age-encrypt --prompt API_KEY</bold>
    Enter value for API_KEY: [hidden input]
"#
);
