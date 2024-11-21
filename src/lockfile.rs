use crate::config::{Config, CONFIG, SETTINGS};
use crate::file;
use crate::file::display_path;
use crate::toolset::{ToolSource, ToolVersion, ToolVersionList, ToolsetBuilder};
use eyre::{bail, Report, Result};
use itertools::Itertools;
use once_cell::sync::Lazy;
use serde_derive::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    #[serde(skip)]
    tools: BTreeMap<String, Vec<LockfileTool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockfileTool {
    pub version: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub checksums: BTreeMap<String, String>,
}

impl Lockfile {
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self> {
        trace!("reading lockfile {}", display_path(&path));
        let content = file::read_to_string(path)?;
        let mut table: toml::Table = toml::from_str(&content)?;
        let tools: toml::Table = table
            .remove("tools")
            .unwrap_or(toml::Table::new().into())
            .try_into()?;
        let mut lockfile = Lockfile::default();
        for (short, value) in tools {
            let versions = match value {
                toml::Value::Array(arr) => arr
                    .into_iter()
                    .map(LockfileTool::try_from)
                    .collect::<Result<Vec<_>>>()?,
                _ => vec![LockfileTool::try_from(value)?],
            };
            lockfile.tools.insert(short, versions);
        }
        Ok(lockfile)
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        SETTINGS.ensure_experimental("lockfile")?;
        if self.is_empty() {
            let _ = file::remove_file(path);
        } else {
            let mut tools = toml::Table::new();
            for (short, versions) in &self.tools {
                let value: toml::Value = if versions.len() == 1 {
                    versions[0].clone().into_toml_value()
                } else {
                    versions
                        .iter()
                        .cloned()
                        .map(|version| version.into_toml_value())
                        .collect::<Vec<toml::Value>>()
                        .into()
                };
                tools.insert(short.clone(), value);
            }
            let mut lockfile = toml::Table::new();
            lockfile.insert("tools".to_string(), tools.into());
            let content = toml::to_string_pretty(&toml::Value::Table(lockfile))?;
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
    for (backend, group) in &new_versions.iter().chunk_by(|tv| tv.ba()) {
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
            .retain(|k, _| all_tool_names.contains(k) || SETTINGS.disable_tools().contains(k));

        for (short, tvl) in tools {
            existing_lockfile
                .tools
                .insert(short.to_string(), tvl.clone().into());
        }

        existing_lockfile.save(&lockfile_path)?;
    }

    Ok(())
}

fn read_all_lockfiles() -> Lockfile {
    CONFIG
        .config_files
        .keys()
        .rev()
        .map(|cf| read_lockfile_for(cf))
        .filter_map(|l| match l {
            Ok(l) => Some(l),
            Err(err) => {
                warn!("failed to read lockfile: {err}");
                None
            }
        })
        .fold(Lockfile::default(), |mut acc, l| {
            for (short, tvl) in l.tools {
                acc.tools.insert(short, tvl);
            }
            acc
        })
}

fn read_lockfile_for(path: &Path) -> Result<Lockfile> {
    static CACHE: Lazy<Mutex<HashMap<PathBuf, Lockfile>>> = Lazy::new(Default::default);
    let mut cache = CACHE.lock().unwrap();
    cache.entry(path.to_path_buf()).or_insert_with(|| {
        Lockfile::read(path.with_extension("lock"))
            .unwrap_or_else(|err| handle_missing_lockfile(err, &path.with_extension("lock")))
    });
    let lockfile = cache.get(path).unwrap().clone();
    Ok(lockfile)
}

pub fn get_locked_version(
    path: Option<&Path>,
    short: &str,
    prefix: &str,
) -> Result<Option<LockfileTool>> {
    if !SETTINGS.lockfile {
        return Ok(None);
    }
    SETTINGS.ensure_experimental("lockfile")?;

    let lockfile = match path {
        Some(path) => {
            trace!(
                "[{short}@{prefix}] reading lockfile for {}",
                display_path(path)
            );
            read_lockfile_for(path)?
        }
        None => {
            trace!("[{short}@{prefix}] reading all lockfiles");
            read_all_lockfiles()
        }
    };

    if let Some(tool) = lockfile.tools.get(short) {
        Ok(tool
            .iter()
            // TODO: this likely won't work right when using `python@latest python@3.12`
            .find(|v| prefix == "latest" || v.version.starts_with(prefix))
            .inspect(|v| trace!("[{short}@{prefix}] found {} in lockfile", v.version))
            .cloned())
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

impl TryFrom<toml::Value> for LockfileTool {
    type Error = Report;
    fn try_from(value: toml::Value) -> Result<Self> {
        let tool = match value {
            toml::Value::String(v) => LockfileTool {
                version: v,
                checksums: Default::default(),
            },
            toml::Value::Table(mut t) => {
                let mut checksums = BTreeMap::new();
                if let Some(checksums_table) = t.remove("checksums") {
                    let checksums_table: toml::Table = checksums_table.try_into()?;
                    for (filename, checksum) in checksums_table {
                        checksums.insert(filename, checksum.try_into()?);
                    }
                }
                LockfileTool {
                    version: t
                        .remove("version")
                        .map(|v| v.try_into())
                        .transpose()?
                        .unwrap_or_default(),
                    checksums,
                }
            }
            _ => bail!("unsupported lockfile format {}", value),
        };
        Ok(tool)
    }
}

impl LockfileTool {
    fn into_toml_value(self) -> toml::Value {
        let mut table = toml::Table::new();
        table.insert("version".to_string(), self.version.into());
        if !self.checksums.is_empty() {
            table.insert("checksums".to_string(), self.checksums.into());
        }
        table.into()
    }
}

impl From<ToolVersionList> for Vec<LockfileTool> {
    fn from(tvl: ToolVersionList) -> Self {
        tvl.versions
            .iter()
            .map(|tv| LockfileTool {
                version: tv.version.clone(),
                checksums: tv.checksums.clone(),
            })
            .collect()
    }
}
