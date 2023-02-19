use std::collections::HashMap;

use std::fmt;
use std::fmt::{Display, Formatter};
use std::fs::{create_dir_all, remove_dir_all};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result, WrapErr};
use console::style;
use indicatif::ProgressStyle;
use once_cell::sync::Lazy;

use runtime_conf::RuntimeConf;

use crate::config::Config;
use crate::config::Settings;
use crate::env_diff::{EnvDiff, EnvDiffOperation};

use crate::plugins::{InstallType, Plugin, Script, ScriptManager};

use crate::ui::progress_report::ProgressReport;

use crate::{dirs, env, fake_asdf, file};

mod runtime_conf;

/// These represent individual plugin@version pairs of runtimes
/// installed to ~/.local/share/rtx/runtimes
#[derive(Debug, Clone)]
pub struct RuntimeVersion {
    pub version: String,
    pub plugin: Arc<Plugin>,
    pub install_path: PathBuf,
    download_path: PathBuf,
    runtime_conf_path: PathBuf,
    script_man: ScriptManager,
}

impl RuntimeVersion {
    pub fn new(plugin: Arc<Plugin>, version: &str) -> Self {
        let install_path = dirs::INSTALLS.join(&plugin.name).join(version);
        let download_path = dirs::DOWNLOADS.join(&plugin.name).join(version);
        Self {
            runtime_conf_path: install_path.join(".rtxconf.msgpack"),
            script_man: build_script_man(
                version,
                &plugin.plugin_path,
                &install_path,
                &download_path,
            ),
            download_path: dirs::DOWNLOADS.join(&plugin.name).join(version),
            install_path,
            version: version.into(),
            plugin,
        }
    }

    pub fn install(
        &self,
        install_type: InstallType,
        config: &Config,
        mut pr: ProgressReport,
    ) -> Result<()> {
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
        debug!("install {} {}", self, install_type);

        self.create_install_dirs()?;
        let download = Script::Download(install_type.clone());
        let install = Script::Install(install_type);

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
                    pr.set_message(line.into());
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

        let conf = RuntimeConf {
            bin_paths: self.get_bin_paths()?,
        };
        conf.write(&self.runtime_conf_path)?;

        // attempt to touch all the .tool-version files to trigger updates in hook-env
        let mut touch_dirs = vec![dirs::ROOT.to_path_buf()];
        touch_dirs.extend(config.config_files.iter().cloned());
        for path in touch_dirs {
            let err = file::touch_dir(&path);
            if let Err(err) = err {
                debug!("error touching config file: {:?} {:?}", path, err);
            }
        }
        pr.finish_with_message(style("✓").green().for_stderr().to_string());

        Ok(())
    }

    pub fn list_bin_paths(&self) -> Result<Vec<PathBuf>> {
        if self.version == "system" {
            return Ok(vec![]);
        }
        let conf = RuntimeConf::parse(&self.runtime_conf_path)
            .wrap_err_with(|| eyre!("failed to fetch runtimeconf for {}", self))?;
        let bin_paths = conf
            .bin_paths
            .iter()
            .map(|path| self.install_path.join(path))
            .collect();

        Ok(bin_paths)
    }

    pub fn is_installed(&self) -> bool {
        if self.version == "system" {
            return true;
        }
        self.runtime_conf_path.is_file()
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

    fn get_bin_paths(&self) -> Result<Vec<String>> {
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
    version: &str,
    plugin_path: &Path,
    install_path: &Path,
    download_path: &Path,
) -> ScriptManager {
    ScriptManager::new(plugin_path.to_path_buf())
        .with_envs(env::PRISTINE_ENV.clone())
        .with_env("PATH".into(), fake_asdf::get_path_with_fake_asdf())
        .with_env("ASDF_INSTALL_VERSION".into(), version.to_string())
        .with_env(
            "ASDF_INSTALL_PATH".into(),
            install_path.to_string_lossy().to_string(),
        )
        .with_env(
            "ASDF_DOWNLOAD_PATH".into(),
            download_path.to_string_lossy().to_string(),
        )
        .with_env("ASDF_CONCURRENCY".into(), num_cpus::get().to_string())
}
