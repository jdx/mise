use color_eyre::eyre::{eyre, Result};
use console::style;
use std::fs;
use std::path::PathBuf;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::env;
use crate::output::Output;
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{runtime_symlinks, shims};

/// Removes runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP)]
pub struct Uninstall {
    /// Tool(s) to remove
    #[clap(required = true, value_name="TOOL@VERSION", value_parser = ToolArgParser)]
    tool: Vec<ToolArg>,

    /// Remove all tools for the runtime
    #[clap(long)]
    all: bool,
}

impl Command for Uninstall {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        let runtimes = ToolArg::double_tool_condition(&self.tool);
        if self.all {
            for runtime in &runtimes {
                let tool_path: PathBuf =
                    env::RTX_DATA_DIR.join("installs").join(runtime.to_string());
                if fs::metadata(&tool_path).is_ok() {
                    match fs::remove_dir_all(&tool_path) {
                        Ok(()) => {}
                        Err(err) => eprintln!("Error while removing {}: {}", runtime, err),
                    }
                }
                info!("removed all {} verions", runtime);
            }
            return Ok(());
        }

        let tool_versions = runtimes
            .iter()
            .map(|a| {
                let tool = config.get_or_create_tool(&a.plugin);
                let tv = match &a.tvr {
                    Some(tvr) => tvr.resolve(&config, &tool, Default::default(), false)?,
                    None => {
                        let ts = ToolsetBuilder::new().build(&mut config)?;
                        let tv = ts
                            .versions
                            .get(&a.plugin)
                            .and_then(|v| v.versions.first())
                            .expect("no version found");
                        tv.clone()
                    }
                };
                Ok((tool, tv))
            })
            .collect::<Result<Vec<_>>>()?;

        let mpr = MultiProgressReport::new(config.show_progress_bars());
        for (plugin, tv) in tool_versions {
            if !plugin.is_version_installed(&tv) {
                warn!("{} is not installed", style(&tv).cyan().for_stderr());
                continue;
            }

            let mut pr = mpr.add();
            plugin.decorate_progress_bar(&mut pr, Some(&tv));
            if let Err(err) = plugin.uninstall_version(&config, &tv, &pr, false) {
                pr.error(err.to_string());
                return Err(eyre!(err).wrap_err(format!("failed to uninstall {}", &tv)));
            }
            pr.finish_with_message("uninstalled");
        }

        let ts = ToolsetBuilder::new().build(&mut config)?;
        shims::reshim(&mut config, &ts).map_err(|err| eyre!("failed to reshim: {}", err))?;
        runtime_symlinks::rebuild(&config)?;

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx uninstall node@18.0.0</bold> # will uninstall specific version
  $ <bold>rtx uninstall node</bold>        # will uninstall current node version
"#
);
