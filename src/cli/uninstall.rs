use color_eyre::eyre::{eyre, Result, WrapErr};
use owo_colors::OwoColorize;
use owo_colors::Stream;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::runtimes::RuntimeVersion;

/// removes a runtime version
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP)]
pub struct Uninstall {
    /// runtime(s) to remove
    #[clap(required = true, value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,
}

impl Command for Uninstall {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let runtime_versions = self
            .runtime
            .iter()
            .map(|a| {
                let prefix = match a.version.as_str() {
                    "latest" => "",
                    v => v,
                };
                RuntimeVersion::find_by_version_prefix(&a.plugin, prefix)
            })
            .collect::<Result<Vec<RuntimeVersion>>>()?;

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
  $ rtx uninstall nodejs
"#;
