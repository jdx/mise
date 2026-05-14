use crate::config::{Config, Settings};
use crate::errors::Error::PluginNotInstalled;
use crate::file::remove_all_with_progress;
use crate::git::{CloneOptions, Git};
use crate::http::HTTP;
use crate::plugins::warn_if_env_plugin_shadows_registry;
use crate::plugins::{Plugin, PluginSource, PluginType};
use crate::result::Result;
use crate::toolset::install_state;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::ui::prompt;
use crate::{backend, dirs, file, lock_file, registry};
use async_trait::async_trait;
use console::style;
use contracts::requires;
use eyre::{Context, bail, eyre};
use indexmap::{IndexMap, indexmap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use url::Url;
use vfox::Vfox;
use vfox::embedded_plugins;

/// Result from a mise_env call with cache metadata
#[derive(Debug, Default)]
pub struct MiseEnvResponse {
    /// Environment variables to set
    pub env: IndexMap<String, String>,
    /// Whether this module's output can be cached
    pub cacheable: bool,
    /// Files to watch for cache invalidation
    pub watch_files: Vec<PathBuf>,
    /// Whether the plugin wants its env vars to be redacted
    pub redact: bool,
}
use xx::regex;

#[derive(Debug)]
pub struct VfoxPlugin {
    pub name: String,
    pub full: Option<String>,
    pub plugin_path: PathBuf,
    pub repo: Mutex<Git>,
    repo_url: Mutex<Option<String>>,
}

impl VfoxPlugin {
    #[requires(!name.is_empty())]
    pub fn new(name: String, plugin_path: PathBuf) -> Self {
        let repo = Git::new(&plugin_path);
        Self {
            name,
            full: None,
            repo_url: Mutex::new(None),
            repo: Mutex::new(repo),
            plugin_path,
        }
    }

    fn repo(&self) -> MutexGuard<'_, Git> {
        self.repo.lock().unwrap()
    }

    fn get_repo_url(&self, config: &Config) -> eyre::Result<Url> {
        if let Some(url) = self.repo_url.lock().unwrap().clone() {
            return Ok(Url::parse(&url)?);
        }
        if let Some(url) = self.repo().get_remote_url() {
            return Ok(Url::parse(&url)?);
        }
        if let Some(url) = config.get_repo_url(&self.name) {
            return Ok(Url::parse(&url)?);
        }
        let url = self
            .full
            .as_ref()
            .unwrap_or(&self.name)
            .split_once(':')
            .map(|f| f.1)
            .unwrap_or(&self.name);
        vfox_to_url(url)
    }

    pub async fn mise_env(
        &self,
        opts: &toml::Value,
        env: &IndexMap<String, String>,
        config_root: Option<&Path>,
    ) -> Result<Option<MiseEnvResponse>> {
        let (vfox, _) = self.vfox()?;
        let result = vfox
            .mise_env(&self.name, opts, env, config_root.and_then(|p| p.to_str()))
            .await?;
        let mut result_env = indexmap!();
        for ek in result.env {
            result_env.insert(ek.key, ek.value);
        }
        Ok(Some(MiseEnvResponse {
            env: result_env,
            cacheable: result.cacheable,
            watch_files: result.watch_files,
            redact: result.redact,
        }))
    }

    pub async fn mise_path(
        &self,
        opts: &toml::Value,
        env: &IndexMap<String, String>,
        config_root: Option<&Path>,
    ) -> Result<Option<Vec<String>>> {
        let (vfox, _) = self.vfox()?;
        let mut out = vec![];
        let results = vfox
            .mise_path(&self.name, opts, env, config_root.and_then(|p| p.to_str()))
            .await?;
        for entry in results {
            out.push(entry);
        }
        Ok(Some(out))
    }

    pub fn vfox(&self) -> Result<(Vfox, mpsc::Receiver<String>)> {
        let settings = Settings::get();
        let env_type = if settings.os() == "linux" {
            settings.libc().map(str::to_string)
        } else {
            None
        };
        let mut vfox = Vfox::new();
        vfox.runtime_env_type = env_type;
        vfox.plugin_dir = dirs::PLUGINS.to_path_buf();
        vfox.cache_dir = dirs::CACHE.to_path_buf();
        vfox.download_dir = dirs::DOWNLOADS.to_path_buf();
        vfox.install_dir = dirs::INSTALLS.to_path_buf();
        vfox.default_inline_shell = Some(settings.default_inline_shell()?);
        // Resolve the GitHub token lazily — only when a Lua plugin actually
        // makes an HTTP request to a GitHub API URL. This avoids spawning
        // `github.credential_command` (or hitting other token sources) for
        // operations that never need a token, like `mise hook-env` or shell
        // completion. `resolve_token` itself caches results per-process.
        vfox.github_token_resolver = Some(Arc::new(|| {
            crate::github::resolve_token("github.com").map(|(token, _)| token)
        }));
        let rx = vfox.log_subscribe();
        Ok((vfox, rx))
    }

    async fn install_from_zip(&self, url: &str, pr: &dyn SingleReport) -> eyre::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let temp_archive = temp_dir.path().join("archive.zip");
        HTTP.download_file(url, &temp_archive, Some(pr)).await?;

        pr.set_message("extracting zip file".to_string());

        let strip_components = file::should_strip_components(&temp_archive, file::TarFormat::Zip)?;

        file::unzip(
            &temp_archive,
            &self.plugin_path,
            &file::ZipOptions {
                strip_components: if strip_components { 1 } else { 0 },
            },
        )?;
        Ok(())
    }

    pub fn is_embedded(&self) -> bool {
        embedded_plugins::get_embedded_plugin(&self.name).is_some()
    }

    fn has_repo_url_override(&self) -> bool {
        self.repo_url.lock().unwrap().is_some()
    }
}

#[async_trait]
impl Plugin for VfoxPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> PathBuf {
        self.plugin_path.clone()
    }

    fn get_remote_url(&self) -> eyre::Result<Option<String>> {
        let url = self.repo().get_remote_url();
        Ok(url.or(self.repo_url.lock().unwrap().clone()))
    }

    fn set_remote_url(&self, url: String) {
        *self.repo_url.lock().unwrap() = Some(url);
    }

    fn current_abbrev_ref(&self) -> eyre::Result<Option<String>> {
        // No git ref for embedded plugins or if plugin_path doesn't exist
        if !self.plugin_path.exists() {
            return Ok(None);
        }
        self.repo().current_abbrev_ref().map(Some)
    }

    fn current_sha_short(&self) -> eyre::Result<Option<String>> {
        // No git sha for embedded plugins or if plugin_path doesn't exist
        if !self.plugin_path.exists() {
            return Ok(None);
        }
        self.repo().current_sha_short().map(Some)
    }

    fn remote_sha(&self) -> eyre::Result<Option<String>> {
        if !self.plugin_path.exists() {
            return Ok(None);
        }
        let branch = self.repo().current_branch()?;
        self.repo().remote_sha(&branch)
    }

    fn is_installed(&self) -> bool {
        // Embedded plugins are installed unless an explicit URL is being installed
        // as a filesystem override.
        (self.is_embedded() && !self.has_repo_url_override()) || self.plugin_path.exists()
    }

    fn is_installed_err(&self) -> eyre::Result<()> {
        if self.is_installed() {
            return Ok(());
        }
        Err(eyre!("asdf plugin {} is not installed", self.name())
            .wrap_err("run with --yes to install plugin automatically"))
    }

    async fn ensure_installed(
        &self,
        config: &Arc<Config>,
        mpr: &MultiProgressReport,
        force: bool,
        dry_run: bool,
    ) -> Result<()> {
        // Skip installation for embedded plugins unless an explicit URL is being
        // installed as a filesystem override.
        if self.is_embedded() && !self.has_repo_url_override() {
            return Ok(());
        }

        let settings = Settings::try_get()?;
        if !force {
            if self.is_installed() {
                return Ok(());
            }
            if !settings.yes && self.repo_url.lock().unwrap().is_none() {
                let url = self.get_repo_url(config)?;
                let url_string = url.to_string();
                if !registry::is_trusted_plugin(self.name(), &url_string) {
                    warn!(
                        "⚠️ {} is a community-developed plugin – {}",
                        style(&self.name).blue(),
                        style(&url_string.trim_end_matches(".git")).yellow()
                    );
                    if settings.paranoid {
                        bail!(
                            "Paranoid mode is enabled, refusing to install community-developed plugin"
                        );
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
        let pr = mpr.add_with_options(&prefix, dry_run);
        if !dry_run {
            let _lock = lock_file::get(&self.plugin_path, force)?;
            self.install(config, pr.as_ref()).await?;
            let plugin_type =
                PluginType::from_plugin_path(&self.plugin_path).unwrap_or(PluginType::Vfox);
            install_state::add_plugin(&self.name, plugin_type).await?;
            backend::remove(&self.name);
            warn_if_env_plugin_shadows_registry(&self.name, &self.plugin_path);
        }
        Ok(())
    }

    async fn update(&self, pr: &dyn SingleReport, gitref: Option<String>) -> Result<()> {
        // If only embedded (no filesystem plugin), warn that it can't be updated
        if self.is_embedded() && !self.plugin_path.exists() {
            warn!(
                "plugin:{} is embedded in mise, not updating",
                style(&self.name).blue().for_stderr()
            );
            pr.finish_with_message("embedded plugin".into());
            return Ok(());
        }

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

    async fn uninstall(&self, pr: &dyn SingleReport) -> Result<()> {
        if !self.is_installed() {
            return Ok(());
        }
        // If only embedded (no filesystem plugin), warn that it can't be uninstalled
        if self.is_embedded() && !self.plugin_path.exists() {
            warn!(
                "plugin:{} is embedded in mise, cannot uninstall",
                style(&self.name).blue().for_stderr()
            );
            pr.finish_with_message("embedded plugin".into());
            return Ok(());
        }
        pr.set_message("uninstall".into());

        remove_all_with_progress(&self.plugin_path, pr)?;

        Ok(())
    }

    async fn install(&self, config: &Arc<Config>, pr: &dyn SingleReport) -> eyre::Result<()> {
        let repository = self.get_repo_url(config)?;
        let source = PluginSource::parse(repository.as_str());
        debug!("vfox_plugin[{}]:install {:?}", self.name, repository);

        if self.is_installed() {
            self.uninstall(pr).await?;
        }

        match source {
            PluginSource::Zip { url } => {
                self.install_from_zip(&url, pr).await?;
                pr.finish_with_message(url.to_string());
                Ok(())
            }
            PluginSource::Git {
                url: repo_url,
                git_ref,
            } => {
                if regex!(r"^[/~]").is_match(&repo_url) {
                    Err(eyre!(
                        r#"Invalid repository URL: {repo_url}
If you are trying to link to a local directory, use `mise plugins link` instead.
Plugins could support local directories in the future but for now a symlink is required which `mise plugins link` will create for you."#
                    ))?;
                }
                let git = Git::new(&self.plugin_path);
                pr.set_message(format!("clone {repo_url}"));
                git.clone(&repo_url, CloneOptions::default().pr(pr))?;
                if let Some(ref_) = &git_ref {
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
    }
}

fn vfox_to_url(name: &str) -> eyre::Result<Url> {
    let name = name.strip_prefix("vfox:").unwrap_or(name);
    if let Some(rt) = registry::REGISTRY.get(name.trim_start_matches("vfox-")) {
        // bun -> version-fox/vfox-bun
        if let Some((_, tool_name)) = rt.backends.iter().find_map(|f| f.full.split_once("vfox:")) {
            return vfox_to_url(tool_name);
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
