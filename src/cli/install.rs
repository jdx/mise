use std::collections::HashSet;
use std::sync::Arc;

use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser, RuntimeArgVersion};
use crate::cli::command::Command;
use crate::config::Config;
use crate::config::MissingRuntimeBehavior::AutoInstall;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;
use crate::plugins::{Plugin, PluginName};
use crate::shims::reshim;
use crate::toolset::{ToolVersion, ToolVersionType, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;

/// Install a runtime
///
/// This will install a runtime to `~/.local/share/rtx/installs/<PLUGIN>/<VERSION>`
/// It won't be used simply by being installed, however.
/// For that, you must set up a `.tool-version` file manually or with `rtx local/global`.
/// Or you can call a runtime explicitly with `rtx exec <PLUGIN>@<VERSION> -- <COMMAND>`.
///
/// Runtimes will be installed in parallel. To disable, set `--jobs=1` or `RTX_JOBS=1`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Install {
    /// Runtime(s) to install
    /// e.g.: nodejs@18
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Option<Vec<RuntimeArg>>,

    /// Only install runtime(s) for <PLUGIN>
    #[clap(long, short, conflicts_with = "runtime")]
    plugin: Option<Vec<PluginName>>,

    /// Force reinstall even if already installed
    #[clap(long, short, requires = "runtime")]
    force: bool,

    /// Install all missing runtimes as well as all plugins for the current directory
    /// This is hidden because it's now the default behavior
    #[clap(long, short, conflicts_with_all = ["runtime", "plugin", "force"], hide = true)]
    all: bool,

    /// Show installation output
    #[clap(long, short, action = clap::ArgAction::Count)]
    verbose: u8,
}

impl Command for Install {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        config.settings.missing_runtime_behavior = AutoInstall;

        match &self.runtime {
            Some(runtime) => self.install_runtimes(config, runtime)?,
            None => self.install_missing_runtimes(config)?,
        }

        Ok(())
    }
}

impl Install {
    fn install_runtimes(&self, mut config: Config, runtimes: &[RuntimeArg]) -> Result<()> {
        let mpr = MultiProgressReport::new(config.settings.verbose);
        let mut tool_versions = vec![];
        let ts = ToolsetBuilder::new().build(&mut config)?;
        for runtime in RuntimeArg::double_runtime_condition(runtimes) {
            match runtime.to_tool_version() {
                Some(tv) => tool_versions.push(tv),
                None => {
                    if runtime.version == RuntimeArgVersion::None {
                        match ts.versions.get(&runtime.plugin) {
                            Some(tvl) => {
                                for tv in &tvl.versions {
                                    tool_versions.push(tv.clone());
                                }
                            }
                            None => {
                                let tv = ToolVersion::new(
                                    runtime.plugin.clone(),
                                    ToolVersionType::Version("latest".into()),
                                );
                                tool_versions.push(tv);
                            }
                        }
                    }
                }
            }
        }
        ThreadPoolBuilder::new()
            .num_threads(config.settings.jobs)
            .build()?
            .install(|| -> Result<()> {
                let mut versions = vec![];
                for mut tv in tool_versions {
                    let plugin = match config.plugins.get(&tv.plugin_name).cloned() {
                        Some(plugin) => plugin,
                        None => {
                            let plugin = Plugin::new(&tv.plugin_name);
                            let mut pr = mpr.add();
                            match plugin.install(&config, &mut pr, false) {
                                Ok(_) => Arc::new(plugin),
                                Err(err) => {
                                    pr.error();
                                    return Err(err)?;
                                }
                            }
                        }
                    };
                    tv.resolve(&config, plugin)?;
                    versions.push(tv.rtv.unwrap());
                }
                if versions.is_empty() {
                    warn!("no runtimes to install");
                    warn!("specify a version with `rtx install <PLUGIN>@<VERSION>`");
                    return Ok(());
                }
                let mut to_uninstall = vec![];
                for rtv in &versions {
                    if rtv.is_installed() {
                        if self.force {
                            to_uninstall.push(rtv.clone());
                        } else {
                            warn!("{} already installed", style(rtv).cyan().for_stderr());
                        }
                    }
                }
                if !to_uninstall.is_empty() {
                    to_uninstall
                        .into_par_iter()
                        .map(|rtv| {
                            let mut pr = mpr.add();
                            rtv.decorate_progress_bar(&mut pr);
                            match rtv.uninstall(&config.settings, &pr, false) {
                                Ok(_) => {
                                    pr.finish();
                                    Ok(())
                                }
                                Err(err) => {
                                    pr.error();
                                    Err(err.wrap_err(format!("failed to uninstall {}", rtv)))
                                }
                            }
                        })
                        .collect::<Result<Vec<_>>>()?;
                }
                versions
                    .into_par_iter()
                    .map(|rtv| {
                        if rtv.is_installed() {
                            return Ok(());
                        }
                        let mut pr = mpr.add();
                        rtv.decorate_progress_bar(&mut pr);
                        match rtv.install(&config, &mut pr, self.force) {
                            Ok(_) => Ok(()),
                            Err(err) => {
                                pr.error();
                                Err(err.wrap_err(format!("failed to install {}", rtv)))
                            }
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                reshim(&mut config, &ts)
            })
    }

    fn install_missing_runtimes(&self, mut config: Config) -> Result<()> {
        let mut ts = ToolsetBuilder::new().build(&mut config)?;
        if let Some(plugins) = &self.plugin {
            let plugins = plugins.iter().collect::<HashSet<&PluginName>>();
            for plugin in ts.versions.keys().cloned().collect::<Vec<_>>() {
                if !plugins.contains(&plugin) {
                    ts.versions.remove(&plugin);
                }
            }
            for plugin in plugins {
                if !ts.versions.contains_key(plugin) {
                    Err(PluginNotInstalled(plugin.to_string()))?;
                }
            }
        }
        if ts.list_missing_versions().is_empty() {
            warn!("no runtimes to install");
        }
        let mpr = MultiProgressReport::new(config.settings.verbose);
        ts.install_missing(&mut config, mpr)?;

        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx install nodejs@18.0.0  # install specific nodejs version
      $ rtx install nodejs@18      # install fuzzy nodejs version
      $ rtx install nodejs         # install version specified in .tool-versions
      $ rtx install                # installs all runtimes specified in .tool-versions for installed plugins
      $ rtx install --all          # installs all runtimes and all plugins
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, assert_cli_snapshot, dirs};

    #[test]
    fn test_install_force() {
        assert_cli!("install", "-f", "tiny");
    }

    #[test]
    fn test_install_asdf_style() {
        assert_cli!("install", "tiny", "2");
    }

    #[test]
    fn test_install_with_alias() {
        assert_cli!("install", "-f", "tiny@my/alias");
        assert_cli_snapshot!("where", "tiny@my/alias");
    }

    #[test]
    fn test_install_ref() {
        assert_cli!("install", "-f", "dummy@ref:master");
        assert_cli!("global", "dummy@ref:master");
        let output = assert_cli!("where", "dummy");
        assert_str_eq!(
            output.trim(),
            dirs::INSTALLS.join("dummy/ref-master").to_string_lossy()
        );
        assert_cli!("global", "--unset", "dummy");
    }

    #[test]
    fn test_install_nothing() {
        // this doesn't do anything since dummy isn't specified
        assert_cli_snapshot!("install", "dummy");
    }
}
