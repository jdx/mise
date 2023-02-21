use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;
use url::Url;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::Plugin;
use crate::shorthand::shorthand_to_repository;
use crate::toolset::ToolsetBuilder;
use crate::ui::progress_report::ProgressReport;

/// install a plugin
///
/// note that rtx automatically can install plugins when you install a runtime
/// e.g.: `rtx install nodejs@18` will autoinstall the nodejs plugin
///
/// This behavior can be modified in ~/.rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["i", "a"], alias = "add", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
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
    #[clap(short, long, conflicts_with_all = ["name", "force"], verbatim_doc_comment)]
    all: bool,

    /// Show installation output
    #[clap(long, short, action = clap::ArgAction::Count)]
    verbose: u8,
}

impl Command for PluginsInstall {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        if self.all {
            return self.install_all_missing_plugins(&config);
        }
        let (name, git_url) = get_name_and_url(self.name.unwrap(), self.git_url)?;
        let plugin = Plugin::new(&name);
        if self.force {
            plugin.uninstall()?;
        }
        if !self.force && plugin.is_installed() {
            warn!("plugin {} already installed", name);
        } else {
            let pr = ProgressReport::new(config.settings.verbose);
            plugin.install(&config.settings, Some(&git_url), pr)?;
        }

        Ok(())
    }
}

impl PluginsInstall {
    fn install_all_missing_plugins(&self, config: &Config) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config);
        let missing_plugins = ts.list_missing_plugins();
        if missing_plugins.is_empty() {
            warn!("all plugins already installed");
        }
        for plugin in missing_plugins {
            let plugin = Plugin::new(&plugin);
            let (_, git_url) = get_name_and_url(plugin.name.clone(), None)?;
            plugin.install(
                &config.settings,
                Some(&git_url),
                ProgressReport::new(config.settings.verbose),
            )?;
        }
        Ok(())
    }
}

fn get_name_and_url(name: String, git_url: Option<String>) -> Result<(String, String)> {
    Ok(match git_url {
        Some(url) => (name, url),
        None => match name.contains(':') {
            true => (get_name_from_url(&name)?, name),
            false => {
                let git_url = shorthand_to_repository(&name)
                    .ok_or_else(|| eyre!("could not find plugin {}", name))?;
                (name, git_url.to_string())
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
            return Ok(name.to_string());
        }
    }
    Err(eyre!("could not infer plugin name from url: {}", url))
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx install nodejs  # install the nodejs plugin using the shorthand repo:
                          # https://github.com/asdf-vm/asdf-plugins

      $ rtx install nodejs https://github.com/asdf-vm/asdf-nodejs.git
                          # install the nodejs plugin using the git url

      $ rtx install https://github.com/asdf-vm/asdf-nodejs.git
                          # install the nodejs plugin using the git url only
                          # (nodejs is inferred from the url)
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use insta::{assert_display_snapshot, assert_snapshot};

    use crate::assert_cli;
    use crate::cli::tests::cli_run;
    use crate::cli::tests::grep;

    #[test]
    fn test_plugin_install_url() {
        assert_cli!("plugin", "add", "-f", "https://github.com/jdxcode/rtx-tiny");
        let stdout = assert_cli!("plugin", "--urls");
        assert_snapshot!(grep(stdout, "tiny"), @"tiny                          https://github.com/jdxcode/rtx-tiny");
    }

    #[test]
    fn test_plugin_install_invalid_url() {
        let args = ["rtx", "plugin", "add", "ruby:"].map(String::from).into();
        let err = cli_run(&args).unwrap_err();
        assert_display_snapshot!(err);
    }

    #[test]
    fn test_plugin_install_all() {
        assert_cli!("plugin", "rm", "tiny");
        assert_cli!("plugin", "install", "--all");
        let stdout = assert_cli!("plugin");
        assert_snapshot!(grep(stdout, "tiny"), "tiny");
    }
}
