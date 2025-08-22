use eyre::Result;
use std::{collections::BTreeMap, sync::Arc};

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::shell::{ShellType, get_shell};
use crate::toolset::{InstallOptions, Toolset, ToolsetBuilder};
use indexmap::IndexSet;

/// Exports env vars to activate mise a single time
///
/// Use this if you don't want to permanently install mise. It's not necessary to
/// use this if you have `mise activate` in your shell rc file.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "e", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Env {
    /// Tool(s) to use
    #[clap(value_name = "TOOL@VERSION")]
    tool: Vec<ToolArg>,

    /// Output in JSON format
    #[clap(long, short = 'J', overrides_with = "shell")]
    json: bool,

    /// Output in JSON format with additional information (source, tool)
    #[clap(long, overrides_with = "shell")]
    json_extended: bool,

    /// Output in dotenv format
    #[clap(long, short = 'D', overrides_with = "shell")]
    dotenv: bool,

    /// Only show redacted environment variables
    #[clap(long)]
    redacted: bool,

    /// Shell type to generate environment variables for
    #[clap(long, short, overrides_with = "json")]
    shell: Option<ShellType>,

    /// Only show values of environment variables
    #[clap(long)]
    values: bool,
}

impl Env {
    pub async fn run(self) -> Result<()> {
        let mut config = Config::get().await?;
        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .build(&config)
            .await?;
        ts.install_missing_versions(&mut config, &InstallOptions::default())
            .await?;
        ts.notify_if_versions_missing(&config).await;

        // Get redacted keys if needed
        let redacted_keys = if self.redacted {
            let env_results = config.env_results().await?;
            let mut keys = IndexSet::new();
            keys.extend(env_results.redactions.clone());
            keys.extend(config.redaction_keys());
            Some(keys)
        } else {
            None
        };

        if self.json {
            self.output_json(&config, ts, &redacted_keys).await
        } else if self.json_extended {
            self.output_extended_json(&config, ts, &redacted_keys).await
        } else if self.dotenv {
            self.output_dotenv(&config, ts, &redacted_keys).await
        } else if self.values {
            self.output_values(&config, ts, &redacted_keys).await
        } else {
            self.output_shell(&config, ts, &redacted_keys).await
        }
    }

    async fn output_json(
        &self,
        config: &Arc<Config>,
        ts: Toolset,
        redacted_keys: &Option<IndexSet<String>>,
    ) -> Result<()> {
        let mut env = ts.env_with_path(config).await?;

        if let Some(keys) = redacted_keys {
            env.retain(|k, _| self.should_include_key(k, keys));
        }

        miseprintln!("{}", serde_json::to_string_pretty(&env)?);
        Ok(())
    }

    async fn output_extended_json(
        &self,
        config: &Arc<Config>,
        ts: Toolset,
        redacted_keys: &Option<IndexSet<String>>,
    ) -> Result<()> {
        let mut res = BTreeMap::new();

        ts.env_with_path(config).await?.iter().for_each(|(k, v)| {
            res.insert(k.to_string(), BTreeMap::from([("value", v.to_string())]));
        });

        config.env_with_sources().await?.iter().for_each(|(k, v)| {
            res.insert(
                k.to_string(),
                BTreeMap::from([
                    ("value", v.0.to_string()),
                    ("source", v.1.to_string_lossy().into_owned()),
                ]),
            );
        });

        let tool_map: BTreeMap<String, String> = ts
            .list_all_versions(config)
            .await?
            .into_iter()
            .map(|(b, tv)| {
                (
                    b.id().into(),
                    tv.request
                        .source()
                        .path()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "".to_string()),
                )
            })
            .collect();

        ts.env_from_tools(config)
            .await
            .iter()
            .for_each(|(name, value, tool_id)| {
                res.insert(
                    name.to_string(),
                    BTreeMap::from([
                        ("value", value.to_string()),
                        ("tool", tool_id.to_string()),
                        (
                            "source",
                            tool_map
                                .get(tool_id)
                                .cloned()
                                .unwrap_or_else(|| "unknown_source".to_string()),
                        ),
                    ]),
                );
            });

        if let Some(keys) = redacted_keys {
            res.retain(|k, _| self.should_include_key(k, keys));
        }

        miseprintln!("{}", serde_json::to_string_pretty(&res)?);
        Ok(())
    }

    async fn output_shell(
        &self,
        config: &Arc<Config>,
        ts: Toolset,
        redacted_keys: &Option<IndexSet<String>>,
    ) -> Result<()> {
        let default_shell = get_shell(Some(ShellType::Bash)).unwrap();
        let shell = get_shell(self.shell).unwrap_or(default_shell);
        let mut env = ts.env_with_path(config).await?;

        if let Some(keys) = redacted_keys {
            env.retain(|k, _| self.should_include_key(k, keys));
        }

        for (k, v) in env {
            let k = k.to_string();
            let v = v.to_string();
            miseprint!("{}", shell.set_env(&k, &v))?;
        }
        Ok(())
    }

    async fn output_dotenv(
        &self,
        config: &Arc<Config>,
        ts: Toolset,
        redacted_keys: &Option<IndexSet<String>>,
    ) -> Result<()> {
        let (mut env, _) = ts.final_env(config).await?;

        if let Some(keys) = redacted_keys {
            env.retain(|k, _| self.should_include_key(k, keys));
        }

        for (k, v) in env {
            let k = k.to_string();
            let v = v.to_string();
            miseprint!("{}={}\n", k, v)?;
        }
        Ok(())
    }

    async fn output_values(
        &self,
        config: &Arc<Config>,
        ts: Toolset,
        redacted_keys: &Option<IndexSet<String>>,
    ) -> Result<()> {
        let mut env = ts.env_with_path(config).await?;

        if let Some(keys) = redacted_keys {
            env.retain(|k, _| self.should_include_key(k, keys));
        }

        for (_, v) in env {
            miseprintln!("{}", v);
        }
        Ok(())
    }

    fn should_include_key(&self, key: &str, redacted_keys: &IndexSet<String>) -> bool {
        // Check if key matches any redaction pattern (supporting wildcards)
        redacted_keys.iter().any(|pattern| {
            if pattern.contains('*') {
                // Handle wildcard patterns
                if pattern == "*" {
                    true
                } else if let Some(prefix) = pattern.strip_suffix('*') {
                    key.starts_with(prefix)
                } else if let Some(suffix) = pattern.strip_prefix('*') {
                    key.ends_with(suffix)
                } else {
                    // Pattern has * in the middle, not supported yet
                    false
                }
            } else {
                key == pattern
            }
        })
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>eval "$(mise env -s bash)"</bold>
    $ <bold>eval "$(mise env -s zsh)"</bold>
    $ <bold>mise env -s fish | source</bold>
    $ <bold>execx($(mise env -s xonsh))</bold>
"#
);
