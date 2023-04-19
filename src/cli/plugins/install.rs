use color_eyre::eyre::{eyre, Result};
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use url::Url;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::{ExternalPlugin, Plugin, PluginName};
use crate::tool::Tool;
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;

/// Install a plugin
///
/// note that rtx automatically can install plugins when you install a runtime
/// e.g.: `rtx install nodejs@20` will autoinstall the nodejs plugin
///
/// This behavior can be modified in ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["i", "a"], alias = "add", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
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
        let (name, git_url) = get_name_and_url(&self.name.clone().unwrap(), &self.git_url)?;
        if git_url.is_some() {
            self.install_one(&config, &name, git_url, &mpr)?;
        } else {
            let mut plugins: Vec<PluginName> = vec![name];
            if let Some(second) = self.git_url.clone() {
                plugins.push(second);
            };
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
        let ts = ToolsetBuilder::new().build(config)?;
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
                    .map(|plugin| self.install_one(config, plugin, None, &mpr))
                    .collect::<Result<Vec<_>>>()?;
                Ok(())
            })
    }

    fn install_one(
        &self,
        config: &Config,
        name: &String,
        git_url: Option<String>,
        mpr: &MultiProgressReport,
    ) -> Result<()> {
        let mut plugin = ExternalPlugin::new(&config.settings, name);
        plugin.repo_url = git_url;
        if !self.force && plugin.is_installed() {
            mpr.warn(format!("plugin {} already installed", name));
        } else {
            let mut pr = mpr.add();
            let tool = Tool::new(plugin.name.clone(), Box::new(plugin));
            tool.decorate_progress_bar(&mut pr, None);
            tool.install(config, &mut pr, self.force)?;
        }
        Ok(())
    }
}

fn get_name_and_url(name: &str, git_url: &Option<String>) -> Result<(String, Option<String>)> {
    Ok(match git_url {
        Some(url) => match url.contains("://") {
            true => (name.to_string(), Some(url.clone())),
            false => (name.to_string(), None),
        },
        None => match name.contains("://") {
            true => (get_name_from_url(name)?, Some(name.to_string())),
            false => (name.to_string(), None),
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  # install the nodejs via shorthand
  $ <bold>rtx plugins install nodejs</bold>

  # install the nodejs plugin using a specific git url
  $ <bold>rtx plugins install nodejs https://github.com/jdxcode/rtx-nodejs.git</bold>

  # install the nodejs plugin using the git url only
  # (nodejs is inferred from the url)
  $ <bold>rtx plugins install https://github.com/jdxcode/rtx-nodejs.git</bold>

  # install the nodejs plugin using a specific ref
  $ <bold>rtx plugins install nodejs https://github.com/jdxcode/rtx-nodejs.git#v1.0.0</bold>
"#
);

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
