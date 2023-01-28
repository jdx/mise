use std::collections::HashMap;
use std::error::Error;

use std::fmt;
use std::fmt::{Display, Formatter};
use std::fs::{create_dir_all, remove_dir_all, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result, WrapErr};
use duct::Expression;
use owo_colors::{OwoColorize, Stream};
use serde_derive::{Deserialize, Serialize};

use crate::config::settings::{MissingRuntimeBehavior, Settings};
use crate::config::Config;
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::errors::Error::{PluginNotInstalled, VersionNotInstalled};
use crate::plugins::Plugin;
use crate::ui::prompt;
use crate::{cmd, dirs, env, fake_asdf, file, ui};

#[derive(Debug, Clone)]
pub struct RuntimeVersion {
    pub version: String,
    pub plugin: Arc<Plugin>,
    pub install_path: PathBuf,
    download_path: PathBuf,
    runtime_conf_path: PathBuf,
}

impl RuntimeVersion {
    pub fn new(plugin: Arc<Plugin>, version: &str) -> Self {
        let install_path = dirs::INSTALLS.join(&plugin.name).join(version);
        Self {
            runtime_conf_path: install_path.join(".rtxconf.msgpack"),
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

    pub fn install(&self, install_type: &str, config: &Config) -> Result<()> {
        debug!(
            "install {} {} {}",
            self.plugin.name, self.version, install_type
        );

        let settings = &config.settings;
        if !self.plugin.ensure_installed(settings)? {
            return Err(PluginNotInstalled(self.plugin.name.clone()).into());
        }

        fake_asdf::setup(&fake_asdf::get_path(dirs::ROOT.as_path()))?;
        self.create_install_dirs()?;

        if self.plugin.plugin_path.join("bin/download").is_file() {
            self.run_script("download")
                .env("ASDF_INSTALL_TYPE", install_type)
                .stdout_to_stderr()
                .run()
                .map_err(|err| {
                    self.cleanup_install_dirs(Some(&err), settings);
                    err
                })?;
        }

        self.run_script("install")
            .env("ASDF_INSTALL_TYPE", install_type)
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
                self.install("version", config)?;
                Ok(true)
            }
            MissingRuntimeBehavior::Prompt => {
                if prompt_for_install(&format!("{self}")) {
                    self.install("version", config)?;
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
            let err = self.run_script("uninstall").run();
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
        let mut env: HashMap<String, String> = env::PRISTINE_ENV.clone();
        env.extend(self.script_env());
        let ed = EnvDiff::from_bash_script(&script, env)?;
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

    fn run_script(&self, script: &str) -> Expression {
        let mut cmd = cmd!(self.plugin.plugin_path.join("bin").join(script));
        for (k, v) in self.script_env() {
            cmd = cmd.env(k, v);
        }
        cmd
    }

    fn script_env(&self) -> HashMap<String, String> {
        let path = [
            fake_asdf::get_path(dirs::ROOT.as_path()).to_string_lossy(),
            env::PATH.to_string_lossy(),
        ]
        .join(":");
        return HashMap::from([
            ("RTX".into(), "1".into()),
            (
                "RTX_EXE".into(),
                env::RTX_EXE.as_path().to_string_lossy().into(),
            ),
            ("PATH".into(), path),
            ("ASDF_INSTALL_VERSION".into(), self.version.to_string()),
            (
                "ASDF_INSTALL_PATH".into(),
                self.install_path.to_string_lossy().into(),
            ),
            (
                "ASDF_DOWNLOAD_PATH".into(),
                self.download_path.to_string_lossy().into(),
            ),
            ("ASDF_CONCURRENCY".into(), num_cpus::get().to_string()),
        ]);
    }

    fn get_bin_paths(&self) -> Result<Vec<String>> {
        let list_bin_paths = self.plugin.plugin_path.join("bin/list-bin-paths");
        if list_bin_paths.exists() {
            let output = self.run_script("list-bin-paths").read()?;
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
    match ui::is_tty() {
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

#[derive(Debug, Serialize, Deserialize, Default)]
struct RuntimeConf {
    bin_paths: Vec<String>,
}

impl RuntimeConf {
    fn parse(path: &Path) -> Result<Self> {
        Ok(rmp_serde::from_read(File::open(path)?)?)
        // let contents = std::fs::read_to_string(path)
        //     .wrap_err_with(|| format!("failed to read {}", path.to_string_lossy()))?;
        // let conf: Self = toml::from_str(&contents)
        //     .wrap_err_with(|| format!("failed to from_file {}", path.to_string_lossy()))?;

        // Ok(conf)
    }

    fn write(&self, path: &Path) -> Result<()> {
        let bytes = rmp_serde::to_vec_named(self)?;
        File::create(path)?.write_all(&bytes)?;
        Ok(())
    }
}
