use std::collections::HashMap;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};
use itertools::Itertools;

use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{create_dir_all, display_path};
use crate::git::Git;
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::plugins::Plugin;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::progress_report::ProgressReport;
use crate::{cmd, env, file, http};

#[derive(Debug)]
pub struct PythonPlugin {
    core: CorePlugin,
}

impl PythonPlugin {
    pub fn new() -> Self {
        Self {
            core: CorePlugin::new("python"),
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
        match self.core.fetch_remote_versions_from_rtx() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        self.install_or_update_python_build()?;
        let python_build_bin = self.python_build_bin();
        CorePlugin::run_fetch_task_with_timeout(move || {
            let output = cmd!(python_build_bin, "--definitions").read()?;
            let versions = output
                .split('\n')
                .map(|s| s.to_string())
                .sorted_by_cached_key(|v| regex!(r"^\d+").is_match(v))
                .collect();
            Ok(versions)
        })
    }

    fn python_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_short_path().join("bin/python")
    }

    fn install_default_packages(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        if !env::RTX_PYTHON_DEFAULT_PACKAGES_FILE.exists() {
            return Ok(());
        }
        pr.set_message("installing default packages");
        CmdLineRunner::new(tv.install_path().join("bin/python"))
            .with_pr(pr)
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--upgrade")
            .arg("-r")
            .arg(&*env::RTX_PYTHON_DEFAULT_PACKAGES_FILE)
            .envs(&config.env)
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
            if !virtualenv.exists() {
                info!("setting up virtualenv at: {}", virtualenv.display());
                let mut cmd = CmdLineRunner::new(self.python_path(tv))
                    .arg("-m")
                    .arg("venv")
                    .arg(&virtualenv)
                    .envs(&config.env);
                if let Some(pr) = pr {
                    cmd = cmd.with_pr(pr);
                }
                cmd.execute()?;
            }
            self.check_venv_python(&virtualenv, tv)?;
            Ok(Some(virtualenv))
        } else {
            Ok(None)
        }
    }

    fn check_venv_python(&self, virtualenv: &Path, tv: &ToolVersion) -> Result<()> {
        let symlink = virtualenv.join("bin/python");
        let target = self.python_path(tv);
        let symlink_target = symlink.read_link().unwrap_or_default();
        ensure!(
            symlink_target == target,
            "expected venv {} to point to {}.\nTry deleting the venv at {}.",
            display_path(&symlink),
            display_path(&target),
            display_path(virtualenv)
        );
        Ok(())
    }

    fn test_python(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        pr.set_message("python --version");
        CmdLineRunner::new(self.python_path(tv))
            .arg("--version")
            .envs(&config.env)
            .execute()
    }
}

impl Plugin for PythonPlugin {
    fn name(&self) -> &str {
        "python"
    }

    fn list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".python-version".to_string()])
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let config = Config::get();
        let settings = Settings::try_get()?;
        self.install_or_update_python_build()?;
        if matches!(&ctx.tv.request, ToolVersionRequest::Ref(..)) {
            return Err(eyre!("Ref versions not supported for python"));
        }
        ctx.pr.set_message("Running python-build");
        let mut cmd = CmdLineRunner::new(self.python_build_bin())
            .with_pr(&ctx.pr)
            .arg(ctx.tv.version.as_str())
            .arg(&ctx.tv.install_path())
            .envs(&config.env);
        if settings.verbose {
            cmd = cmd.arg("--verbose");
        }
        if let Some(patch_url) = &*env::RTX_PYTHON_PATCH_URL {
            ctx.pr
                .set_message(format!("with patch file from: {patch_url}"));
            let http = http::Client::new()?;
            let patch = http.get_text(patch_url)?;
            cmd = cmd.arg("--patch").stdin_string(patch)
        }
        if let Some(patches_dir) = &*env::RTX_PYTHON_PATCHES_DIRECTORY {
            let patch_file = patches_dir.join(format!("{}.patch", &ctx.tv.version));
            if patch_file.exists() {
                ctx.pr
                    .set_message(format!("with patch file: {}", patch_file.display()));
                let contents = file::read_to_string(&patch_file)?;
                cmd = cmd.arg("--patch").stdin_string(contents);
            } else {
                ctx.pr
                    .warn(format!("patch file not found: {}", patch_file.display()));
            }
        }
        cmd.execute()?;
        self.test_python(&config, &ctx.tv, &ctx.pr)?;
        if let Err(e) = self.get_virtualenv(&config, &ctx.tv, Some(&ctx.pr)) {
            warn!("failed to get virtualenv: {e}");
        }
        self.install_default_packages(&config, &ctx.tv, &ctx.pr)?;
        Ok(())
    }

    fn exec_env(
        &self,
        config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<HashMap<String, String>> {
        let hm = match self.get_virtualenv(config, tv, None) {
            Err(e) => {
                warn!("failed to get virtualenv: {e}");
                HashMap::new()
            }
            Ok(Some(virtualenv)) => HashMap::from([
                (
                    "VIRTUAL_ENV".to_string(),
                    virtualenv.to_string_lossy().to_string(),
                ),
                (
                    "RTX_ADD_PATH".to_string(),
                    virtualenv.join("bin").to_string_lossy().to_string(),
                ),
            ]),
            Ok(None) => HashMap::new(),
        };
        Ok(hm)
    }
}
