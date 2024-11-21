use crate::file::{display_path, remove_all};
use crate::git::Git;
use crate::plugins::{Plugin, PluginType};
use crate::result::Result;
use crate::tokio::RUNTIME;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::{dirs, registry};
use console::style;
use contracts::requires;
use eyre::{eyre, Context};
use indexmap::{indexmap, IndexMap};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex, MutexGuard};
use url::Url;
use vfox::Vfox;
use xx::regex;

#[derive(Debug)]
pub struct VfoxPlugin {
    pub name: String,
    pub full: Option<String>,
    pub plugin_path: PathBuf,
    pub repo: Mutex<Git>,
    pub repo_url: Option<String>,
}

impl VfoxPlugin {
    #[requires(!name.is_empty())]
    pub fn new(name: String, plugin_path: PathBuf) -> Self {
        let repo = Git::new(&plugin_path);
        Self {
            name,
            full: None,
            repo_url: None,
            repo: Mutex::new(repo),
            plugin_path,
        }
    }

    fn repo(&self) -> MutexGuard<Git> {
        self.repo.lock().unwrap()
    }

    fn get_repo_url(&self) -> eyre::Result<Url> {
        if let Some(url) = self.repo().get_remote_url() {
            return Ok(Url::parse(&url)?);
        }
        vfox_to_url(self.full.as_ref().unwrap_or(&self.name))
    }

    pub fn mise_env(&self, opts: &toml::Value) -> Result<Option<IndexMap<String, String>>> {
        let (vfox, _) = self.vfox();
        let mut out = indexmap!();
        let results = RUNTIME.block_on(vfox.mise_env(&self.name, opts))?;
        for env in results {
            out.insert(env.key, env.value);
        }
        Ok(Some(out))
    }

    pub fn mise_path(&self, opts: &toml::Value) -> Result<Option<Vec<String>>> {
        let (vfox, _) = self.vfox();
        let mut out = vec![];
        let results = RUNTIME.block_on(vfox.mise_path(&self.name, opts))?;
        for env in results {
            out.push(env);
        }
        Ok(Some(out))
    }

    pub fn vfox(&self) -> (Vfox, mpsc::Receiver<String>) {
        let mut vfox = Vfox::new();
        vfox.plugin_dir = dirs::PLUGINS.to_path_buf();
        vfox.cache_dir = dirs::CACHE.to_path_buf();
        vfox.download_dir = dirs::DOWNLOADS.to_path_buf();
        vfox.install_dir = dirs::INSTALLS.to_path_buf();
        let rx = vfox.log_subscribe();
        (vfox, rx)
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

    fn set_remote_url(&mut self, url: String) {
        self.repo_url = Some(url);
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

    fn ensure_installed(&self, _mpr: &MultiProgressReport, _force: bool) -> Result<()> {
        if !self.plugin_path.exists() {
            let url = self.get_repo_url()?;
            trace!("Cloning vfox plugin: {url}");
            self.repo().clone(url.as_str())?;
        }
        Ok(())
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
        pr.set_message("update git repo".into());
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
        pr.set_message("uninstall".into());

        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            pr.set_message(format!("remove {}", display_path(dir)));
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
        let repository = self.get_repo_url()?;
        let (repo_url, repo_ref) = Git::split_url_and_ref(repository.as_str());
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
        pr.set_message(format!("clone {repo_url}"));
        git.clone(&repo_url)?;
        if let Some(ref_) = &repo_ref {
            pr.set_message(format!("git update {ref_}"));
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

fn vfox_to_url(name: &str) -> eyre::Result<Url> {
    let name = name.strip_prefix("vfox:").unwrap_or(name);
    if let Some(rt) = registry::REGISTRY.get(name.trim_start_matches("vfox-")) {
        // bun -> version-fox/vfox-bun
        if let Some(full) = rt.backends.iter().find(|f| f.starts_with("vfox:")) {
            return vfox_to_url(full.split_once(':').unwrap().1);
        }
    }
    let res = if let Some(caps) = regex!(r#"^([^/]+)/([^/]+)$"#).captures(name) {
        let user = caps.get(1).unwrap().as_str();
        let repo = caps.get(2).unwrap().as_str();
        format!("https://github.com/{user}/{repo}").parse()
    } else {
        name.to_string().parse()
    };
    res.wrap_err_with(|| format!("Invalid version: {name}"))
}
