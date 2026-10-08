use std::sync::Arc;

use color_eyre::eyre::{Result, bail, eyre};
use contracts::ensures;
use heck::ToKebabCase;
use tokio::{sync::Semaphore, task::JoinSet};
use url::Url;

use crate::config::Config;
use crate::dirs;
use crate::plugins::PluginType;
use crate::plugins::core::CORE_PLUGINS;
use crate::plugins::{plugin_drift, warn_if_env_plugin_shadows_registry, warn_plugin_drift};
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::style;
use crate::{backend::unalias_backend, config::Settings};

use super::{PluginTaskNames, PluginTaskResult, join_plugin_tasks, spawn_plugin_task};

/// Install plugins from a configured source, Git URL, or archive
///
/// Most registry tools use built-in backends and need no plugin. When a tool's
/// backend requires a plugin, mise installs it with the tool. Use this command
/// to install one ahead of time or from a custom source.
///
/// Pass a name that the registry or a `[plugins]` entry knows, a name and a
/// URL, or only a URL. With only a URL, the plugin is named after the
/// repository without an `asdf-`, `mise-`, or `vfox-` prefix. Pass several names
/// to install several plugins. Prefix a name with `vfox:`, `vfox-backend:`,
/// `package:`, or `asdf:` to choose the plugin type.
///
/// A Git URL may end in `#ref` to select a plugin commit, tag, or branch. This
/// selects the plugin code, separately from the tool version `mise use` installs.
#[derive(Debug, usage_rs::Args)]
#[usage(visible_aliases = ["i", "a", "add"], verbatim_doc_comment,
    example(r###"mise plugins install my-tool https://github.com/your-org/mise-my-tool"###, help = r###"Install a plugin from a Git repository"###),
    example(r###"mise plugins install my-tool 'https://github.com/your-org/mise-my-tool#v1.2.0'"###, help = r###"Pin the plugin to a tag"###),
    example(r###"mise plugins install https://github.com/your-org/vfox-my-tool"###, help = r###"Name the plugin after its repository (my-tool)"###),
    example(r###"mise plugins install my-tool file:///path/to/mise-my-tool#v1.0.0"###, help = r###"Use a local plugin repository at a tag you created"###),
    example(r###"mise plugins install --all"###, help = r###"Install the plugins that the current config needs"###)
)]
pub(crate) struct PluginsInstall {
    /// The plugin to install, or a URL to infer its name from
    #[usage(required_unless = "all", verbatim_doc_comment)]
    new_plugin: Option<String>,

    /// Where to install from: a Git URL (optionally ending in #ref) or an archive URL
    #[usage(value_hint = usage_rs::ValueHint::Url, verbatim_doc_comment)]
    git_url: Option<String>,

    #[usage(hide = true)]
    rest: Vec<String>,

    /// Install every plugin the current config needs that is not installed yet
    ///
    /// Only plugins with a registry shorthand or a `[plugins]` entry can be
    /// installed this way.
    #[usage(short, long, conflicts = ["new_plugin", "force"])]
    all: bool,

    /// Reinstall even if the plugin is already installed
    #[usage(short, long, verbatim_doc_comment)]
    force: bool,

    /// Number of plugins to install in parallel (default: the `jobs` setting)
    #[usage(long, short, verbatim_doc_comment)]
    jobs: Option<usize>,

    /// Show installation output
    #[usage(long, short, action = usage_rs::ArgAction::Count, verbatim_doc_comment)]
    verbose: u8,
}

impl PluginsInstall {
    pub(crate) async fn run(self, config: &Arc<Config>) -> Result<()> {
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
        warn_plugin_drift(config);
        self.install_many(config, missing_plugins).await?;
        Ok(())
    }

    async fn install_many(
        self: Arc<Self>,
        config: &Arc<Config>,
        plugins: Vec<String>,
    ) -> Result<()> {
        let mut jset: JoinSet<PluginTaskResult> = JoinSet::new();
        let mut task_names = PluginTaskNames::new();
        let jobs = crate::jobs::resolve(Settings::get().jobs, self.jobs);
        let semaphore = Arc::new(Semaphore::new(jobs));
        for plugin in plugins {
            let this = self.clone();
            let config = config.clone();
            let semaphore = semaphore.clone();
            let plugin_name = plugin.clone();
            spawn_plugin_task(&mut jset, &mut task_names, plugin_name, async move {
                let _permit = semaphore.acquire_owned().await?;
                // Progress only: a closed stdout must not skip the install.
                let _ = miseprint!("installing {plugin}\n");
                this.install_one(&config, plugin, None).await
            });
        }
        join_plugin_tasks(jset, task_names, "install").await
    }

    async fn install_one(
        self: Arc<Self>,
        config: &Arc<Config>,
        name: String,
        git_url: Option<String>,
    ) -> Result<()> {
        install_plugin(config, &name, git_url, self.force, false).await
    }
}

pub(crate) async fn install_plugin(
    config: &Arc<Config>,
    name: &str,
    git_url: Option<String>,
    force: bool,
    dry_run: bool,
) -> Result<()> {
    let explicit_type = name.contains(':');
    let (mut plugin_type, name) = PluginType::from_plugin_config(name);
    // `[plugins]` says what to install, so `--force` reinstalls from it rather
    // than from the existing checkout's origin. Without `--force` the plugin
    // resolves the entry itself, keeping the untrusted-plugin prompt that an
    // explicit URL skips.
    let git_url = git_url
        .or_else(|| force.then(|| config.configured_plugin_url(name)).flatten())
        .or_else(|| {
            config
                .get_repo_url(name)
                .filter(|url| url.starts_with("packslip:"))
        });
    if git_url
        .as_deref()
        .is_some_and(|url| url.starts_with("packslip:"))
    {
        if explicit_type && plugin_type != PluginType::Vfox {
            bail!("packslip plugin sources require the vfox plugin type");
        }
        plugin_type = PluginType::Vfox;
    }
    let name = name.to_string();
    if plugin_type == PluginType::Package && crate::system::packages::is_builtin_manager_name(&name)
    {
        bail!("package plugin '{name}' collides with a built-in package manager");
    }
    let path = dirs::PLUGINS.join(name.to_kebab_case());
    let plugin = plugin_type.plugin(name.clone());
    if let Some(url) = git_url {
        plugin.set_remote_url(url);
    }
    if !force && plugin.is_installed() {
        warn!("Plugin {name} already installed");
        warn!("Use --force to install anyway");
        if let Some(drift) = plugin_drift(config).into_iter().find(|d| d.name == name) {
            warn!("{drift}");
        }
    } else {
        let mpr = MultiProgressReport::get();
        plugin
            .ensure_installed(config, &mpr, force, dry_run)
            .await?;
        if !dry_run {
            warn_if_env_plugin_shadows_registry(&name, &path);
        }
    }
    Ok(())
}

#[ensures(!ret.as_ref().is_ok_and(|(r, _)| r.is_empty()), "plugin name is empty")]
fn get_name_and_url(name: &str, git_url: &Option<String>) -> Result<(String, Option<String>)> {
    let name = unalias_backend(name);
    let name = name.as_ref();
    if git_url.is_none()
        && let Some((kind, short)) = name.split_once(':')
        && matches!(kind, "vfox" | "vfox-backend" | "package" | "asdf")
        && !short.is_empty()
        && !short.contains(['/', ':'])
    {
        return Ok((name.to_string(), None));
    }
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
    let name = name.strip_prefix("mise-").unwrap_or(name);
    let name = name.strip_prefix("vfox-").unwrap_or(name);
    Ok(unalias_backend(name).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_str_eq;

    #[test]
    fn typed_plugin_name_uses_configured_source() {
        assert_eq!(
            get_name_and_url("vfox:bfs", &None).unwrap(),
            ("vfox:bfs".to_string(), None)
        );
    }

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
