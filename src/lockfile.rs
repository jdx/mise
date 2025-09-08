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
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub platforms: BTreeMap<String, PlatformInfo>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl TryFrom<toml::Value> for PlatformInfo {
    type Error = Report;
    fn try_from(value: toml::Value) -> Result<Self> {
        match value {
            toml::Value::String(checksum) => Ok(PlatformInfo {
                checksum: Some(checksum),
                size: None,
                url: None,
            }),
            toml::Value::Integer(size) => Ok(PlatformInfo {
                checksum: None,
                size: Some(size.try_into()?),
                url: None,
            }),
            toml::Value::Table(mut t) => {
                let checksum = match t.remove("checksum") {
                    Some(toml::Value::String(s)) => Some(s),
                    _ => None,
                };
                let size = t
                    .remove("size")
                    .and_then(|v| v.as_integer())
                    .map(|i| i.try_into())
                    .transpose()?;
                let url = match t.remove("url") {
                    Some(toml::Value::String(s)) => Some(s),
                    _ => None,
                };
                Ok(PlatformInfo {
                    checksum,
                    size,
                    url,
                })
            }
            _ => bail!("unsupported asset info format"),
        }
    }
}

impl From<PlatformInfo> for toml::Value {
    fn from(platform_info: PlatformInfo) -> Self {
        let mut table = toml::Table::new();
        if let Some(checksum) = platform_info.checksum {
            table.insert("checksum".to_string(), checksum.into());
        }
        if let Some(size) = platform_info.size {
            table.insert("size".to_string(), (size as i64).into());
        }
        if let Some(url) = platform_info.url {
            table.insert("url".to_string(), url.into());
        }
        toml::Value::Table(table)
    }
}

impl Lockfile {
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self> {
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
        let mut has_single_version_format = false;

        for (short, value) in tools {
            let versions = match value {
                toml::Value::Array(arr) => arr
                    .into_iter()
                    .map(LockfileTool::try_from)
                    .collect::<Result<Vec<_>>>()?,
                _ => {
                    // Single-Version format detected - will be auto-migrated
                    has_single_version_format = true;
                    trace!("Auto-migrating single-version format for tool: {}", short);
                    vec![LockfileTool::try_from(value)?]
                }
            };
            lockfile.tools.insert(short, versions);
        }

        if has_single_version_format {
            debug!(
                "Auto-migrated lockfile from single-version to multi-version format: {}",
                path.display()
            );
        }

        Ok(lockfile)
    }

    fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        if self.is_empty() {
            let _ = file::remove_file(path);
        } else {
            let mut tools = toml::Table::new();
            for (short, versions) in &self.tools {
                // Always write Multi-Version format (array format) for consistency
                let value: toml::Value = versions
                    .iter()
                    .cloned()
                    .map(|version| version.into_toml_value())
                    .collect::<Vec<toml::Value>>()
                    .into();
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

    pub fn tools(&self) -> &BTreeMap<String, Vec<LockfileTool>> {
        &self.tools
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
    // When `mise up` runs, new_versions contains the upgraded version
    // We need to replace the old version, not add to it
    for (backend, group) in &new_versions.iter().chunk_by(|tv| tv.ba()) {
        let tvs = group.cloned().collect_vec();
        let source = tvs[0].request.source().clone();

        // Get or create the entry for this source and backend
        let source_tools = tools_by_source
            .entry(source.clone())
            .or_insert_with(HashMap::new);

        if let Some(existing_tvl) = source_tools.get_mut(&backend.short) {
            // Check if new versions are upgrades (same request, different version)
            // If so, replace the old versions with matching requests
            for new_tv in tvs {
                // Remove any existing versions with the same request
                existing_tvl.versions.retain(|tv| {
                    // Keep versions that have different requests
                    tv.request.version() != new_tv.request.version()
                });

                // Add the new version
                existing_tvl.versions.push(new_tv);
            }
        } else {
            // Create new entry if it doesn't exist
            let mut tvl = ToolVersionList::new(Arc::new(backend.clone()), source.clone());
            tvl.versions.extend(tvs);
            source_tools.insert(backend.short.to_string(), tvl);
        }
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
        // Only update existing lockfiles - creation is done elsewhere (e.g., by `mise lock`)
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
            let new_lockfile_tools: Vec<LockfileTool> = tvl.clone().into();

            // Merge with existing lockfile tools to preserve platform information
            if let Some(existing_tools) = existing_lockfile.tools.get(short) {
                let mut merged_tools = Vec::new();

                // For each new tool, check if we have an existing entry with platform info
                for new_tool in new_lockfile_tools {
                    // Look for existing tool with same version to preserve platform info
                    if let Some(existing_tool) = existing_tools
                        .iter()
                        .find(|et| et.version == new_tool.version)
                    {
                        // Start with the new tool as base (it may have fresh platform info)
                        let mut merged_tool = new_tool;

                        // Merge in any existing platform info that's not in the new tool
                        for (platform, platform_info) in &existing_tool.platforms {
                            if !merged_tool.platforms.contains_key(platform) {
                                merged_tool
                                    .platforms
                                    .insert(platform.clone(), platform_info.clone());
                            }
                        }
                        merged_tools.push(merged_tool);
                    } else {
                        // No existing version match, use new tool as-is
                        merged_tools.push(new_tool);
                    }
                }

                // Add any existing tools that weren't in the new toolset
                // BUT only if they still match a request in the current configuration
                for existing_tool in existing_tools {
                    if !merged_tools
                        .iter()
                        .any(|mt| mt.version == existing_tool.version)
                    {
                        // Check if this version still matches any request in the current toolset
                        // This prevents stale versions from persisting after upgrades
                        if let Some(tvl) = tools.get(short) {
                            let version_matches_request = tvl
                                .versions
                                .iter()
                                .any(|tv| tv.version == existing_tool.version);
                            if version_matches_request {
                                merged_tools.push(existing_tool.clone());
                            }
                        }
                    }
                }

                existing_lockfile
                    .tools
                    .insert(short.to_string(), merged_tools);
            } else {
                // No existing tools, just use the new ones
                existing_lockfile
                    .tools
                    .insert(short.to_string(), new_lockfile_tools);
            }
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
            .cloned())
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
                platforms: Default::default(),
            },
            toml::Value::Table(mut t) => {
                let mut platforms = BTreeMap::new();
                if let Some(platforms_table) = t.remove("platforms") {
                    let platforms_table: toml::Table = platforms_table.try_into()?;
                    for (platform, platform_info) in platforms_table {
                        platforms.insert(platform, platform_info.try_into()?);
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
                    platforms,
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
        if !self.platforms.is_empty() {
            table.insert("platforms".to_string(), self.platforms.clone().into());
        }
        table.into()
    }
}

impl From<ToolVersionList> for Vec<LockfileTool> {
    fn from(tvl: ToolVersionList) -> Self {
        tvl.versions
            .iter()
            .map(|tv| {
                let mut platforms = BTreeMap::new();

                // Convert tool version lock_platforms to lockfile platforms
                for (platform, platform_info) in &tv.lock_platforms {
                    platforms.insert(
                        platform.clone(),
                        PlatformInfo {
                            checksum: platform_info.checksum.clone(),
                            size: platform_info.size,
                            url: platform_info.url.clone(),
                        },
                    );
                }

                LockfileTool {
                    version: tv.version.clone(),
                    backend: Some(tv.ba().full()),
                    platforms,
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
                        // Sort platforms section within each tool
                        if let Some(toml_edit::Item::Table(platforms_table)) =
                            t.get_mut("platforms")
                        {
                            platforms_table.sort_values();
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
                    // Sort platforms section within each tool
                    if let Some(toml_edit::Item::Table(platforms_table)) = t.get_mut("platforms") {
                        platforms_table.sort_values();
                    }
                }
                _ => {}
            }
        }
    }

    doc.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_multi_version_format_migration() {
        // Test that single-version format is read correctly and writes as multi-version
        let single_version_toml = r#"
[tools.node]
version = "20.10.0"
backend = "core:node"

[[tools.python]]
version = "3.11.0"
backend = "core:python"
"#;

        let table: toml::Table = toml::from_str(single_version_toml).unwrap();
        let tools: toml::Table = table.get("tools").unwrap().clone().try_into().unwrap();

        let mut lockfile = Lockfile::default();
        for (short, value) in tools {
            let versions = match value {
                toml::Value::Array(arr) => arr
                    .into_iter()
                    .map(LockfileTool::try_from)
                    .collect::<Result<Vec<_>>>()
                    .unwrap(),
                _ => vec![LockfileTool::try_from(value).unwrap()],
            };
            lockfile.tools.insert(short, versions);
        }

        // Verify that we have the expected tools
        assert_eq!(lockfile.tools.len(), 2);
        assert!(lockfile.tools.contains_key("node"));
        assert!(lockfile.tools.contains_key("python"));

        // Verify node was migrated from single-version
        let node_versions = &lockfile.tools["node"];
        assert_eq!(node_versions.len(), 1);
        assert_eq!(node_versions[0].version, "20.10.0");
        assert_eq!(node_versions[0].backend, Some("core:node".to_string()));

        // Verify python was already multi-version
        let python_versions = &lockfile.tools["python"];
        assert_eq!(python_versions.len(), 1);
        assert_eq!(python_versions[0].version, "3.11.0");
    }

    #[test]
    fn test_save_always_uses_multi_version_format() {
        let mut lockfile = Lockfile::default();
        let mut platforms = BTreeMap::new();
        platforms.insert(
            "macos-arm64".to_string(),
            PlatformInfo {
                checksum: Some("sha256:abc123".to_string()),
                url: Some("https://example.com/node.tar.gz".to_string()),
                size: Some(12345678),
            },
        );

        let tool = LockfileTool {
            version: "20.10.0".to_string(),
            backend: Some("core:node".to_string()),
            platforms,
        };

        lockfile.tools.insert("node".to_string(), vec![tool]);

        // Create a temporary file to test saving
        let temp_dir = std::env::temp_dir();
        let test_lockfile = temp_dir.join("test_lockfile.lock");

        // Save and verify it uses multi-version format
        lockfile.save(&test_lockfile).unwrap();

        let content = std::fs::read_to_string(&test_lockfile).unwrap();

        // Should use [[tools.node]] array syntax, not [tools.node] single version
        assert!(content.contains("[[tools.node]]"));
        // Verify it doesn't use single-version format (but allow platforms sections)
        assert!(!content.lines().any(|line| line.trim() == "[tools.node]"));

        // Clean up
        let _ = std::fs::remove_file(&test_lockfile);
    }
}
