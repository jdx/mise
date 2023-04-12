use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{eyre, Result};

use crate::cache::CacheManager;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::RTX_EXE;
use crate::file::create_dir_all;
use crate::git::Git;
use crate::plugins::{Plugin, PluginName};
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::ProgressReport;
use crate::{cmd, dirs, env, file, http};

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

    fn python_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/python")
    }

    fn pip_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/pip")
    }

    fn install_default_packages(
        &self,
        settings: &Settings,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        if !env::RTX_PYTHON_DEFAULT_PACKAGES_FILE.exists() {
            return Ok(());
        }
        pr.set_message("installing default packages");
        let pip = self.pip_path(tv);
        let mut cmd = CmdLineRunner::new(settings, pip);
        cmd.with_pr(pr)
            .arg("install")
            .arg("--upgrade")
            .arg("-r")
            .arg(&*env::RTX_PYTHON_DEFAULT_PACKAGES_FILE);
        cmd.execute()
    }

    fn get_virtualenv(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: Option<&ProgressReport>,
    ) -> Result<Option<PathBuf>> {
        if let Some(virtualenv) = tv.opts.get("virtualenv") {
            let mut virtualenv: PathBuf = file::replace_path(Path::new(virtualenv));
            if !virtualenv.is_absolute() {
                // TODO: use the path of the config file that specified python, not the top one like this
                if let Some(project_root) = &config.project_root {
                    virtualenv = project_root.join(virtualenv);
                }
            }
            if !virtualenv.exists() || !self.check_venv_python(&virtualenv, tv)? {
                debug!("setting up virtualenv at: {}", virtualenv.display());
                let mut cmd = CmdLineRunner::new(&config.settings, self.python_path(tv));
                cmd.arg("-m").arg("venv").arg("--clear").arg(&virtualenv);
                if let Some(pr) = pr {
                    cmd.with_pr(pr);
                }
                cmd.execute()?;
            }
            Ok(Some(virtualenv))
        } else {
            Ok(None)
        }
    }

    fn check_venv_python(&self, virtualenv: &Path, tv: &ToolVersion) -> Result<bool> {
        let symlink = virtualenv.join("bin/python");
        let target = tv.install_path().join("bin/python");
        let symlink_target = symlink.read_link().unwrap_or_default();
        Ok(symlink_target == target)
    }
}

impl Plugin for PythonPlugin {
    fn name(&self) -> &PluginName {
        &self.name
    }

    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn legacy_filenames(&self, _settings: &Settings) -> Result<Vec<String>> {
        Ok(vec![".python-version".to_string()])
    }

    fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        self.install_python_build()?;
        if matches!(tv.request, ToolVersionRequest::Ref(..)) {
            return Err(eyre!("Ref versions not supported for python"));
        }
        pr.set_message("running python-build");
        let mut cmd = CmdLineRunner::new(&config.settings, self.python_build_bin());
        cmd.with_pr(pr)
            .arg(tv.version.as_str())
            .arg(tv.install_path());
        if let Some(patch_url) = &*env::RTX_PYTHON_PATCH_URL {
            pr.set_message(format!("with patch file from: {patch_url}"));
            cmd.arg("--patch");
            let http = http::Client::new()?;
            let patch = http.get(patch_url).send()?.text()?;
            cmd.stdin_string(patch);
        }
        if let Some(patches_dir) = &*env::RTX_PYTHON_PATCHES_DIRECTORY {
            dbg!(patches_dir);
            let patch_file = patches_dir.join(format!("{}.patch", tv.version));
            if patch_file.exists() {
                pr.set_message(format!("with patch file: {}", patch_file.display()));
                cmd.arg("--patch");
                let contents = std::fs::read_to_string(&patch_file)?;
                cmd.stdin_string(contents);
            } else {
                pr.warn(format!("patch file not found: {}", patch_file.display()));
            }
        }
        cmd.execute()?;
        self.get_virtualenv(config, tv, Some(pr))?;
        self.install_default_packages(&config.settings, tv, pr)?;
        Ok(())
    }

    fn list_bin_paths(&self, config: &Config, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        if let Some(virtualenv) = self.get_virtualenv(config, tv, None)? {
            Ok(vec![virtualenv.join("bin"), tv.install_path().join("bin")])
        } else {
            Ok(vec![tv.install_path().join("bin")])
        }
    }

    fn exec_env(&self, config: &Config, tv: &ToolVersion) -> Result<HashMap<String, String>> {
        if let Some(virtualenv) = self.get_virtualenv(config, tv, None)? {
            let hm = HashMap::from([(
                "VIRTUAL_ENV".to_string(),
                virtualenv.to_string_lossy().to_string(),
            )]);
            Ok(hm)
        } else {
            Ok(HashMap::new())
        }
    }
}
