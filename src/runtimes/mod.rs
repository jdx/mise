use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::fs::{create_dir_all, remove_dir_all};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result, WrapErr};
use owo_colors::{OwoColorize, Stream};

use runtime_conf::RuntimeConf;

use crate::config::Config;
use crate::config::{MissingRuntimeBehavior, Settings};
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::errors::Error::{PluginNotInstalled, VersionNotInstalled};
use crate::plugins::{InstallType, Plugin, Script, ScriptManager};
use crate::ui::prompt;
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

    pub fn list() -> Result<Vec<Self>> {
        let mut versions = vec![];
        for plugin in Plugin::list()? {
            let plugin = Arc::new(plugin);
            for version in file::dir_subdirs(&dirs::INSTALLS.join(&plugin.name))? {
                versions.push(Self::new(plugin.clone(), &version));
            }
        }
        versions.sort_by_cached_key(|rtv| versions::Mess::new(rtv.version.as_str()));
        Ok(versions)
    }

    pub fn install(&self, install_type: InstallType, config: &Config) -> Result<()> {
        let plugin = &self.plugin;
        let settings = &config.settings;
        debug!("install {} {} {}", plugin.name, self.version, install_type);

        if !self.plugin.ensure_installed(settings)? {
            return Err(PluginNotInstalled(self.plugin.name.clone()).into());
        }

        self.create_install_dirs()?;
        let download = Script::Download(install_type.clone());
        let install = Script::Install(install_type);

        if self.script_man.script_exists(&download) {
            self.script_man
                .cmd(download)
                .stdout_to_stderr()
                .run()
                .map_err(|err| {
                    self.cleanup_install_dirs(Some(&err), settings);
                    err
                })?;
        }

        self.script_man
            .cmd(install)
            .stdout_to_stderr()
            .run()
            .map_err(|err| {
                self.cleanup_install_dirs(Some(&err), settings);
                err
            })?;
        self.cleanup_install_dirs(None, settings);

        let conf = RuntimeConf {
            bin_paths: self.get_bin_paths()?,
        };
        conf.write(&self.runtime_conf_path)?;

        // attempt to touch all the .tool-version files to trigger updates in hook-env
        for path in &config.config_files {
            let err = file::touch_dir(path);
            if let Err(err) = err {
                debug!("error touching config file: {:?} {:?}", path, err);
            }
        }

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

    pub fn ensure_installed(&self, config: &Config) -> Result<bool> {
        if self.is_installed() || self.version == "system" {
            return Ok(true);
        }
        match config.settings.missing_runtime_behavior {
            MissingRuntimeBehavior::AutoInstall => {
                self.install(InstallType::Version, config)?;
                Ok(true)
            }
            MissingRuntimeBehavior::Prompt => {
                if prompt_for_install(&format!("{self}")) {
                    self.install(InstallType::Version, config)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            MissingRuntimeBehavior::Warn => {
                let plugin = self.plugin.name.clone();
                let version = self.version.clone();
                warn!("{}", VersionNotInstalled(plugin, version));
                Ok(false)
            }
            MissingRuntimeBehavior::Ignore => {
                let plugin = self.plugin.name.clone();
                let version = self.version.clone();
                debug!("{}", VersionNotInstalled(plugin, version));
                Ok(false)
            }
        }
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
                    dir.to_str()
                        .unwrap()
                        .if_supports_color(Stream::Stderr, |t| t.cyan())
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
        create_dir_all(&self.install_path)?;
        create_dir_all(&self.download_path)?;
        Ok(())
    }

    fn cleanup_install_dirs(&self, err: Option<&dyn Error>, settings: &Settings) {
        if err.is_some() {
            let _ = remove_dir_all(&self.install_path);
        }
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

fn prompt_for_install(thing: &str) -> bool {
    match prompt::is_tty() {
        true => {
            eprint!(
                "rtx: Would you like to install {}? [Y/n] ",
                thing.if_supports_color(Stream::Stderr, |s| s.bold())
            );
            matches!(prompt::prompt().to_lowercase().as_str(), "" | "y" | "yes")
        }
        false => false,
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
