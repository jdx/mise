use std::collections::HashMap;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::fs::{remove_file, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr};
use console::style;
use once_cell::sync::Lazy;

use crate::cache::CacheManager;
use crate::config::Config;
use crate::config::Settings;
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::file::{create_dir_all, display_path, remove_all_with_warning};
use crate::hash::hash_to_str;
use crate::lock_file::LockFile;
use crate::plugins::Script::{Download, ExecEnv, Install};
use crate::plugins::{Plugin, Script, ScriptManager};
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::toolset::{ToolVersion, ToolVersionType};
use crate::ui::progress_report::{ProgressReport, PROG_TEMPLATE};
use crate::{dirs, env, fake_asdf, file};

/// These represent individual plugin@version pairs of runtimes
/// installed to ~/.local/share/rtx/runtimes
#[derive(Debug, Clone)]
pub struct RuntimeVersion {
    pub version: String,
    pub plugin: Arc<Plugin>,
    pub install_path: PathBuf,
    download_path: PathBuf,
    cache_path: PathBuf,
    script_man: ScriptManager,
    list_bin_paths_cache: CacheManager<Vec<PathBuf>>,
    exec_env_cache: CacheManager<HashMap<String, String>>,
}

impl RuntimeVersion {
    pub fn new(config: &Config, plugin: Arc<Plugin>, version: String, tv: ToolVersion) -> Self {
        let install_path = match &tv.r#type {
            ToolVersionType::Path(p) => p.clone(),
            _ => dirs::INSTALLS.join(&plugin.name).join(&version),
        };
        let download_path = match &tv.r#type {
            ToolVersionType::Path(p) => p.clone(),
            _ => dirs::DOWNLOADS.join(&plugin.name).join(&version),
        };
        let cache_path = match &tv.r#type {
            ToolVersionType::Path(p) => dirs::CACHE.join(&plugin.name).join(hash_to_str(&p)),
            _ => dirs::CACHE.join(&plugin.name).join(&version),
        };
        let script_man = build_script_man(
            config,
            &tv,
            &version,
            &plugin.plugin_path,
            &install_path,
            &download_path,
        );
        let list_bin_paths_cache =
            Self::list_bin_paths_cache(config, &tv, &plugin, &install_path, &cache_path).unwrap();
        let exec_env_cache =
            Self::exec_env_cache(config, &tv, &plugin, &install_path, &cache_path).unwrap();

        Self {
            script_man,
            list_bin_paths_cache,
            exec_env_cache,
            cache_path,
            download_path,
            install_path,
            version,
            plugin,
        }
    }

    fn list_bin_paths_cache(
        config: &Config,
        tv: &ToolVersion,
        plugin: &Arc<Plugin>,
        install_path: &Path,
        cache_path: &Path,
    ) -> Result<CacheManager<Vec<PathBuf>>> {
        let list_bin_paths_filename = match &plugin.toml.list_bin_paths.cache_key {
            Some(key) => {
                let key = key.join("-");
                let key = Self::parse_template(config, tv, &key)?;
                let filename = format!("{}.msgpack.z", key);
                cache_path.join("list_bin_paths").join(filename)
            }
            None => cache_path.join("list_bin_paths.msgpack.z"),
        };
        let cm = CacheManager::new(list_bin_paths_filename)
            .with_fresh_file(dirs::ROOT.clone())
            .with_fresh_file(plugin.plugin_path.clone())
            .with_fresh_file(install_path.to_path_buf());
        Ok(cm)
    }
    fn exec_env_cache(
        config: &Config,
        tv: &ToolVersion,
        plugin: &Arc<Plugin>,
        install_path: &Path,
        cache_path: &Path,
    ) -> Result<CacheManager<HashMap<String, String>>> {
        let exec_env_filename = match &plugin.toml.exec_env.cache_key {
            Some(key) => {
                let key = key.join("-");
                let key = Self::parse_template(config, tv, &key)?;
                let filename = format!("{}.msgpack.z", key);
                cache_path.join("exec_env").join(filename)
            }
            None => cache_path.join("exec_env.msgpack.z"),
        };
        let cm = CacheManager::new(exec_env_filename)
            .with_fresh_file(dirs::ROOT.clone())
            .with_fresh_file(plugin.plugin_path.clone())
            .with_fresh_file(install_path.to_path_buf());
        Ok(cm)
    }

    fn parse_template(config: &Config, tv: &ToolVersion, tmpl: &str) -> Result<String> {
        let mut ctx = BASE_CONTEXT.clone();
        ctx.insert("project_root", &config.project_root);
        ctx.insert("opts", &tv.options);
        get_tera(config.project_root.as_ref().unwrap_or(&*env::PWD))
            .render_str(tmpl, &ctx)
            .with_context(|| format!("failed to parse template: {}", tmpl))
    }

    pub fn install(&self, config: &Config, pr: &mut ProgressReport, force: bool) -> Result<()> {
        self.decorate_progress_bar(pr);

        let settings = &config.settings;
        let _lock = self.get_lock(force, pr)?;
        self.create_install_dirs()?;

        let run_script = |script| {
            self.script_man.run_by_line(
                settings,
                script,
                |output| {
                    self.cleanup_install_dirs_on_error(settings);
                    pr.error();
                    if !settings.verbose && !output.trim().is_empty() {
                        pr.println(output);
                    }
                },
                |line| {
                    if !line.trim().is_empty() {
                        pr.set_message(line.into());
                    }
                },
                |line| {
                    if !line.trim().is_empty() {
                        pr.println(line.into());
                    }
                },
            )
        };

        if self.script_man.script_exists(&Download) {
            pr.set_message("downloading".into());
            run_script(&Download)?;
        }
        pr.set_message("installing".into());
        run_script(&Install)?;
        self.cleanup_install_dirs(settings);

        // attempt to touch all the .tool-version files to trigger updates in hook-env
        let mut touch_dirs = vec![dirs::ROOT.to_path_buf()];
        touch_dirs.extend(config.config_files.keys().cloned());
        for path in touch_dirs {
            let err = file::touch_dir(&path);
            if let Err(err) = err {
                debug!("error touching config file: {:?} {:?}", path, err);
            }
        }
        if let Err(err) = remove_file(self.incomplete_file_path()) {
            debug!("error removing incomplete file: {:?}", err);
        }
        pr.finish();

        Ok(())
    }

    pub fn list_bin_paths(&self, settings: &Settings) -> Result<&Vec<PathBuf>> {
        self.list_bin_paths_cache
            .get_or_try_init(|| self.fetch_bin_paths(settings))
    }

    pub fn which(&self, settings: &Settings, bin_name: &str) -> Result<Option<PathBuf>> {
        let bin_paths = self.list_bin_paths(settings)?;
        for bin_path in bin_paths {
            let bin_path = bin_path.join(bin_name);
            if bin_path.exists() {
                return Ok(Some(bin_path));
            }
        }
        Ok(None)
    }

    pub fn is_installed(&self) -> bool {
        match self.version.as_str() {
            "system" => true,
            _ => self.install_path.exists() && !self.incomplete_file_path().exists(),
        }
    }

    pub fn uninstall(&self, settings: &Settings, pr: &ProgressReport, dryrun: bool) -> Result<()> {
        pr.set_message(format!("uninstall {}", self));

        if self.plugin.plugin_path.join("bin/uninstall").exists() {
            self.script_man.run(settings, &Script::Uninstall)?;
        }
        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            pr.set_message(format!("removing {}", display_path(dir)));
            if dryrun {
                return Ok(());
            }
            remove_all_with_warning(dir)
        };
        rmdir(&self.install_path)?;
        rmdir(&self.download_path)?;
        Ok(())
    }

    pub fn exec_env(&self) -> Result<&HashMap<String, String>> {
        if self.version.as_str() == "system" {
            return Ok(&*EMPTY_HASH_MAP);
        }
        if !self.script_man.script_exists(&ExecEnv) || *env::__RTX_SCRIPT {
            // if the script does not exist or we're running from within a script already
            // the second is to prevent infinite loops
            return Ok(&*EMPTY_HASH_MAP);
        }
        self.exec_env_cache.get_or_try_init(|| {
            let script = self.script_man.get_script_path(&ExecEnv);
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
        })
    }

    fn fetch_bin_paths(&self, settings: &Settings) -> Result<Vec<PathBuf>> {
        let list_bin_paths = self.plugin.plugin_path.join("bin/list-bin-paths");
        let bin_paths = if list_bin_paths.exists() {
            let output = self
                .script_man
                .cmd(settings, &Script::ListBinPaths)
                .read()?;
            output.split_whitespace().map(|f| f.to_string()).collect()
        } else if self.version == "system" {
            vec![]
        } else {
            vec!["bin".into()]
        };
        let bin_paths = bin_paths
            .into_iter()
            .map(|path| self.install_path.join(path))
            .collect();
        Ok(bin_paths)
    }

    fn create_install_dirs(&self) -> Result<()> {
        let _ = remove_all_with_warning(&self.install_path);
        let _ = remove_all_with_warning(&self.download_path);
        let _ = remove_all_with_warning(&self.cache_path);
        let _ = remove_file(&self.install_path); // removes if it is a symlink
        create_dir_all(&self.install_path)?;
        create_dir_all(&self.download_path)?;
        create_dir_all(&self.cache_path)?;
        File::create(self.incomplete_file_path())?;
        Ok(())
    }

    fn cleanup_install_dirs_on_error(&self, settings: &Settings) {
        let _ = remove_all_with_warning(&self.install_path);
        self.cleanup_install_dirs(settings);
    }
    fn cleanup_install_dirs(&self, settings: &Settings) {
        if !settings.always_keep_download {
            let _ = remove_all_with_warning(&self.download_path);
        }
    }

    fn incomplete_file_path(&self) -> PathBuf {
        self.cache_path.join("incomplete")
    }

    pub fn decorate_progress_bar(&self, pr: &mut ProgressReport) {
        pr.set_style(PROG_TEMPLATE.clone());
        pr.set_prefix(format!(
            "{} {} ",
            style("rtx").dim().for_stderr(),
            style(&self.to_string()).cyan().for_stderr()
        ));
        pr.enable_steady_tick();
    }

    fn get_lock(&self, force: bool, _pr: &ProgressReport) -> Result<Option<fslock::LockFile>> {
        let lock = if force {
            None
        } else {
            let lock = LockFile::new(&self.install_path)
                .with_callback(|l| {
                    // pr.set_message(format!("waiting for lock on {}", display_path(l)));
                    debug!("waiting for lock on {}", display_path(l));
                })
                .lock()?;
            Some(lock)
        };
        Ok(lock)
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
    config: &Config,
    tv: &ToolVersion,
    version: &str,
    plugin_path: &Path,
    install_path: &Path,
    download_path: &Path,
) -> ScriptManager {
    let mut sm = ScriptManager::new(plugin_path.to_path_buf())
        .with_envs(env::PRISTINE_ENV.clone())
        .with_env("PATH".into(), fake_asdf::get_path_with_fake_asdf())
        .with_env(
            "ASDF_INSTALL_PATH".into(),
            install_path.to_string_lossy().to_string(),
        )
        .with_env(
            "RTX_INSTALL_PATH".into(),
            install_path.to_string_lossy().to_string(),
        )
        .with_env(
            "ASDF_DOWNLOAD_PATH".into(),
            download_path.to_string_lossy().to_string(),
        )
        .with_env(
            "RTX_DOWNLOAD_PATH".into(),
            download_path.to_string_lossy().to_string(),
        )
        .with_env("RTX_CONCURRENCY".into(), num_cpus::get().to_string())
        .with_env("ASDF_CONCURRENCY".into(), num_cpus::get().to_string())
        .with_env(
            "RTX_DATA_DIR".into(),
            dirs::ROOT.to_string_lossy().to_string(),
        )
        .with_env("__RTX_SCRIPT".into(), "1".into())
        .with_env("RTX_PLUGIN_NAME".into(), tv.plugin_name.clone())
        .with_env(
            "RTX_PLUGIN_PATH".into(),
            plugin_path.to_string_lossy().to_string(),
        );
    if let Some(shims_dir) = &config.settings.shims_dir {
        let shims_dir = shims_dir.to_string_lossy().to_string();
        sm = sm.with_env("RTX_SHIMS_DIR".into(), shims_dir);
    }
    if let Some(project_root) = &config.project_root {
        let project_root = project_root.to_string_lossy().to_string();
        sm = sm.with_env("RTX_PROJECT_ROOT".into(), project_root);
    }
    for (key, value) in tv.options.iter() {
        let k = format!("RTX_TOOL_OPTS__{}", key.to_uppercase());
        sm = sm.with_env(k, value.clone());
    }
    match &tv.r#type {
        ToolVersionType::Version(_) | ToolVersionType::Prefix(_) => sm
            .with_env("ASDF_INSTALL_TYPE".into(), "version".into())
            .with_env("RTX_INSTALL_TYPE".into(), "version".into())
            .with_env("ASDF_INSTALL_VERSION".into(), version.to_string())
            .with_env("RTX_INSTALL_VERSION".into(), version.to_string()),
        ToolVersionType::Ref(r) => sm
            .with_env("ASDF_INSTALL_TYPE".into(), "ref".into())
            .with_env("RTX_INSTALL_TYPE".into(), "ref".into())
            .with_env("ASDF_INSTALL_VERSION".into(), r.to_string())
            .with_env("RTX_INSTALL_VERSION".into(), r.to_string()),
        _ => sm,
    }
}

static EMPTY_HASH_MAP: Lazy<HashMap<String, String>> = Lazy::new(HashMap::new);
