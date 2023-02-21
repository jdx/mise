use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::{create_dir_all, remove_dir_all, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fmt, fs};

use color_eyre::eyre::{Result, WrapErr};
use console::style;
use indicatif::ProgressStyle;
use once_cell::sync::Lazy;

use crate::cache::CacheManager;
use crate::config::Config;
use crate::config::Settings;
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::hash::hash_to_str;
use crate::plugins::{InstallType, Plugin, Script, ScriptManager};
use crate::ui::progress_report::ProgressReport;
use crate::{dirs, env, fake_asdf, file};

/// These represent individual plugin@version pairs of runtimes
/// installed to ~/.local/share/rtx/runtimes
#[derive(Debug, Clone)]
pub struct RuntimeVersion {
    pub version: String,
    pub plugin: Arc<Plugin>,
    pub install_path: PathBuf,
    pub install_type: InstallType,
    download_path: PathBuf,
    script_man: ScriptManager,
    bin_paths_cache: CacheManager<Vec<String>>,
}

impl RuntimeVersion {
    pub fn new(plugin: Arc<Plugin>, install_type: InstallType) -> Self {
        let version = match &install_type {
            InstallType::Version(v) => v.to_string(),
            InstallType::Ref(r) => format!("ref-{r}"),
            InstallType::Path(p) => p.display().to_string(),
            InstallType::System => "system".into(),
        };
        let install_path = match &install_type {
            InstallType::Path(p) => p.clone(),
            _ => dirs::INSTALLS.join(&plugin.name).join(&version),
        };
        let download_path = match &install_type {
            InstallType::Path(p) => p.clone(),
            _ => dirs::DOWNLOADS.join(&plugin.name).join(&version),
        };
        let cache_path = match &install_type {
            InstallType::Path(p) => dirs::CACHE.join(&plugin.name).join(hash_to_str(&p)),
            _ => dirs::CACHE.join(&plugin.name).join(&version),
        };
        Self {
            script_man: build_script_man(
                install_type.clone(),
                &plugin.plugin_path,
                &install_path,
                &download_path,
            ),
            bin_paths_cache: CacheManager::new(cache_path.join("bin_paths.msgpack.zlib"))
                .with_fresh_file(install_path.clone()),
            download_path,
            install_path,
            version,
            plugin,
            install_type,
        }
    }

    pub fn install(&self, config: &Config, mut pr: ProgressReport) -> Result<()> {
        static PROG_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
            ProgressStyle::with_template("{prefix}{wide_msg} {spinner:.blue} {elapsed:.dim.italic}")
                .unwrap()
        });
        pr.set_style(PROG_TEMPLATE.clone());
        pr.set_prefix(format!(
            "{} {} ",
            style("rtx").dim().for_stderr(),
            style(&self.to_string()).cyan().for_stderr()
        ));
        pr.enable_steady_tick();

        let settings = &config.settings;
        debug!("install {} {}", self, self.install_type);

        self.create_install_dirs()?;
        let download = Script::Download(self.install_type.clone());
        let install = Script::Install(self.install_type.clone());

        let run_script = |script| {
            self.script_man.run_by_line(
                script,
                |output| {
                    self.cleanup_install_dirs_on_error(settings);
                    pr.finish_with_message(format!("error {}", style("✗").red().for_stderr()));
                    if !settings.verbose && !output.trim().is_empty() {
                        pr.println(output);
                    }
                },
                |line| {
                    if !line.trim().is_empty() {
                        pr.set_message(line.into());
                    }
                },
            )
        };

        if self.script_man.script_exists(&download) {
            pr.set_message("downloading".into());
            run_script(download)?;
        }
        pr.set_message("installing".into());
        run_script(install)?;
        self.cleanup_install_dirs(settings);

        // attempt to touch all the .tool-version files to trigger updates in hook-env
        let mut touch_dirs = vec![dirs::ROOT.to_path_buf()];
        touch_dirs.extend(config.config_files.iter().cloned());
        for path in touch_dirs {
            let err = file::touch_dir(&path);
            if let Err(err) = err {
                debug!("error touching config file: {:?} {:?}", path, err);
            }
        }
        if let Err(err) = fs::remove_file(self.incomplete_file_path()) {
            debug!("error removing .rtx-incomplete: {:?}", err);
        }
        pr.finish_with_message(style("✓").green().for_stderr().to_string());

        Ok(())
    }

    pub fn list_bin_paths(&self) -> Result<Vec<PathBuf>> {
        Ok(self
            .bin_paths_cache
            .get_or_try_init(|| self.fetch_bin_paths())?
            .iter()
            .map(|path| self.install_path.join(path))
            .collect())
    }

    pub fn is_installed(&self) -> bool {
        match &self.install_type {
            InstallType::System => true,
            InstallType::Path(p) => p.exists(),
            InstallType::Version(_) | InstallType::Ref(_) => {
                self.install_path.exists() && !self.incomplete_file_path().exists()
            }
        }
    }

    pub fn uninstall(&self) -> Result<()> {
        debug!("uninstall {} {}", self.plugin.name, self.version);
        if self.plugin.plugin_path.join("bin/uninstall").exists() {
            let err = self.script_man.run(Script::Uninstall);
            if err.is_err() {
                warn!("Failed to run uninstall script: {}", err.unwrap_err());
            }
        }
        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            remove_dir_all(dir).wrap_err_with(|| {
                format!(
                    "Failed to remove directory {}",
                    style(dir.to_str().unwrap()).cyan().for_stderr()
                )
            })
        };
        rmdir(&self.install_path)?;
        let err = rmdir(&self.download_path);
        if err.is_err() {
            warn!("Failed to remove download directory: {}", err.unwrap_err());
        }
        Ok(())
    }

    pub fn exec_env(&self) -> Result<HashMap<String, String>> {
        let script = self.plugin.plugin_path.join("bin/exec-env");
        if !self.is_installed() || !script.exists() {
            return Ok(HashMap::new());
        }
        let ed = EnvDiff::from_bash_script(&script, &self.script_man.env)?;
        let env = ed
            .to_patches()
            .into_iter()
            .filter_map(|p| match p {
                EnvDiffOperation::Add(key, value) => Some((key, value)),
                EnvDiffOperation::Change(key, value) => Some((key, value)),
                _ => None,
            })
            .collect();
        Ok(env)
    }

    fn fetch_bin_paths(&self) -> Result<Vec<String>> {
        let list_bin_paths = self.plugin.plugin_path.join("bin/list-bin-paths");
        if list_bin_paths.exists() {
            let output = self.script_man.cmd(Script::ListBinPaths).read()?;
            Ok(output.split_whitespace().map(|e| e.into()).collect())
        } else {
            Ok(vec!["bin".into()])
        }
    }

    fn create_install_dirs(&self) -> Result<()> {
        let _ = remove_dir_all(&self.install_path);
        let _ = remove_dir_all(&self.download_path);
        create_dir_all(&self.install_path)?;
        create_dir_all(&self.download_path)?;
        File::create(self.incomplete_file_path())?;
        Ok(())
    }

    fn cleanup_install_dirs_on_error(&self, settings: &Settings) {
        let _ = remove_dir_all(&self.install_path);
        self.cleanup_install_dirs(settings);
    }
    fn cleanup_install_dirs(&self, settings: &Settings) {
        if !settings.always_keep_download {
            let _ = remove_dir_all(&self.download_path);
        }
    }

    fn incomplete_file_path(&self) -> PathBuf {
        self.install_path.join(".rtx-incomplete")
    }
}

impl Display for RuntimeVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.plugin.name, self.version)
    }
}

impl PartialEq for RuntimeVersion {
    fn eq(&self, other: &Self) -> bool {
        self.plugin.name == other.plugin.name && self.version == other.version
    }
}

fn build_script_man(
    install_type: InstallType,
    plugin_path: &Path,
    install_path: &Path,
    download_path: &Path,
) -> ScriptManager {
    let sm = ScriptManager::new(plugin_path.to_path_buf())
        .with_envs(env::PRISTINE_ENV.clone())
        .with_env("PATH".into(), fake_asdf::get_path_with_fake_asdf())
        .with_env(
            "ASDF_INSTALL_PATH".into(),
            install_path.to_string_lossy().to_string(),
        )
        .with_env(
            "ASDF_DOWNLOAD_PATH".into(),
            download_path.to_string_lossy().to_string(),
        )
        .with_env("ASDF_CONCURRENCY".into(), num_cpus::get().to_string());
    match install_type {
        InstallType::Version(v) => sm
            .with_env("ASDF_INSTALL_TYPE".into(), "version".into())
            .with_env("ASDF_INSTALL_VERSION".into(), v),
        InstallType::Ref(r) => sm
            .with_env("ASDF_INSTALL_TYPE".into(), "ref".into())
            .with_env("ASDF_INSTALL_VERSION".into(), r),
        _ => sm,
    }
}
