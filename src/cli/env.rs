use eyre::Result;
use std::collections::BTreeMap;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::shell::{get_shell, ShellType};
use crate::toolset::{InstallOptions, Toolset, ToolsetBuilder};

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

    /// Shell type to generate environment variables for
    #[clap(long, short, overrides_with = "json")]
    shell: Option<ShellType>,
}

impl Env {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(&config)?;
        ts.install_missing_versions(&InstallOptions::default())?;
        ts.notify_if_versions_missing();

        if self.json {
            self.output_json(&config, ts)
        } else if self.json_extended {
            self.output_extended_json(&config, ts)
        } else if self.dotenv {
            self.output_dotenv(&config, ts)
        } else {
            self.output_shell(&config, ts)
        }
    }

    fn output_json(&self, config: &Config, ts: Toolset) -> Result<()> {
        let env = ts.env_with_path(config)?;
        miseprintln!("{}", serde_json::to_string_pretty(&env)?);
        Ok(())
    }

    fn output_extended_json(&self, config: &Config, ts: Toolset) -> Result<()> {
        let mut res = BTreeMap::new();

        ts.env_with_path(config)?.iter().for_each(|(k, v)| {
            res.insert(k.to_string(), BTreeMap::from([("value", v.to_string())]));
        });

        config.env_with_sources()?.iter().for_each(|(k, v)| {
            res.insert(
                k.to_string(),
                BTreeMap::from([
                    ("value", v.0.to_string()),
                    ("source", v.1.to_string_lossy().into_owned()),
                ]),
            );
        });

        let tool_map: BTreeMap<String, String> = ts
            .list_all_versions()?
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

        miseprintln!("{}", serde_json::to_string_pretty(&res)?);
        Ok(())
    }

    fn output_shell(&self, config: &Config, ts: Toolset) -> Result<()> {
        let default_shell = get_shell(Some(ShellType::Bash)).unwrap();
        let shell = get_shell(self.shell).unwrap_or(default_shell);
        for (k, v) in ts.env_with_path(config)? {
            let k = k.to_string();
            let v = v.to_string();
            miseprint!("{}", shell.set_env(&k, &v))?;
        }
        Ok(())
    }

    fn output_dotenv(&self, config: &Config, ts: Toolset) -> Result<()> {
        for (k, v) in ts.env(config)? {
            let k = k.to_string();
            let v = v.to_string();
            miseprint!("{}={}\n", k, v)?;
        }
        Ok(())
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
