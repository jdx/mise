use std::sync::Arc;

use atty::Stream::Stderr;
use color_eyre::eyre::Result;
use owo_colors::Stream;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::config::MissingRuntimeBehavior::AutoInstall;
use crate::config::Settings;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;
use crate::plugins::InstallType::Version;
use crate::plugins::{Plugin, PluginName};
use crate::runtimes::RuntimeVersion;
use crate::ui::color::cyan;

/// install a runtime
///
/// this will install a runtime to `~/.local/share/rtx/installs/<PLUGIN>/<VERSION>`
/// it won't be used simply by being installed, however.
/// For that, you must set up a `.tool-version` file manually or with `rtx local/global`.
/// Or you can call a runtime explicitly with `rtx exec <PLUGIN>@<VERSION> -- <COMMAND>`.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Install {
    /// runtime(s) to install
    ///
    /// e.g.: nodejs@20
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Option<Vec<RuntimeArg>>,

    /// only install runtime(s) for <PLUGIN>
    #[clap(long, short, conflicts_with = "runtime")]
    plugin: Option<Vec<PluginName>>,

    /// force reinstall even if already installed
    #[clap(long, short, requires = "runtime")]
    force: bool,

    /// install all missing runtimes as well as all plugins for the current directory
    #[clap(long, short, conflicts_with_all = ["runtime", "plugin", "force"])]
    all: bool,

    /// Show installation output
    #[clap(long, short, action = clap::ArgAction::Count)]
    verbose: u8,
}

impl Command for Install {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match &self.runtime {
            Some(runtime) => self.install_runtimes(config, out, runtime)?,
            None => self.install_missing_runtimes(config, out)?,
        }

        Ok(())
    }
}

impl Install {
    fn install_runtimes(
        &self,
        config: Config,
        out: &mut Output,
        runtimes: &[RuntimeArg],
    ) -> Result<()> {
        let settings = Settings {
            missing_runtime_behavior: AutoInstall,
            ..config.settings.clone()
        };

        for r in RuntimeArg::double_runtime_condition(runtimes) {
            if r.version == "system" {
                continue;
            }
            let plugin = Plugin::load_ensure_installed(&r.plugin, &settings)?;

            let version = config.resolve_alias(&r.plugin, r.version.clone());
            let version = plugin.latest_version(&version)?.unwrap_or(version);

            let rtv = RuntimeVersion::new(Arc::new(plugin), &version);

            if rtv.is_installed() && self.force {
                rtv.uninstall()?;
            } else if rtv.is_installed() {
                warn!(
                    "{} is already installed",
                    cyan(Stream::Stderr, &rtv.to_string())
                );
                continue;
            }

            self.do_install(&config, out, &rtv)?;
        }

        Ok(())
    }

    fn install_missing_runtimes(&self, mut config: Config, out: &mut Output) -> Result<()> {
        for rtv in config.ts.list_current_versions() {
            let plugins = match self.all {
                true => Some(get_all_plugin_names(&config)),
                false => self.plugin.clone(),
            };
            if let Some(plugins) = &plugins {
                // they've specified --all or --plugin, so we already know they want to install
                config.settings.missing_runtime_behavior = AutoInstall;
                if !plugins.contains(&rtv.plugin.name) {
                    continue;
                }
                // ensure plugin is installed only if explicitly called with --plugin or using --all
                if !rtv.plugin.ensure_installed(&config.settings)? {
                    Err(PluginNotInstalled(rtv.plugin.name.to_string()))?;
                }
            }

            if !rtv.plugin.is_installed() {
                warn_plugin_not_installed(&rtv.plugin);
                continue;
            }
            if rtv.version == "system" || rtv.is_installed() {
                continue;
            }
            let version = rtv
                .plugin
                .latest_version(&rtv.version)?
                .unwrap_or_else(|| rtv.version.clone());
            // need to re-create the rtv because the version may have changed
            let rtv = RuntimeVersion::new(rtv.plugin.clone(), &version);
            self.do_install(&config, out, &rtv)?;
        }
        Ok(())
    }

    fn do_install(&self, config: &Config, _out: &mut Output, rtv: &RuntimeVersion) -> Result<()> {
        rtv.install(Version, config)?;
        Ok(())
    }
}

fn warn_plugin_not_installed(plugin: &Plugin) {
    warn!(
        "plugin {} is not installed. Install it with `rtx plugin add {}`",
        cyan(Stderr, &plugin.name),
        plugin.name,
    );
}

fn get_all_plugin_names(config: &Config) -> Vec<String> {
    config
        .ts
        .list_plugins()
        .into_iter()
        .map(|p| p.name.clone())
        .collect()
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  $ rtx install nodejs@18.0.0  # install specific nodejs version
  $ rtx install nodejs@18      # install fuzzy nodejs version
  $ rtx install nodejs         # install latest nodejs version—or what is specified in .tool-versions
  $ rtx install                # installs all runtimes specified in .tool-versions for installed plugins
  $ rtx install --all          # installs all runtimes and all plugins
"#;

#[cfg(test)]
mod test {
    use crate::assert_cli;

    #[test]
    fn test_install_force() {
        assert_cli!("install", "-f", "shfmt");
    }

    #[test]
    fn test_install_asdf_style() {
        assert_cli!("install", "shfmt", "2");
    }

    #[test]
    fn test_install_with_alias() {
        assert_cli!("install", "-f", "shfmt@my/alias");
        assert_cli!("where", "shfmt@my/alias");
    }
}
