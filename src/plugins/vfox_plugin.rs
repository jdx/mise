use crate::config::{Config, Settings};
use crate::errors::Error::PluginNotInstalled;
use crate::file::{display_path, remove_all};
use crate::git::Git;
use crate::plugins::{Plugin, PluginList, PluginType};
use crate::result::Result;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::ui::prompt;
use crate::{dirs, lock_file, plugins};
use console::style;
use contracts::requires;
use eyre::{bail, eyre, Context};
use once_cell::sync::Lazy;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use xx::regex;

#[derive(Debug)]
pub struct VfoxPlugin {
    pub name: String,
    pub plugin_path: PathBuf,
    pub repo: Mutex<Git>,
    pub repo_url: Option<String>,
}

pub static VFOX_PLUGIN_NAMES: Lazy<BTreeSet<String>> = Lazy::new(|| match VfoxPlugin::list() {
    Ok(plugins) => plugins.into_iter().map(|p| p.name().to_string()).collect(),
    Err(err) => {
        warn!("Failed to list vfox plugins: {err}");
        BTreeSet::new()
    }
});

impl VfoxPlugin {
    #[requires(!name.is_empty())]
    pub fn new(name: String) -> Self {
        let plugin_path = dirs::PLUGINS.join(&name);
        let repo = Git::new(&plugin_path);
        Self {
            name,
            repo_url: None,
            repo: Mutex::new(repo),
            plugin_path,
        }
    }

    pub fn list() -> eyre::Result<PluginList> {
        let settings = Settings::get();
        let plugins = plugins::INSTALLED_PLUGINS
            .iter()
            .filter(|(_, t)| matches!(t, PluginType::Vfox))
            .map(|(dir, _)| {
                let name = dir.file_name().unwrap().to_string_lossy().to_string();
                Box::new(VfoxPlugin::new(name)) as Box<dyn Plugin>
            })
            .filter(|p| !settings.disable_tools.contains(p.name()))
            .collect();
        Ok(plugins)
    }

    fn repo(&self) -> MutexGuard<Git> {
        self.repo.lock().unwrap()
    }

    fn get_repo_url(&self, config: &Config) -> eyre::Result<String> {
        self.repo_url
            .clone()
            .or_else(|| self.repo().get_remote_url())
            .or_else(|| config.get_repo_url(&self.name))
            .ok_or_else(|| eyre!("No repository found for plugin {}", self.name))
    }
}

impl Plugin for VfoxPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> PathBuf {
        self.plugin_path.clone()
    }

    fn get_plugin_type(&self) -> PluginType {
        PluginType::Vfox
    }

    fn get_remote_url(&self) -> eyre::Result<Option<String>> {
        let url = self.repo().get_remote_url();
        Ok(url.or(self.repo_url.clone()))
    }

    fn current_abbrev_ref(&self) -> eyre::Result<Option<String>> {
        if !self.is_installed() {
            return Ok(None);
        }
        self.repo().current_abbrev_ref().map(Some)
    }

    fn current_sha_short(&self) -> eyre::Result<Option<String>> {
        if !self.is_installed() {
            return Ok(None);
        }
        self.repo().current_sha_short().map(Some)
    }

    fn is_installed(&self) -> bool {
        self.plugin_path.exists()
    }

    fn is_installed_err(&self) -> eyre::Result<()> {
        if self.is_installed() {
            return Ok(());
        }
        Err(eyre!("asdf plugin {} is not installed", self.name())
            .wrap_err("run with --yes to install plugin automatically"))
    }

    fn ensure_installed(&self, mpr: &MultiProgressReport, force: bool) -> Result<()> {
        let config = Config::get();
        let settings = Settings::try_get()?;
        if !force {
            if self.is_installed() {
                return Ok(());
            }
            if !settings.yes && self.repo_url.is_none() {
                let url = self.get_repo_url(&config).unwrap_or_default();
                warn!(
                    "⚠️ {} is a community-developed plugin – {}",
                    style(&self.name).blue(),
                    style(url.trim_end_matches(".git")).yellow()
                );
                if settings.paranoid {
                    bail!(
                        "Paranoid mode is enabled, refusing to install community-developed plugin"
                    );
                }
                if !prompt::confirm_with_all(format!("Would you like to install {}?", self.name))? {
                    Err(PluginNotInstalled(self.name.clone()))?
                }
            }
        }
        let prefix = format!("plugin:{}", style(&self.name).blue().for_stderr());
        let pr = mpr.add(&prefix);
        let _lock = lock_file::get(&self.plugin_path, force)?;
        self.install(pr.as_ref())
    }

    fn update(&self, pr: &dyn SingleReport, gitref: Option<String>) -> Result<()> {
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
        git.update(gitref)?;
        let sha = git.current_sha_short()?;
        let repo_url = self.get_remote_url()?.unwrap_or_default();
        pr.finish_with_message(format!(
            "{repo_url}#{}",
            style(&sha).bright().yellow().for_stderr(),
        ));
        Ok(())
    }

    fn uninstall(&self, pr: &dyn SingleReport) -> Result<()> {
        if !self.is_installed() {
            return Ok(());
        }
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

        rmdir(&self.plugin_path)?;

        Ok(())
    }

    fn install(&self, pr: &dyn SingleReport) -> eyre::Result<()> {
        let config = Config::get();
        let repository = self.get_repo_url(&config)?;
        let (repo_url, repo_ref) = Git::split_url_and_ref(&repository);
        debug!("vfox_plugin[{}]:install {:?}", self.name, repository);

        if self.is_installed() {
            self.uninstall(pr)?;
        }

        if regex!(r"^[/~]").is_match(&repo_url) {
            Err(eyre!(
                r#"Invalid repository URL: {repo_url}
If you are trying to link to a local directory, use `mise plugins link` instead.
Plugins could support local directories in the future but for now a symlink is required which `mise plugins link` will create for you."#
            ))?;
        }
        let git = Git::new(&self.plugin_path);
        pr.set_message(format!("cloning {repo_url}"));
        git.clone(&repo_url)?;
        if let Some(ref_) = &repo_ref {
            pr.set_message(format!("checking out {ref_}"));
            git.update(Some(ref_.to_string()))?;
        }

        let sha = git.current_sha_short()?;
        pr.finish_with_message(format!(
            "{repo_url}#{}",
            style(&sha).bright().yellow().for_stderr(),
        ));
        Ok(())
    }
}
