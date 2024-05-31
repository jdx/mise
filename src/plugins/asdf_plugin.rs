use crate::config::{Config, Settings};
use crate::default_shorthands::{DEFAULT_SHORTHANDS, TRUSTED_SHORTHANDS};
use crate::dirs;
use crate::errors::Error::PluginNotInstalled;
use crate::file::{display_path, remove_all};
use crate::git::Git;
use crate::plugins::{Plugin, PluginList, PluginType};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::ui::prompt;
use console::style;
use eyre::{bail, Context};
use rayon::prelude::*;
use std::path::Path;
use url::Url;
use xx::file;

#[derive(Debug)]
pub struct AsdfPlugin {
    pub name: String,
    pub repo: Git,
    pub repo_url: Option<String>,
}

impl AsdfPlugin {
    pub fn new(name: String) -> Self {
        let dir = dirs::PLUGINS.join(&name);
        Self {
            name,
            repo: Git::new(dir),
            repo_url: None,
        }
    }

    pub fn list() -> eyre::Result<PluginList> {
        let settings = Settings::get();
        Ok(file::ls(*dirs::PLUGINS)?
            .into_par_iter()
            .map(|dir| {
                let name = dir.file_name().unwrap().to_string_lossy().to_string();
                Box::new(AsdfPlugin::new(name)) as Box<dyn Plugin>
            })
            .filter(|p| !settings.disable_tools.contains(p.name()))
            .collect())
    }
}

impl Plugin for AsdfPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn get_plugin_type(&self) -> PluginType {
        PluginType::Asdf
    }

    fn get_remote_url(&self) -> eyre::Result<Option<String>> {
        let url = self.repo.get_remote_url();
        Ok(url.or(self.repo_url.clone()))
    }

    fn current_abbrev_ref(&self) -> eyre::Result<Option<String>> {
        if !self.is_installed() {
            return Ok(None);
        }
        self.repo.current_abbrev_ref().map(Some)
    }

    fn current_sha_short(&self) -> eyre::Result<Option<String>> {
        if !self.is_installed() {
            return Ok(None);
        }
        self.repo.current_sha_short().map(Some)
    }

    fn is_installed(&self) -> bool {
        self.repo.exists()
    }

    fn ensure_installed(&self, mpr: &MultiProgressReport, force: bool) -> eyre::Result<()> {
        let config = Config::get();
        let settings = Settings::try_get()?;
        if !force {
            if self.is_installed() {
                return Ok(());
            }
            if !settings.yes && self.repo_url.is_none() {
                let url = self.get_repo_url(&config).unwrap_or_default();
                if !is_trusted_plugin(self.name(), &url) {
                    warn!(
                        "⚠️ {} is a community-developed plugin",
                        style(&self.name).blue(),
                    );
                    warn!("url: {}", style(url.trim_end_matches(".git")).yellow(),);
                    if settings.paranoid {
                        bail!("Paranoid mode is enabled, refusing to install community-developed plugin");
                    }
                    if !prompt::confirm_with_all(format!(
                        "Would you like to install {}?",
                        self.name
                    ))? {
                        Err(PluginNotInstalled(self.name.clone()))?
                    }
                }
            }
        }
        let prefix = format!("plugin:{}", style(&self.name).blue().for_stderr());
        let pr = mpr.add(&prefix);
        let _lock = self.get_lock(&self.plugin_path, force)?;
        self.install(pr.as_ref())
    }

    fn uninstall(&self, pr: &dyn SingleReport) -> eyre::Result<()> {
        if !self.is_installed() {
            return Ok(());
        }
        self.exec_hook(pr, "pre-plugin-remove")?;
        pr.set_message("uninstalling".into());

        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            pr.set_message(format!("removing {}", display_path(dir)));
            remove_all(dir).wrap_err_with(|| {
                format!(
                    "Failed to remove directory {}",
                    style(display_path(dir)).cyan().for_stderr()
                )
            })
        };

        rmdir(&self.repo.dir)?;

        Ok(())
    }

    fn update(&self, pr: &dyn SingleReport, gitref: Option<String>) -> eyre::Result<()> {
        let plugin_path = self.plugin_path.to_path_buf();
        if plugin_path.is_symlink() {
            warn!(
                "plugin:{} is a symlink, not updating",
                style(&self.name).blue().for_stderr()
            );
            return Ok(());
        }
        let git = Git::new(plugin_path);
        if !git.is_repo() {
            warn!(
                "plugin:{} is not a git repository, not updating",
                style(&self.name).blue().for_stderr()
            );
            return Ok(());
        }
        pr.set_message("updating git repo".into());
        let (pre, post) = git.update(gitref)?;
        let sha = git.current_sha_short()?;
        let repo_url = self.get_remote_url().unwrap_or_default();
        self.exec_hook_post_plugin_update(pr, pre, post)?;
        pr.finish_with_message(format!(
            "{repo_url}#{}",
            style(&sha).bright().yellow().for_stderr(),
        ));
        Ok(())
    }
}

fn is_trusted_plugin(name: &str, remote: &str) -> bool {
    let normalized_url = normalize_remote(remote).unwrap_or("INVALID_URL".into());
    let is_shorthand = DEFAULT_SHORTHANDS
        .get(name)
        .is_some_and(|s| normalize_remote(s).unwrap_or_default() == normalized_url);
    let is_mise_url = normalized_url.starts_with("github.com/mise-plugins/");

    !is_shorthand || is_mise_url || TRUSTED_SHORTHANDS.contains(name)
}

fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url.host_str().unwrap();
    let path = url.path().trim_end_matches(".git");
    Ok(format!("{host}{path}"))
}
