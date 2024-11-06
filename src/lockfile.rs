use crate::config::{Config, SETTINGS};
use crate::file;
use crate::file::display_path;
use crate::toolset::{ToolSource, ToolVersion, ToolVersionList, ToolsetBuilder};
use eyre::{Report, Result};
use itertools::Itertools;
use once_cell::sync::Lazy;
use serde_derive::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    tools: BTreeMap<String, toml::Value>,
}

impl Lockfile {
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = file::read_to_string(path)?;
        let lockfile: Lockfile = toml::from_str(&content)?;
        Ok(lockfile)
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        SETTINGS.ensure_experimental("lockfile")?;
        if self.is_empty() {
            let _ = file::remove_file(path);
        } else {
            let content = toml::to_string_pretty(self)?;
            file::write(path, content)?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

pub fn update_lockfiles(new_versions: &[ToolVersion]) -> Result<()> {
    if !SETTINGS.lockfile {
        return Ok(());
    }
    SETTINGS.ensure_experimental("lockfile")?;
    let config = Config::load()?;
    let ts = ToolsetBuilder::new().build(&config)?;
    let mut all_tool_names = HashSet::new();
    let mut tools_by_source = HashMap::new();
    for (source, group) in &ts.versions.iter().chunk_by(|(_, tvl)| &tvl.source) {
        for (ba, tvl) in group {
            tools_by_source
                .entry(source.clone())
                .or_insert_with(HashMap::new)
                .insert(ba.short.to_string(), tvl.clone());
            all_tool_names.insert(ba.short.to_string());
        }
    }

    // add versions added within this session such as from `mise use` or `mise up`
    for (backend, group) in &new_versions.iter().chunk_by(|tv| &tv.backend) {
        let tvs = group.cloned().collect_vec();
        let source = tvs[0].request.source().clone();
        let mut tvl = ToolVersionList::new(backend.clone(), source.clone());
        tvl.versions.extend(tvs);
        tools_by_source
            .entry(source)
            .or_insert_with(HashMap::new)
            .insert(backend.short.to_string(), tvl);
    }

    let lockfiles = config.config_files.keys().rev().collect_vec();
    debug!("updating {} lockfiles", lockfiles.len());

    let empty = HashMap::new();
    for config_path in lockfiles {
        let lockfile_path = config_path.with_extension("lock");
        if !lockfile_path.exists() {
            continue;
        }
        let tool_source = ToolSource::MiseToml(config_path.clone());
        let tools = tools_by_source.get(&tool_source).unwrap_or(&empty);
        trace!(
            "updating {} tools in lockfile {}",
            tools.len(),
            display_path(&lockfile_path)
        );
        let mut existing_lockfile = Lockfile::read(&lockfile_path)
            .unwrap_or_else(|err| handle_missing_lockfile(err, &lockfile_path));

        // there are tools that should remain in the lockfile even though they're not in this current toolset
        // * tools that are disabled via settings
        // * tools inside a parent config but are overridden by a child config (we just keep what was in the lockfile before, if anything)
        existing_lockfile
            .tools
            .retain(|k, _| all_tool_names.contains(k) || SETTINGS.disable_tools.contains(k));

        for (short, tvl) in tools {
            if tvl.versions.len() > 1 {
                let versions = toml::Value::Array(
                    tvl.versions
                        .iter()
                        .map(|tv| tv.version.clone().into())
                        .collect(),
                );
                existing_lockfile.tools.insert(short.to_string(), versions);
            } else {
                existing_lockfile.tools.insert(
                    short.to_string(),
                    toml::Value::String(tvl.versions[0].version.clone()),
                );
            }
        }

        existing_lockfile.save(&lockfile_path)?;
    }

    Ok(())
}

pub fn get_locked_version(path: &Path, short: &str, prefix: &str) -> Result<Option<String>> {
    static CACHE: Lazy<Mutex<HashMap<PathBuf, Lockfile>>> = Lazy::new(Default::default);

    if !SETTINGS.lockfile {
        return Ok(None);
    }
    SETTINGS.ensure_experimental("lockfile")?;

    let mut cache = CACHE.lock().unwrap();
    let lockfile = cache.entry(path.to_path_buf()).or_insert_with(|| {
        let lockfile_path = path.with_extension("lock");
        Lockfile::read(&lockfile_path)
            .unwrap_or_else(|err| handle_missing_lockfile(err, &lockfile_path))
    });

    if let Some(tool) = lockfile.tools.get(short) {
        // TODO: handle something like `mise use python@3 python@3.1`
        match tool {
            toml::Value::String(v) => {
                if v.starts_with(prefix) {
                    Ok(Some(v.clone()))
                } else {
                    Ok(None)
                }
            }
            toml::Value::Array(v) => Ok(v
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .find(|v| v.starts_with(prefix))),
            _ => unimplemented!("unsupported lockfile format"),
        }
    } else {
        Ok(None)
    }
}

fn handle_missing_lockfile(err: Report, lockfile_path: &Path) -> Lockfile {
    if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
        if io_err.kind() != std::io::ErrorKind::NotFound {
            warn!(
                "failed to read lockfile {}: {err:?}",
                display_path(lockfile_path)
            );
        }
    }
    Lockfile::default()
}
