use color_eyre::eyre::{eyre, Result, WrapErr};
use itertools::Itertools;
use owo_colors::OwoColorize;
use owo_colors::Stream;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::errors::Error::VersionNotInstalled;
use crate::output::Output;

/// removes runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP)]
pub struct Uninstall {
    /// runtime(s) to remove
    #[clap(required = true, value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,
}

impl Command for Uninstall {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let runtime_versions = self
            .runtime
            .iter()
            .map(|a| {
                let prefix = match a.version.as_str() {
                    "latest" => "",
                    v => v,
                };
                let prefix = config.resolve_alias(&a.plugin, prefix.to_string());
                let mut versions = config.ts.list_current_versions();
                versions.extend(config.ts.list_installed_versions());
                let versions = versions
                    .into_iter()
                    .filter(|rtv| rtv.plugin.name == a.plugin)
                    .filter(|rtv| rtv.version.starts_with(&prefix))
                    .unique_by(|rtv| rtv.version.clone())
                    .collect::<Vec<_>>();

                if versions.is_empty() {
                    Err(VersionNotInstalled(a.plugin.clone(), a.version.clone()))?
                } else {
                    Ok(versions)
                }
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect_vec();

        for rtv in runtime_versions {
            if !rtv.is_installed() {
                warn!(
                    "{} is not installed",
                    rtv.to_string()
                        .if_supports_color(Stream::Stderr, |t| t.cyan())
                );
                continue;
            }

            rtxprintln!(out, "uninstalling {}", rtv.to_string().cyan());
            rtv.uninstall()
                .wrap_err_with(|| eyre!("error uninstalling {}", rtv))?;
        }
        Ok(())
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  $ rtx uninstall nodejs@18 # will uninstall ALL nodejs-18.x versions
  $ rtx uninstall nodejs    # will uninstall ALL nodejs versions
"#;
