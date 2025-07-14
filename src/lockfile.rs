use crate::config::{Config, Settings};
use crate::file;
use crate::file::display_path;
use crate::path::PathExt;
use crate::registry::{REGISTRY, tool_enabled};
use crate::toolset::{ToolSource, ToolVersion, ToolVersionList, Toolset};
use eyre::{Report, Result, bail};
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::sync::Mutex;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};
use toml_edit::DocumentMut;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    #[serde(skip)]
    tools: BTreeMap<String, Vec<LockfileTool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockfileTool {
    pub version: String,
    pub backend: Option<String>,
    // New consolidated assets section (replaces separate checksums and sizes)
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub assets: BTreeMap<String, AssetInfo>,
    // Legacy fields for backward compatibility during migration
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub checksums: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sizes: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Lockfile {
    fn read<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Lockfile::default());
        }
        trace!("reading lockfile {}", path.display_user());
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
        
        // Migrate legacy format to new format
        lockfile.migrate_legacy_format();
        
        Ok(lockfile)
    }

    fn migrate_legacy_format(&mut self) {
        // Move checksums and sizes from separate sections to consolidated assets section
        for (_tool_name, versions) in &mut self.tools {
            for version in versions {
                // Migrate checksums and sizes to assets section
                let checksums_to_migrate: Vec<(String, String)> = version.checksums.clone().into_iter().collect();
                let sizes_to_migrate: Vec<(String, u64)> = version.sizes.clone().into_iter().collect();
                
                version.checksums.clear();
                version.sizes.clear();
                
                // Combine checksums and sizes into assets
                for (filename, checksum) in checksums_to_migrate {
                    let asset = version.assets.entry(filename).or_insert_with(|| AssetInfo {
                        checksum: None,
                        size: None,
                        url: None,
                    });
                    asset.checksum = Some(checksum);
                }
                
                for (filename, size) in sizes_to_migrate {
                    let asset = version.assets.entry(filename).or_insert_with(|| AssetInfo {
                        checksum: None,
                        size: None,
                        url: None,
                    });
                    asset.size = Some(size);
                }
            }
        }
    }

    fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
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
            let content = format(content.parse()?);
            file::write(path, content)?;
        }
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
    
    // Helper method to get checksum for a filename from a specific tool
    pub fn get_checksum(&self, tool_name: &str, filename: &str) -> Option<&String> {
        self.tools.get(tool_name)?
            .first()?
            .assets.get(filename)?
            .checksum.as_ref()
    }
    
    // Helper method to get size for a filename from a specific tool
    pub fn get_size(&self, tool_name: &str, filename: &str) -> Option<u64> {
        self.tools.get(tool_name)?
            .first()?
            .assets.get(filename)?
            .size
    }
    
    // Helper method to get URL for a filename from a specific tool
    pub fn get_url(&self, tool_name: &str, filename: &str) -> Option<&String> {
        self.tools.get(tool_name)?
            .first()?
            .assets.get(filename)?
            .url.as_ref()
    }
}

pub fn update_lockfiles(config: &Config, ts: &Toolset, new_versions: &[ToolVersion]) -> Result<()> {
    if !Settings::get().lockfile || !Settings::get().experimental {
        return Ok(());
    }
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
        let mut tvl = ToolVersionList::new(Arc::new(backend.clone()), source.clone());
        tvl.versions.extend(tvs);
        tools_by_source
            .entry(source)
            .or_insert_with(HashMap::new)
            .insert(backend.short.to_string(), tvl);
    }

    let lockfiles = config
        .config_files
        .iter()
        .rev()
        .filter(|(_, cf)| cf.source().is_mise_toml())
        .map(|(p, _)| p)
        .collect_vec();
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
        existing_lockfile.tools.retain(|k, _| {
            all_tool_names.contains(k)
                || !tool_enabled(
                    &Settings::get().enable_tools(),
                    &Settings::get().disable_tools(),
                    k,
                )
                || REGISTRY
                    .get(&k.as_str())
                    .is_some_and(|rt| !rt.is_supported_os())
        });

        for (short, tvl) in tools {
            let lockfile_tools: Vec<LockfileTool> = tvl.clone().into();
            existing_lockfile.tools.insert(short.to_string(), lockfile_tools);
        }

        existing_lockfile.save(&lockfile_path)?;
    }

    Ok(())
}

fn read_all_lockfiles(config: &Config) -> Lockfile {
    config
        .config_files
        .iter()
        .rev()
        .filter(|(_, cf)| cf.source().is_mise_toml())
        .map(|(p, _)| read_lockfile_for(p))
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
    config: &Config,
    path: Option<&Path>,
    short: &str,
    prefix: &str,
) -> Result<Option<LockfileTool>> {
    if !Settings::get().lockfile || !Settings::get().experimental {
        return Ok(None);
    }

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
            read_all_lockfiles(config)
        }
    };

    if let Some(tool) = lockfile.tools.get(short) {
        Ok(tool
            .iter()
            // TODO: this likely won't work right when using `python@latest python@3.12`
            .find(|v| prefix == "latest" || v.version.starts_with(prefix))
            .inspect(|v| trace!("[{short}@{prefix}] found {} in lockfile", v.version))
            .cloned()
            .map(|mut tool| {
                // Populate checksums and sizes from the assets section for backward compatibility
                for (filename, asset_info) in &tool.assets {
                    if let Some(checksum) = &asset_info.checksum {
                        tool.checksums.insert(filename.clone(), checksum.clone());
                    }
                    if let Some(size) = asset_info.size {
                        tool.sizes.insert(filename.clone(), size);
                    }
                }
                tool
            }))
    } else {
        Ok(None)
    }
}

fn handle_missing_lockfile(err: Report, lockfile_path: &Path) -> Lockfile {
    warn!(
        "failed to read lockfile {}: {err:?}",
        display_path(lockfile_path)
    );
    Lockfile::default()
}

impl TryFrom<toml::Value> for LockfileTool {
    type Error = Report;
    fn try_from(value: toml::Value) -> Result<Self> {
        let tool = match value {
            toml::Value::String(v) => LockfileTool {
                version: v,
                backend: Default::default(),
                assets: Default::default(),
                checksums: Default::default(),
                sizes: Default::default(),
            },
            toml::Value::Table(mut t) => {
                let mut assets = BTreeMap::new();
                let mut checksums = BTreeMap::new();
                let mut sizes = BTreeMap::new();
                
                if let Some(assets_table) = t.remove("assets") {
                    let assets_table: toml::Table = assets_table.try_into()?;
                    for (filename, asset_info) in assets_table {
                        assets.insert(filename, asset_info.try_into()?);
                    }
                }
                if let Some(checksums_table) = t.remove("checksums") {
                    let checksums_table: toml::Table = checksums_table.try_into()?;
                    for (filename, checksum) in checksums_table {
                        checksums.insert(filename, checksum.try_into()?);
                    }
                }
                if let Some(sizes_table) = t.remove("sizes") {
                    let sizes_table: toml::Table = sizes_table.try_into()?;
                    for (filename, size) in sizes_table {
                        sizes.insert(filename, size.try_into()?);
                    }
                }
                LockfileTool {
                    version: t
                        .remove("version")
                        .map(|v| v.try_into())
                        .transpose()?
                        .unwrap_or_default(),
                    backend: t
                        .remove("backend")
                        .map(|v| v.try_into())
                        .transpose()?
                        .unwrap_or_default(),
                    assets,
                    checksums,
                    sizes,
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
        if let Some(backend) = self.backend {
            table.insert("backend".to_string(), backend.into());
        }
        // Serialize new consolidated assets section
        if !self.assets.is_empty() {
            table.insert("assets".to_string(), self.assets.clone().into());
        }
        // Don't serialize legacy checksums and sizes fields - they're now in the assets section
        table.into()
    }
}

impl From<ToolVersionList> for Vec<LockfileTool> {
    fn from(tvl: ToolVersionList) -> Self {
        tvl.versions
            .iter()
            .map(|tv| {
                let mut assets = BTreeMap::new();
                
                // Convert checksums and sizes to assets format
                for (filename, checksum) in &tv.checksums {
                    let asset = assets.entry(filename.clone()).or_insert_with(|| AssetInfo {
                        checksum: None,
                        size: None,
                        url: None,
                    });
                    asset.checksum = Some(checksum.clone());
                }
                
                for (filename, size) in &tv.sizes {
                    let asset = assets.entry(filename.clone()).or_insert_with(|| AssetInfo {
                        checksum: None,
                        size: None,
                        url: None,
                    });
                    asset.size = Some(*size);
                }
                
                LockfileTool {
                    version: tv.version.clone(),
                    backend: Some(tv.ba().full()),
                    assets,
                    // Keep empty legacy fields for compatibility
                    checksums: BTreeMap::new(),
                    sizes: BTreeMap::new(),
                }
            })
            .collect()
    }
}

fn format(mut doc: DocumentMut) -> String {
    if let Some(tools) = doc.get_mut("tools") {
        for (_k, v) in tools.as_table_mut().unwrap().iter_mut() {
            match v {
                toml_edit::Item::ArrayOfTables(art) => {
                    for t in art.iter_mut() {
                        t.sort_values_by(|a, _, b, _| {
                            if a == "version" {
                                return std::cmp::Ordering::Less;
                            }
                            a.to_string().cmp(&b.to_string())
                        });
                        // Sort assets section within each tool
                        if let Some(assets) = t.get_mut("assets") {
                            if let toml_edit::Item::Table(assets_table) = assets {
                                assets_table.sort_values();
                            }
                        }
                    }
                }
                toml_edit::Item::Table(t) => {
                    t.sort_values_by(|a, _, b, _| {
                        if a == "version" {
                            return std::cmp::Ordering::Less;
                        }
                        a.to_string().cmp(&b.to_string())
                    });
                    // Sort assets section within each tool
                    if let Some(assets) = t.get_mut("assets") {
                        if let toml_edit::Item::Table(assets_table) = assets {
                            assets_table.sort_values();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    
    doc.to_string()
}
