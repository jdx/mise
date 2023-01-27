use std::collections::HashMap;
use std::env::{join_paths, split_paths};
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result, WrapErr};
use color_eyre::Report;
use indexmap::IndexMap;
use itertools::Itertools;
use rayon::prelude::*;

use crate::cli::args::runtime::RuntimeArg;
use crate::config::config_file::legacy_version::LegacyVersionFile;
use crate::config::config_file::rtxrc::RTXFile;
use crate::config::config_file::ConfigFile;
use crate::config::settings::Settings;
use crate::config::toolset::Toolset;
use crate::plugins::{Plugin, PluginName, PluginSource};
use crate::{dirs, env, file};

pub mod config_file;
pub mod settings;
mod toolset;

type AliasMap = IndexMap<PluginName, IndexMap<String, String>>;

#[derive(Debug)]
pub struct Config {
    pub settings: Settings,
    pub rtxrc: RTXFile,
    pub ts: Toolset,
    pub config_files: Vec<PathBuf>,
    pub aliases: AliasMap,
}

impl Config {
    pub fn load() -> Result<Self> {
        let rtxrc = load_rtxrc()?;
        let settings = rtxrc.settings();
        let mut ts = Toolset::default();
        load_installed_plugins(&mut ts)?;
        load_installed_runtimes(&mut ts)?;
        let legacy_filenames = load_legacy_filenames(&settings, &ts)?;
        let config_files = find_all_config_files(&legacy_filenames);
        load_config_files(&mut ts, &config_files, &legacy_filenames)?;
        load_runtime_env(&mut ts, env::vars().collect())?;
        let aliases = load_aliases(&settings, &ts)?;
        ts.resolve_all_versions(&aliases)?;

        let config = Self {
            settings,
            ts,
            config_files,
            aliases,
            rtxrc,
        };

        debug!("{}", &config);

        Ok(config)
    }

    pub fn env(&self) -> Result<HashMap<OsString, OsString>> {
        let mut env = HashMap::new();

        let new_envs = self
            .ts
            .list_current_installed_versions()
            .into_par_iter()
            .map(|p| p.exec_env())
            .collect::<Result<Vec<HashMap<OsString, OsString>>>>()?;
        for new_env in new_envs {
            env.extend(new_env);
        }
        env.insert("PATH".into(), self.build_env_path()?);
        Ok(env)
    }

    pub fn with_runtime_args(mut self, args: &[RuntimeArg]) -> Result<Self> {
        let args_by_plugin = &args.iter().group_by(|arg| arg.plugin.clone());
        for (plugin_name, args) in args_by_plugin {
            match self.ts.plugins.get(&plugin_name) {
                Some(plugin) => plugin,
                _ => {
                    let plugin = Plugin::load_ensure_installed(&plugin_name, &self.settings)?;
                    self.ts
                        .plugins
                        .entry(plugin_name.clone())
                        .or_insert_with(|| Arc::new(plugin))
                }
            };
            let args = args.collect_vec();
            let source = PluginSource::Argument(args[0].clone());
            let versions = args.iter().map(|arg| arg.version.clone()).collect();
            self.ts
                .set_current_runtime_versions(&plugin_name, versions, source)?;
        }
        if !args.is_empty() {
            self.ts.resolve_all_versions(&self.aliases)?;
        }
        Ok(self)
    }

    pub fn ensure_installed(&self) -> Result<()> {
        for rtv in self.ts.list_current_versions() {
            if rtv.plugin.is_installed() {
                rtv.ensure_installed(self)?;
            }
        }
        Ok(())
    }

    pub fn resolve_alias(&self, plugin: &str, version: String) -> String {
        if let Some(plugin_aliases) = self.aliases.get(plugin) {
            if let Some(alias) = plugin_aliases.get(&version) {
                return alias.clone();
            }
        }
        version
    }

    fn build_env_path(&self) -> Result<OsString> {
        let mut paths = self
            .ts
            .list_current_installed_versions()
            .into_par_iter()
            .map(|rtv| rtv.list_bin_paths())
            .collect::<Result<Vec<Vec<PathBuf>>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<PathBuf>>();

        for p in split_paths(env::PATH.deref()) {
            if p.starts_with(dirs::INSTALLS.deref()) {
                // ignore existing install directories from previous runs
                continue;
            }
            paths.push(p);
        }
        Ok(join_paths(paths)?)
    }
}

fn load_rtxrc() -> Result<RTXFile> {
    let settings_path = dirs::CONFIG.join("config.toml");
    let rtxrc = if !settings_path.exists() {
        trace!("settings does not exist {:?}", settings_path);
        RTXFile::init(&settings_path)
    } else {
        let rtxrc = RTXFile::from_file(&settings_path)
            .wrap_err_with(|| err_load_settings(&settings_path))?;
        trace!("Settings: {:#?}", rtxrc.settings());
        rtxrc
    };

    Ok(rtxrc)
}

fn load_installed_plugins(ts: &mut Toolset) -> Result<()> {
    let plugins = file::dir_subdirs(&dirs::PLUGINS)?
        .into_par_iter()
        .map(|p| {
            let plugin = Plugin::load(&p)?;
            Ok((p, Arc::new(plugin)))
        })
        .collect::<Result<Vec<_>>>()?;
    for (name, plugin) in plugins {
        ts.plugins.entry(name).or_insert(plugin);
    }
    Ok(())
}

fn load_installed_runtimes(ts: &mut Toolset) -> Result<()> {
    let plugin_versions = ts
        .list_plugins()
        .into_par_iter()
        .map(|p| Ok((p.clone(), p.list_installed_versions()?)))
        .collect::<Result<Vec<(Arc<Plugin>, Vec<String>)>>>()?;
    for (plugin, versions) in plugin_versions {
        ts.add_runtime_versions(&plugin.name, versions)?;
    }
    Ok(())
}

fn load_legacy_filenames(settings: &Settings, ts: &Toolset) -> Result<HashMap<String, PluginName>> {
    if !settings.legacy_version_file {
        return Ok(HashMap::new());
    }
    let filenames = ts
        .list_plugins()
        .into_par_iter()
        .map(|plugin| {
            let mut legacy_filenames = vec![];
            for filename in plugin.legacy_filenames()? {
                legacy_filenames.push((filename, plugin.name.clone()));
            }
            Ok(legacy_filenames)
        })
        .collect::<Result<Vec<Vec<(String, PluginName)>>>>()?
        .into_iter()
        .flatten()
        .collect::<HashMap<String, PluginName>>();
    Ok(filenames)
}

fn find_all_config_files(legacy_filenames: &HashMap<String, PluginName>) -> Vec<PathBuf> {
    let mut filenames = vec![
        // ".rtxrc.toml",
        // ".rtxrc",
        env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str(),
    ];
    for filename in legacy_filenames.keys() {
        filenames.push(filename.as_str());
    }
    filenames.reverse();

    let mut config_files = file::FindUp::new(&dirs::CURRENT, &filenames).collect::<Vec<_>>();

    let home_config = dirs::HOME.join(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
    if home_config.is_file() {
        config_files.push(home_config);
    }

    config_files
}

fn load_config_files(
    ts: &mut Toolset,
    config_files: &Vec<PathBuf>,
    legacy_filenames: &HashMap<String, PluginName>,
) -> Result<()> {
    let parsed_config_files = config_files
        .into_par_iter()
        .rev()
        .map(|path| {
            let filename = path.file_name().unwrap().to_string_lossy().to_string();
            match legacy_filenames.get(&filename) {
                Some(plugin) => {
                    let plugin = ts.find_plugin(plugin).unwrap();
                    let cf = LegacyVersionFile::parse(path.into(), &plugin)?;
                    Ok(Box::new(cf) as Box<dyn ConfigFile>)
                }
                None => config_file::parse(path),
            }
        })
        .collect::<Result<Vec<_>>>()?;

    for cf in parsed_config_files {
        let path = cf.get_path().to_path_buf();
        load_config_file(ts, cf)
            .with_context(|| eyre!("error loading file: {}", path.display()))?;
    }

    Ok(())
}

fn load_config_file(ts: &mut Toolset, cf: Box<dyn ConfigFile>) -> Result<()> {
    trace!("config file: {:#?}", cf);
    for (plugin, versions) in cf.plugins() {
        ts.set_current_runtime_versions(&plugin, versions.clone(), cf.source())?;
    }

    Ok(())
}

fn load_runtime_env(ts: &mut Toolset, env: HashMap<String, String>) -> Result<()> {
    for (k, v) in env {
        if k.starts_with("RTX_") && k.ends_with("_VERSION") {
            let plugin_name = k[4..k.len() - 8].to_lowercase();
            if let Some(plugin) = ts.find_plugin(&plugin_name) {
                if plugin.is_installed() {
                    let source = PluginSource::Environment(k, v.clone());
                    ts.set_current_runtime_versions(&plugin.name, vec![v], source)?;
                }
            }
        }
    }
    Ok(())
}

fn load_aliases(settings: &Settings, ts: &Toolset) -> Result<AliasMap> {
    let mut aliases = IndexMap::new();
    for plugin in ts.list_installed_plugins() {
        for (from, to) in plugin.list_aliases()? {
            aliases
                .entry(plugin.name.clone())
                .or_insert_with(IndexMap::new)
                .insert(from, to);
        }
    }

    for (plugin, plugin_aliases) in &settings.aliases {
        for (from, to) in plugin_aliases {
            aliases
                .entry(plugin.clone())
                .or_insert_with(IndexMap::new)
                .insert(from.clone(), to.clone());
        }
    }

    Ok(aliases)
}

fn err_load_settings(settings_path: &Path) -> Report {
    eyre!(
        "error loading settings from {}",
        settings_path.to_string_lossy()
    )
}

impl Display for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Config:")?;
        writeln!(f, "  Installed Plugins:")?;
        for plugin in self.ts.list_installed_plugins() {
            writeln!(f, "    {}", plugin.name)?;
        }
        writeln!(f, "  Active Versions:")?;
        for rtv in self.ts.list_current_versions() {
            writeln!(f, "    {rtv}")?;
        }
        Ok(())
    }
}
