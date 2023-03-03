use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use url::Url;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::{Plugin, PluginName};
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;

/// Install a plugin
///
/// note that rtx automatically can install plugins when you install a runtime
/// e.g.: `rtx install nodejs@18` will autoinstall the nodejs plugin
///
/// This behavior can be modified in ~/.rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["i", "a"], alias = "add", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct PluginsInstall {
    /// The name of the plugin to install
    /// e.g.: nodejs, ruby
    /// Can specify multiple plugins: `rtx plugins install nodejs ruby python`
    #[clap(required_unless_present = "all", verbatim_doc_comment)]
    name: Option<String>,

    /// The git url of the plugin
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(help = "The git url of the plugin", value_hint = clap::ValueHint::Url, verbatim_doc_comment)]
    git_url: Option<String>,

    /// Reinstall even if plugin exists
    #[clap(short, long, verbatim_doc_comment)]
    force: bool,

    /// Install all missing plugins
    /// This will only install plugins that have matching shorthands.
    /// i.e.: they don't need the full git repo url
    #[clap(short, long, conflicts_with_all = ["name", "force"], verbatim_doc_comment)]
    all: bool,

    /// Show installation output
    #[clap(long, short, action = clap::ArgAction::Count, verbatim_doc_comment)]
    verbose: u8,

    #[clap(hide = true)]
    rest: Vec<String>,
}

impl Command for PluginsInstall {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        let mpr = MultiProgressReport::new(config.settings.verbose);
        if self.all {
            return self.install_all_missing_plugins(&mut config, mpr);
        }
        let (name, git_url) =
            get_name_and_url(&config, &self.name.clone().unwrap(), &self.git_url)?;
        if git_url.contains("://") {
            self.install_one(&config, &name, &git_url, &mpr)?;
        } else {
            let mut plugins: Vec<PluginName> = vec![name, git_url];
            plugins.extend(self.rest.clone());
            self.install_many(&mut config, &plugins, mpr)?;
        }

        Ok(())
    }
}

impl PluginsInstall {
    fn install_all_missing_plugins(
        &self,
        config: &mut Config,
        mpr: MultiProgressReport,
    ) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config);
        let missing_plugins = ts.list_missing_plugins(config);
        if missing_plugins.is_empty() {
            warn!("all plugins already installed");
        }
        self.install_many(config, &missing_plugins, mpr)?;
        Ok(())
    }

    fn install_many(
        &self,
        config: &mut Config,
        plugins: &[PluginName],
        mpr: MultiProgressReport,
    ) -> Result<()> {
        ThreadPoolBuilder::new()
            .num_threads(config.settings.jobs)
            .build()?
            .install(|| -> Result<()> {
                plugins
                    .into_par_iter()
                    .map(|plugin| {
                        let (_, git_url) = get_name_and_url(config, plugin, &None)?;
                        self.install_one(config, plugin, &git_url, &mpr)
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(())
            })
    }

    fn install_one(
        &self,
        config: &Config,
        name: &String,
        git_url: &String,
        mpr: &MultiProgressReport,
    ) -> Result<()> {
        let plugin = Plugin::new(name);
        if !self.force && plugin.is_installed() {
            mpr.suspend(|| {
                warn!("plugin {} already installed", name);
            });
        } else {
            let mut pr = mpr.add();
            plugin.decorate_progress_bar(&mut pr);
            if self.force {
                plugin.uninstall(&pr)?;
            }
            plugin.install(config, Some(git_url), &mut pr)?;
        }
        Ok(())
    }
}

fn get_name_and_url(
    config: &Config,
    name: &String,
    git_url: &Option<String>,
) -> Result<(String, String)> {
    Ok(match git_url {
        Some(url) => (name.clone(), url.clone()),
        None => match name.contains(':') {
            true => (get_name_from_url(name)?, name.clone()),
            false => {
                let git_url = config
                    .get_shorthands()
                    .get(name)
                    .ok_or_else(|| eyre!("could not find plugin {}", name))?;
                (name.clone(), git_url.to_string())
            }
        },
    })
}

fn get_name_from_url(url: &str) -> Result<String> {
    if let Ok(url) = Url::parse(url) {
        if let Some(segments) = url.path_segments() {
            let last = segments.last().unwrap_or_default();
            let name = last.strip_prefix("asdf-").unwrap_or(last);
            let name = name.strip_prefix("rtx-").unwrap_or(name);
            let name = name.strip_suffix(".git").unwrap_or(name);
            return Ok(name.to_string());
        }
    }
    Err(eyre!("could not infer plugin name from url: {}", url))
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      # install the nodejs via shorthand
      $ rtx install nodejs

      # install the nodejs plugin using a specific git url
      $ rtx install nodejs https://github.com/asdf-vm/asdf-nodejs.git

      # install the nodejs plugin using the git url only
      # (nodejs is inferred from the url)
      $ rtx install https://github.com/asdf-vm/asdf-nodejs.git
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use insta::assert_display_snapshot;

    use crate::cli::tests::cli_run;

    #[test]
    fn test_plugin_install_invalid_url() {
        let args = ["rtx", "plugin", "add", "tiny:"].map(String::from).into();
        let err = cli_run(&args).unwrap_err();
        assert_display_snapshot!(err);
    }
}
