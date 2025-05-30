use std::sync::Arc;

use color_eyre::eyre::{Result, bail, eyre};
use contracts::ensures;
use heck::ToKebabCase;
use tokio::{sync::Semaphore, task::JoinSet};
use url::Url;

use crate::config::Config;
use crate::dirs;
use crate::plugins::Plugin;
use crate::plugins::asdf_plugin::AsdfPlugin;
use crate::plugins::core::CORE_PLUGINS;
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::style;
use crate::{backend::unalias_backend, config::Settings};

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

    /// Number of jobs to run in parallel
    #[clap(long, short, verbatim_doc_comment)]
    jobs: Option<usize>,

    #[clap(hide = true)]
    rest: Vec<String>,
}

impl PluginsInstall {
    pub async fn run(self, config: &Arc<Config>) -> Result<()> {
        let this = Arc::new(self);
        if this.all {
            return this.install_all_missing_plugins(config).await;
        }
        let (name, git_url) = get_name_and_url(&this.new_plugin.clone().unwrap(), &this.git_url)?;
        if git_url.is_some() {
            this.install_one(config, name, git_url).await?;
        } else {
            let is_core = CORE_PLUGINS.contains_key(&name);
            if is_core {
                let name = style::eblue(name);
                bail!("{name} is a core plugin and does not need to be installed");
            }
            let mut plugins: Vec<String> = vec![name];
            if let Some(second) = this.git_url.clone() {
                plugins.push(second);
            };
            plugins.extend(this.rest.clone());
            this.install_many(config, plugins).await?;
        }

        Ok(())
    }

    async fn install_all_missing_plugins(self: Arc<Self>, config: &Arc<Config>) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config).await?;
        let missing_plugins = ts.list_missing_plugins();
        if missing_plugins.is_empty() {
            warn!("all plugins already installed");
        }
        self.install_many(config, missing_plugins).await?;
        Ok(())
    }

    async fn install_many(
        self: Arc<Self>,
        config: &Arc<Config>,
        plugins: Vec<String>,
    ) -> Result<()> {
        let mut jset: JoinSet<Result<()>> = JoinSet::new();
        let semaphore = Arc::new(Semaphore::new(self.jobs.unwrap_or(Settings::get().jobs)));
        for plugin in plugins {
            let this = self.clone();
            let config = config.clone();
            let permit = semaphore.clone().acquire_owned().await?;
            jset.spawn(async move {
                let _permit = permit;
                println!("installing {plugin}");
                this.install_one(&config, plugin, None).await
            });
        }
        while let Some(result) = jset.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    return Err(e);
                }
                Err(e) => {
                    return Err(eyre!(e));
                }
            }
        }
        Ok(())
    }

    async fn install_one(
        self: Arc<Self>,
        config: &Arc<Config>,
        name: String,
        git_url: Option<String>,
    ) -> Result<()> {
        let path = dirs::PLUGINS.join(name.to_kebab_case());
        let plugin = AsdfPlugin::new(name.clone(), path);
        if let Some(url) = git_url {
            plugin.set_remote_url(url);
        }
        if !self.force && plugin.is_installed() {
            warn!("Plugin {name} already installed");
            warn!("Use --force to install anyway");
        } else {
            let mpr = MultiProgressReport::get();
            plugin.ensure_installed(config, &mpr, self.force).await?;
        }
        Ok(())
    }
}

#[ensures(!ret.as_ref().is_ok_and(|(r, _)| r.is_empty()), "plugin name is empty")]
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
    let url = url.strip_prefix("git@").unwrap_or(url);
    let url = url.strip_suffix(".git").unwrap_or(url);
    let url = url.strip_suffix("/").unwrap_or(url);
    let name = if let Ok(Some(name)) = Url::parse(url).map(|u| {
        u.path_segments()
            .and_then(|mut s| s.next_back().map(|s| s.to_string()))
    }) {
        name
    } else if let Some(name) = url.split('/').next_back().map(|s| s.to_string()) {
        name
    } else {
        return Err(eyre!("could not infer plugin name from url: {}", url));
    };
    let name = name.strip_prefix("asdf-").unwrap_or(&name);
    let name = name.strip_prefix("rtx-").unwrap_or(name);
    let name = name.strip_prefix("mise-").unwrap_or(name);
    Ok(unalias_backend(name).to_string())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # install the poetry via shorthand
    $ <bold>mise plugins install poetry</bold>

    # install the poetry plugin using a specific git url
    $ <bold>mise plugins install poetry https://github.com/mise-plugins/mise-poetry.git</bold>

    # install the poetry plugin using the git url only
    # (poetry is inferred from the url)
    $ <bold>mise plugins install https://github.com/mise-plugins/mise-poetry.git</bold>

    # install the poetry plugin using a specific ref
    $ <bold>mise plugins install poetry https://github.com/mise-plugins/mise-poetry.git#11d0c1e</bold>
"#
);

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_str_eq;

    #[test]
    fn test_get_name_from_url() {
        let get_name = |url| get_name_from_url(url).unwrap();
        assert_str_eq!(get_name("nodejs"), "node");
        assert_str_eq!(
            get_name("https://github.com/mise-plugins/mise-nodejs.git"),
            "node"
        );
        assert_str_eq!(
            get_name("https://github.com/mise-plugins/asdf-nodejs.git"),
            "node"
        );
        assert_str_eq!(
            get_name("https://github.com/mise-plugins/asdf-nodejs/"),
            "node"
        );
        assert_str_eq!(
            get_name("git@github.com:mise-plugins/asdf-nodejs.git"),
            "node"
        );
    }
}
