use color_eyre::eyre::{eyre, Result};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::cache::CacheManager;
use crate::config::{Config, Settings};
use crate::env::RTX_EXE;
use crate::file::create_dir_all;
use crate::git::Git;
use crate::{cmd, dirs};

use crate::plugins::{Plugin, PluginName};
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::ProgressReport;

#[derive(Debug)]
pub struct PythonPlugin {
    pub name: PluginName,
    cache_path: PathBuf,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl PythonPlugin {
    pub fn new(_settings: &Settings, name: PluginName) -> Self {
        let cache_path = dirs::CACHE.join(&name);
        let fresh_duration = Some(Duration::from_secs(60 * 60 * 12)); // 12 hours
        Self {
            remote_version_cache: CacheManager::new(cache_path.join("remote_versions.msgpack.z"))
                .with_fresh_duration(fresh_duration)
                .with_fresh_file(RTX_EXE.clone()),
            name,
            cache_path,
        }
    }

    fn python_build_path(&self) -> PathBuf {
        self.cache_path.join("pyenv")
    }
    fn python_build_bin(&self) -> PathBuf {
        self.python_build_path()
            .join("plugins/python-build/bin/python-build")
    }
    fn install_or_update_python_build(&self) -> Result<()> {
        if self.python_build_path().exists() {
            self.update_python_build()
        } else {
            self.install_python_build()
        }
    }
    fn install_python_build(&self) -> Result<()> {
        if self.python_build_path().exists() {
            return Ok(());
        }
        debug!(
            "Installing python-build to {}",
            self.python_build_path().display()
        );
        create_dir_all(self.python_build_path().parent().unwrap())?;
        let git = Git::new(self.python_build_path());
        git.clone("https://github.com/pyenv/pyenv.git")?;
        Ok(())
    }
    fn update_python_build(&self) -> Result<()> {
        // TODO: do not update if recently updated
        debug!(
            "Updating python-build in {}",
            self.python_build_path().display()
        );
        let git = Git::new(self.python_build_path());
        git.update(None)?;
        Ok(())
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        self.install_or_update_python_build()?;
        let output = cmd!(self.python_build_bin(), "--definitions").read()?;
        Ok(output.split('\n').map(|s| s.to_string()).collect())
    }
}

impl Plugin for PythonPlugin {
    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &mut ProgressReport,
    ) -> Result<()> {
        self.install_python_build()?;
        if matches!(tv.request, ToolVersionRequest::Ref(..)) {
            return Err(eyre!("Ref versions not supported for python"));
        }
        // TODO: patch support
        pr.set_message("running python-build".to_string());
        let mut cmd = Command::new(self.python_build_bin());
        cmd.arg(tv.version.as_str()).arg(tv.install_path());
        cmd::run_by_line_to_pr(&config.settings, cmd, pr)
    }
}
