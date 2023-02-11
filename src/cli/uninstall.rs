use color_eyre::eyre::{eyre, Result, WrapErr};
use indoc::formatdoc;
use itertools::Itertools;
use once_cell::sync::Lazy;
use owo_colors::OwoColorize;
use owo_colors::Stream;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser, RuntimeArgVersion};
use crate::cli::command::Command;
use crate::config::Config;
use crate::errors::Error::VersionNotInstalled;
use crate::output::Output;
use crate::ui::color::Color;

/// removes runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Uninstall {
    /// runtime(s) to remove
    #[clap(required = true, value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,
}

impl Command for Uninstall {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let runtime_versions = self
            .runtime
            .iter()
            .map(|a| {
                let mut versions = config.ts.list_current_versions();
                versions.extend(config.ts.list_installed_versions());

                let prefix = match &a.version {
                    RuntimeArgVersion::None => config.resolve_runtime_arg(a)?.unwrap(),
                    RuntimeArgVersion::Version(version) => {
                        config.resolve_alias(&a.plugin, version.to_string())
                    }
                    _ => Err(eyre!("invalid version {}", a.to_string()))?,
                };
                let mut versions = config.ts.list_current_versions();
                versions.extend(config.ts.list_installed_versions());
                let versions = versions
                    .into_iter()
                    .filter(|rtv| rtv.plugin.name == a.plugin)
                    .filter(|rtv| rtv.version.starts_with(&prefix))
                    .unique_by(|rtv| rtv.version.clone())
                    .collect::<Vec<_>>();

                if versions.is_empty() {
                    Err(VersionNotInstalled(a.plugin.clone(), a.version.to_string()))?
                } else {
                    // TODO: add a flag to uninstall all matching versions
                    Ok(vec![versions[0].clone()])
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

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stdout));
static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx uninstall nodejs@18 # will uninstall ALL nodejs-18.x versions
      $ rtx uninstall nodejs    # will uninstall ALL nodejs versions
    "#, COLOR.header("Examples:")}
});
