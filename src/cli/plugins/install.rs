use color_eyre::eyre::{bail, eyre, Result};
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use url::Url;

use crate::backend::unalias_backend;
use crate::config::{Config, Settings};
use crate::plugins::asdf_plugin::AsdfPlugin;
use crate::plugins::core::CORE_PLUGINS;
use crate::plugins::Plugin;
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::style;

/// Install a plugin
///
/// note that mise automatically can install plugins when you install a tool
/// e.g.: `mise install node@20` will autoinstall the node plugin
///
/// This behavior can be modified in ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["i", "a", "add"], verbatim_doc_comment, after_long_help = AFTER_LONG_HELP
)]
pub struct PluginsInstall {
    /// The name of the plugin to install
    /// e.g.: node, ruby
    /// Can specify multiple plugins: `mise plugins install node ruby python`
    #[clap(required_unless_present = "all", verbatim_doc_comment)]
    new_plugin: Option<String>,

    /// The git url of the plugin
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(help = "The git url of the plugin", value_hint = clap::ValueHint::Url, verbatim_doc_comment
    )]
    git_url: Option<String>,

    /// Reinstall even if plugin exists
    #[clap(short, long, verbatim_doc_comment)]
    force: bool,

    /// Install all missing plugins
    /// This will only install plugins that have matching shorthands.
    /// i.e.: they don't need the full git repo url
    #[clap(short, long, conflicts_with_all = ["new_plugin", "force"], verbatim_doc_comment)]
    all: bool,

    /// Show installation output
    #[clap(long, short, action = clap::ArgAction::Count, verbatim_doc_comment)]
    verbose: u8,

    #[clap(hide = true)]
    rest: Vec<String>,
}

impl PluginsInstall {
    pub fn run(self, config: &Config) -> Result<()> {
        let mpr = MultiProgressReport::get();
        if self.all {
            return self.install_all_missing_plugins(config, &mpr);
        }
        let (name, git_url) = get_name_and_url(&self.new_plugin.clone().unwrap(), &self.git_url)?;
        if git_url.is_some() {
            self.install_one(name, git_url, &mpr)?;
        } else {
            let is_core = CORE_PLUGINS.contains_key(&name);
            if is_core {
                let name = style::eblue(name);
                bail!("{name} is a core plugin and does not need to be installed");
            }
            let mut plugins: Vec<String> = vec![name];
            if let Some(second) = self.git_url.clone() {
                plugins.push(second);
            };
            plugins.extend(self.rest.clone());
            self.install_many(plugins, &mpr)?;
        }

        Ok(())
    }

    fn install_all_missing_plugins(
        &self,
        config: &Config,
        mpr: &MultiProgressReport,
    ) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config)?;
        let missing_plugins = ts.list_missing_plugins();
        if missing_plugins.is_empty() {
            warn!("all plugins already installed");
        }
        self.install_many(missing_plugins, mpr)?;
        Ok(())
    }

    fn install_many(&self, plugins: Vec<String>, mpr: &MultiProgressReport) -> Result<()> {
        ThreadPoolBuilder::new()
            .num_threads(Settings::get().jobs)
            .build()?
            .install(|| -> Result<()> {
                plugins
                    .into_par_iter()
                    .map(|plugin| self.install_one(plugin, None, mpr))
                    .collect::<Result<Vec<_>>>()?;
                Ok(())
            })
    }

    fn install_one(
        &self,
        name: String,
        git_url: Option<String>,
        mpr: &MultiProgressReport,
    ) -> Result<()> {
        let mut plugin = AsdfPlugin::new(name.clone());
        plugin.repo_url = git_url;
        if !self.force && plugin.is_installed() {
            warn!("Plugin {name} already installed");
            warn!("Use --force to install anyway");
        } else {
            plugin.ensure_installed(mpr, self.force)?;
        }
        Ok(())
    }
}

fn get_name_and_url(name: &str, git_url: &Option<String>) -> Result<(String, Option<String>)> {
    let name = unalias_backend(name);
    Ok(match git_url {
        Some(url) => match url.contains(':') {
            true => (name.to_string(), Some(url.clone())),
            false => (name.to_string(), None),
        },
        None => match name.contains(':') {
            true => (get_name_from_url(name)?, Some(name.to_string())),
            false => (name.to_string(), None),
        },
    })
}

fn get_name_from_url(url: &str) -> Result<String> {
    if let Ok(url) = Url::parse(url.trim_end_matches('/')) {
        if let Some(segments) = url.path_segments() {
            let last = segments.last().unwrap_or_default();
            let name = last.strip_prefix("asdf-").unwrap_or(last);
            let name = name.strip_prefix("rtx-").unwrap_or(name);
            let name = name.strip_prefix("mise-").unwrap_or(name);
            let name = name.strip_suffix(".git").unwrap_or(name);
            return Ok(unalias_backend(name).to_string());
        }
    }
    Err(eyre!("could not infer plugin name from url: {}", url))
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
    # install the node via shorthand
    $ <bold>mise plugins install node</bold>

    # install the node plugin using a specific git url
    $ <bold>mise plugins install node https://github.com/mise-plugins/rtx-nodejs.git</bold>

    # install the node plugin using the git url only
    # (node is inferred from the url)
    $ <bold>mise plugins install https://github.com/mise-plugins/rtx-nodejs.git</bold>

    # install the node plugin using a specific ref
    $ <bold>mise plugins install node https://github.com/mise-plugins/rtx-nodejs.git#v1.0.0</bold>
"#
);

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use test_log::test;

    use crate::test::reset;
    #[test]
    fn test_plugin_install_invalid_url() {
        reset();
        let err = assert_cli_err!("plugin", "add", "tiny*");
        assert_snapshot!(err, @"No repository found for plugin tiny*");
    }

    #[test]
    fn test_plugin_install_core_plugin() {
        reset();
        let err = assert_cli_err!("plugin", "add", "node");
        assert_snapshot!(err, @"node is a core plugin and does not need to be installed");
    }
}
