use std::sync::Arc;

use color_eyre::eyre::{eyre, Result};
use url::Url;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::Plugin;
use crate::shorthand_repository::ShorthandRepo;

/// install a plugin
///
/// note that rtx automatically can install plugins when you install a runtime
/// e.g.: `rtx install nodejs@18` will autoinstall the nodejs plugin
///
/// This behavior can be modified in ~/.rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["i", "a"], alias = "add", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct PluginsInstall {
    /// The name of the plugin to install
    ///
    /// e.g.: nodejs, ruby
    #[clap(required_unless_present = "all")]
    name: Option<String>,

    /// The git url of the plugin
    ///
    /// e.g.: https://github.com/asdf-vm/asdf-nodejs.git
    #[clap(help = "The git url of the plugin", value_hint = clap::ValueHint::Url)]
    git_url: Option<String>,

    /// Reinstall even if plugin exists
    #[clap(short, long)]
    force: bool,

    /// Install all missing plugins
    ///
    /// This will only install plugins that have matching shortnames.
    /// i.e.: they don't need the full git repo url
    #[clap(long, conflicts_with_all = ["name", "force"])]
    all: bool,
}

impl Command for PluginsInstall {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        if self.all {
            return self.install_all_missing_plugins(&config);
        }
        let (name, git_url) = get_name_and_url(&config, self.name.unwrap(), self.git_url)?;
        let plugin = Plugin::load(&name)?;
        if self.force {
            plugin.uninstall()?;
        }
        if !self.force && plugin.is_installed() {
            warn!("plugin {} already installed", name);
        } else {
            plugin.install(&git_url)?;
        }

        Ok(())
    }
}

impl PluginsInstall {
    fn install_all_missing_plugins(&self, config: &Config) -> Result<()> {
        let missing_plugins = self.missing_plugins(config)?;
        if missing_plugins.is_empty() {
            warn!("all plugins already installed");
        }
        for plugin in missing_plugins {
            let (_, git_url) = get_name_and_url(config, plugin.name.clone(), None)?;
            plugin.install(&git_url)?;
        }
        Ok(())
    }

    fn missing_plugins(&self, config: &Config) -> Result<Vec<Arc<Plugin>>> {
        Ok(config
            .ts
            .list_plugins()
            .into_iter()
            .filter(|p| !p.is_installed())
            .collect::<Vec<_>>())
    }
}

fn get_name_and_url(
    config: &Config,
    name: String,
    git_url: Option<String>,
) -> Result<(String, String)> {
    Ok(match git_url {
        Some(url) => (name, url),
        None => match name.contains(':') {
            true => (get_name_from_url(&name)?, name),
            false => {
                let shr = ShorthandRepo::new(&config.settings);
                let git_url = shr.lookup(&name)?;
                (name, git_url)
            }
        },
    })
}

fn get_name_from_url(url: &str) -> Result<String> {
    if let Ok(url) = Url::parse(url) {
        if let Some(segments) = url.path_segments() {
            let last = segments.last().unwrap_or_default();
            let name = last.strip_prefix("asdf-").unwrap_or(last);
            return Ok(name.to_string());
        }
    }
    Err(eyre!("could not infer plugin name from url: {}", url))
}

const AFTER_LONG_HELP: &str = r#"
EXAMPLES:
    $ rtx install nodejs  # install the nodejs plugin using the shorthand repo:
                          # https://github.com/asdf-vm/asdf-plugins

    $ rtx install nodejs https://github.com/asdf-vm/asdf-nodejs.git
                          # install the nodejs plugin using the git url

    $ rtx install https://github.com/asdf-vm/asdf-nodejs.git
                          # install the nodejs plugin using the git url only
                          # (nodejs is inferred from the url)
"#;

#[cfg(test)]
mod test {
    use insta::{assert_display_snapshot, assert_snapshot};

    use crate::assert_cli;
    use crate::cli::test::cli_run;
    use crate::cli::test::grep;

    #[test]
    fn test_plugin_install() {
        assert_cli!("plugin", "add", "nodejs");
    }

    #[test]
    fn test_plugin_install_url() {
        assert_cli!(
            "plugin",
            "add",
            "-f",
            "https://github.com/jdxcode/asdf-nodejs"
        );
        let stdout = assert_cli!("plugin", "--urls");
        assert_snapshot!(grep(stdout, "nodejs"), @"nodejs                        https://github.com/jdxcode/asdf-nodejs");
    }

    #[test]
    fn test_plugin_install_invalid_url() {
        let args = ["rtx", "plugin", "add", "ruby:"].map(String::from).into();
        let err = cli_run(&args).unwrap_err();
        assert_display_snapshot!(err);
    }

    #[test]
    fn test_plugin_install_all() {
        assert_cli!("plugin", "rm", "nodejs");
        assert_cli!("plugin", "install", "--all");
        let stdout = assert_cli!("plugin");
        assert_snapshot!(grep(stdout, "nodejs"), "nodejs");
    }
}
