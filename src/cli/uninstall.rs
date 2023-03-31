use color_eyre::eyre::{eyre, Result};
use console::style;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;

/// Removes runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP)]
pub struct Uninstall {
    /// Runtime(s) to remove
    #[clap(required = true, value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,
}

impl Command for Uninstall {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        let runtimes = RuntimeArg::double_runtime_condition(&self.runtime);
        let ts = ToolsetBuilder::new()
            .with_args(&runtimes)
            .build(&mut config)?;
        let runtime_versions = runtimes.iter().filter_map(|a| ts.resolve_runtime_arg(a));

        let mpr = MultiProgressReport::new(config.settings.verbose);
        for rtv in runtime_versions {
            if !rtv.is_installed() {
                warn!("{} is not installed", style(rtv).cyan().for_stderr());
                continue;
            }

            let mut pr = mpr.add();
            rtv.decorate_progress_bar(&mut pr);
            if let Err(err) = rtv.uninstall(&config.settings, &pr, false) {
                pr.error();
                return Err(eyre!(err).wrap_err(format!("failed to uninstall {}", rtv)));
            }
            pr.finish_with_message("uninstalled".into());
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx uninstall nodejs@18.0.0</bold> # will uninstall specific version
  $ <bold>rtx uninstall nodejs</bold>        # will uninstall current nodejs version
"#
);
