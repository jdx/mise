use std::collections::HashMap;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};

use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};

use crate::file::create_dir_all;
use crate::git::Git;
use crate::plugins::core::CorePlugin;
use crate::plugins::{Plugin, PluginName};
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::ProgressReport;
use crate::{cmd, env, file, http};

#[derive(Debug)]
pub struct PythonPlugin {
    core: CorePlugin,
}

impl PythonPlugin {
    pub fn new(name: PluginName) -> Self {
        Self {
            core: CorePlugin::new(name),
        }
    }

    fn python_build_path(&self) -> PathBuf {
        self.core.cache_path.join("pyenv")
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
        let python_build_path = self.python_build_path();
        debug!("Installing python-build to {}", python_build_path.display());
        create_dir_all(self.python_build_path().parent().unwrap())?;
        let git = Git::new(self.python_build_path());
        git.clone(&env::RTX_PYENV_REPO)?;
        Ok(())
    }
    fn update_python_build(&self) -> Result<()> {
        // TODO: do not update if recently updated
        debug!(
            "Updating python-build in {}",
            self.python_build_path().display()
        );
        let git = Git::new(self.python_build_path());
        CorePlugin::run_fetch_task_with_timeout(move || git.update(None))?;
        Ok(())
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        self.install_or_update_python_build()?;
        let python_build_bin = self.python_build_bin();
        CorePlugin::run_fetch_task_with_timeout(move || {
            let output = cmd!(python_build_bin, "--definitions").read()?;
            Ok(output.split('\n').map(|s| s.to_string()).collect())
        })
    }

    fn python_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/python")
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
        CmdLineRunner::new(settings, self.python_path(tv))
            .with_pr(pr)
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--upgrade")
            .arg("-r")
            .arg(&*env::RTX_PYTHON_DEFAULT_PACKAGES_FILE)
            .execute()
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
                info!("setting up virtualenv at: {}", virtualenv.display());
                let mut cmd = CmdLineRunner::new(&config.settings, self.python_path(tv))
                    .arg("-m")
                    .arg("venv")
                    .arg("--clear")
                    .arg(&virtualenv);
                if let Some(pr) = pr {
                    cmd = cmd.with_pr(pr);
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

    fn test_python(&self, config: &&Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        pr.set_message("python --version");
        CmdLineRunner::new(&config.settings, self.python_path(tv))
            .arg("--version")
            .execute()
    }
}

impl Plugin for PythonPlugin {
    fn name(&self) -> &PluginName {
        &self.core.name
    }

    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
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
        let mut cmd = CmdLineRunner::new(&config.settings, self.python_build_bin())
            .with_pr(pr)
            .arg(tv.version.as_str())
            .arg(tv.install_path());
        if config.settings.verbose {
            cmd = cmd.arg("--verbose");
        }
        if let Some(patch_url) = &*env::RTX_PYTHON_PATCH_URL {
            pr.set_message(format!("with patch file from: {patch_url}"));
            let http = http::Client::new()?;
            let resp = http.get(patch_url).send()?;
            http.ensure_success(&resp)?;
            let patch = resp.text()?;
            cmd = cmd.arg("--patch").stdin_string(patch)
        }
        if let Some(patches_dir) = &*env::RTX_PYTHON_PATCHES_DIRECTORY {
            let patch_file = patches_dir.join(format!("{}.patch", tv.version));
            if patch_file.exists() {
                pr.set_message(format!("with patch file: {}", patch_file.display()));
                let contents = file::read_to_string(&patch_file)?;
                cmd = cmd.arg("--patch").stdin_string(contents);
            } else {
                pr.warn(format!("patch file not found: {}", patch_file.display()));
            }
        }
        cmd.execute()?;
        self.test_python(&config, tv, pr)?;
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
