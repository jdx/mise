use color_eyre::eyre::{eyre, Result, WrapErr};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::toolset::ToolsetBuilder;

/// Removes runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Uninstall {
    /// Runtime(s) to remove
    #[clap(required = true, value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,
}

impl Command for Uninstall {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let runtimes = RuntimeArg::double_runtime_condition(&self.runtime);
        let ts = ToolsetBuilder::new().with_args(&runtimes).build(&config);
        let runtime_versions = runtimes.iter().filter_map(|a| ts.resolve_runtime_arg(a));

        for rtv in runtime_versions {
            if !rtv.is_installed() {
                warn!("{} is not installed", style(rtv).cyan().for_stderr());
                continue;
            }

            rtxprintln!(out, "uninstalling {}", style(rtv).cyan());
            rtv.uninstall()
                .wrap_err_with(|| eyre!("error uninstalling {}", rtv))?;
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx uninstall nodejs@18.0.0 # will uninstall specific version
      $ rtx uninstall nodejs        # will uninstall current nodejs version
    "#, style("Examples:").underlined().bold()}
});
