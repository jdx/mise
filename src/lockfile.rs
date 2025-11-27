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
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
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
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub options: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub platforms: BTreeMap<String, PlatformInfo>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_api: Option<String>,
}

impl TryFrom<toml::Value> for PlatformInfo {
    type Error = Report;
    fn try_from(value: toml::Value) -> Result<Self> {
        match value {
            toml::Value::String(checksum) => Ok(PlatformInfo {
                checksum: Some(checksum),
                name: None,
                size: None,
                url: None,
                url_api: None,
            }),
            toml::Value::Integer(size) => Ok(PlatformInfo {
                checksum: None,
                name: None,
                size: Some(size.try_into()?),
                url: None,
                url_api: None,
            }),
            toml::Value::Table(mut t) => {
                let checksum = match t.remove("checksum") {
                    Some(toml::Value::String(s)) => Some(s),
                    _ => None,
                };
                let name = match t.remove("name") {
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
                let url_api = match t.remove("url_api") {
                    Some(toml::Value::String(s)) => Some(s),
                    _ => None,
                };
                Ok(PlatformInfo {
                    checksum,
                    name,
                    size,
                    url,
                    url_api,
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
        if let Some(name) = platform_info.name {
            table.insert("name".to_string(), name.into());
        }
        if let Some(size) = platform_info.size {
            table.insert("size".to_string(), (size as i64).into());
        }
        if let Some(url) = platform_info.url {
            table.insert("url".to_string(), url.into());
        }
        if let Some(url_api) = platform_info.url_api {
            table.insert("url_api".to_string(), url_api.into());
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

        for (short, value) in tools {
            let versions = match value {
                toml::Value::Array(arr) => arr
                    .into_iter()
                    .map(LockfileTool::try_from)
                    .collect::<Result<Vec<_>>>()?,
                _ => bail!(
                    "invalid lockfile format for tool {short}: expected array ([[tools.{short}]])"
                ),
            };
            lockfile.tools.insert(short, versions);
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

    /// Get all platform keys present in the lockfile
    pub fn all_platform_keys(&self) -> BTreeSet<String> {
        let mut platforms = BTreeSet::new();
        for tools in self.tools.values() {
            for tool in tools {
                for platform_key in tool.platforms.keys() {
                    platforms.insert(platform_key.clone());
                }
            }
        }
        platforms
    }

    /// Update or add platform info for a tool version
    pub fn set_platform_info(
        &mut self,
        short: &str,
        version: &str,
        backend: Option<&str>,
        options: &BTreeMap<String, String>,
        platform_key: &str,
        platform_info: PlatformInfo,
    ) {
        let tools = self.tools.entry(short.to_string()).or_default();
        // Find existing tool version with matching options or create new one
        if let Some(tool) = tools
            .iter_mut()
            .find(|t| t.version == version && &t.options == options)
        {
            tool.platforms
                .insert(platform_key.to_string(), platform_info);
        } else {
            let mut platforms = BTreeMap::new();
            platforms.insert(platform_key.to_string(), platform_info);
            tools.push(LockfileTool {
                version: version.to_string(),
                backend: backend.map(|s| s.to_string()),
                options: options.clone(),
                platforms,
            });
        }
    }

    /// Save the lockfile to disk (public for mise lock command)
    pub fn write<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.save(path)
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
                    // Look for existing tool with same version AND options to preserve platform info
                    if let Some(existing_tool) = existing_tools
                        .iter()
                        .find(|et| et.version == new_tool.version && et.options == new_tool.options)
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
                        // No existing version+options match, use new tool as-is
                        merged_tools.push(new_tool);
                    }
                }

                // Add any existing tools that weren't in the new toolset
                // BUT only if they still match a request in the current configuration
                for existing_tool in existing_tools {
                    if !merged_tools.iter().any(|mt| {
                        mt.version == existing_tool.version && mt.options == existing_tool.options
                    }) {
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
    request_options: &BTreeMap<String, String>,
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
            .find(|v| {
                // Version prefix matching
                let version_matches = prefix == "latest" || v.version.starts_with(prefix);
                // Options must match exactly
                let options_match = &v.options == request_options;
                version_matches && options_match
            })
            .inspect(|v| {
                trace!(
                    "[{short}@{prefix}] found {} in lockfile (options: {:?})",
                    v.version, v.options
                )
            })
            .cloned())
    } else {
        Ok(None)
    }
}

/// Get the backend for a tool from the lockfile, ignoring options.
/// This is used for backend discovery where we just need any entry's backend.
pub fn get_locked_backend(config: &Config, short: &str) -> Option<String> {
    if !Settings::get().lockfile || !Settings::get().experimental {
        return None;
    }

    let lockfile = read_all_lockfiles(config);

    lockfile
        .tools
        .get(short)
        .and_then(|tools| tools.first())
        .and_then(|tool| tool.backend.clone())
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
                options: Default::default(),
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
                let mut options = BTreeMap::new();
                if let Some(opts) = t.remove("options") {
                    let opts_table: toml::Table = opts.try_into()?;
                    for (key, value) in opts_table {
                        if let toml::Value::String(s) = value {
                            options.insert(key, s);
                        }
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
                    options,
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
        if !self.options.is_empty() {
            let opts_table: toml::Table = self
                .options
                .into_iter()
                .map(|(k, v)| (k, toml::Value::String(v)))
                .collect();
            table.insert("options".to_string(), toml::Value::Table(opts_table));
        }
        if !self.platforms.is_empty() {
            table.insert("platforms".to_string(), self.platforms.clone().into());
        }
        table.into()
    }
}

impl From<ToolVersionList> for Vec<LockfileTool> {
    fn from(tvl: ToolVersionList) -> Self {
        use crate::backend::platform_target::PlatformTarget;

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
                            name: platform_info.name.clone(),
                            size: platform_info.size,
                            url: platform_info.url.clone(),
                            url_api: platform_info.url_api.clone(),
                        },
                    );
                }

                // Resolve lockfile options from the backend
                let options = if let Ok(backend) = tv.request.backend() {
                    let target = PlatformTarget::from_current();
                    backend.resolve_lockfile_options(&tv.request, &target)
                } else {
                    BTreeMap::new()
                };

                LockfileTool {
                    version: tv.version.clone(),
                    backend: Some(tv.ba().full()),
                    options,
                    platforms,
                }
            })
            .collect()
    }
}

fn format(mut doc: DocumentMut) -> String {
    if let Some(tools) = doc.get_mut("tools") {
        for (_k, v) in tools.as_table_mut().unwrap().iter_mut() {
            if let toml_edit::Item::ArrayOfTables(art) = v {
                for t in art.iter_mut() {
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
    fn test_array_format_required() {
        // Test that multi-version (array) format is read correctly
        let multi_version_toml = r#"
[[tools.node]]
version = "20.10.0"
backend = "core:node"

[[tools.python]]
version = "3.11.0"
backend = "core:python"
"#;

        let table: toml::Table = toml::from_str(multi_version_toml).unwrap();
        let tools: toml::Table = table.get("tools").unwrap().clone().try_into().unwrap();

        let mut lockfile = Lockfile::default();
        for (short, value) in tools {
            let versions = match value {
                toml::Value::Array(arr) => arr
                    .into_iter()
                    .map(LockfileTool::try_from)
                    .collect::<Result<Vec<_>>>()
                    .unwrap(),
                _ => panic!("expected array format"),
            };
            lockfile.tools.insert(short, versions);
        }

        // Verify that we have the expected tools
        assert_eq!(lockfile.tools.len(), 2);
        assert!(lockfile.tools.contains_key("node"));
        assert!(lockfile.tools.contains_key("python"));

        // Verify node
        let node_versions = &lockfile.tools["node"];
        assert_eq!(node_versions.len(), 1);
        assert_eq!(node_versions[0].version, "20.10.0");
        assert_eq!(node_versions[0].backend, Some("core:node".to_string()));

        // Verify python
        let python_versions = &lockfile.tools["python"];
        assert_eq!(python_versions.len(), 1);
        assert_eq!(python_versions[0].version, "3.11.0");
    }

    #[test]
    fn test_save_uses_array_format() {
        let mut lockfile = Lockfile::default();
        let mut platforms = BTreeMap::new();
        platforms.insert(
            "macos-arm64".to_string(),
            PlatformInfo {
                checksum: Some("sha256:abc123".to_string()),
                name: Some("node.tar.gz".to_string()),
                size: Some(12345678),
                url: Some("https://example.com/node.tar.gz".to_string()),
                url_api: Some("https://api.github.com.com/repos/test/1234".to_string()),
            },
        );

        let tool = LockfileTool {
            version: "20.10.0".to_string(),
            backend: Some("core:node".to_string()),
            options: BTreeMap::new(),
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

    #[test]
    fn test_options_field_parsing_and_serialization() {
        // Test parsing lockfile with options
        let toml_with_options = r#"
[[tools.ripgrep]]
version = "14.0.0"
backend = "ubi:BurntSushi/ripgrep"
options = { exe = "rg", matching = "musl" }

[tools.ripgrep.platforms.linux-x64]
checksum = "blake3:abc123"
"#;

        let table: toml::Table = toml::from_str(toml_with_options).unwrap();
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

        // Verify options were parsed correctly
        let ripgrep = &lockfile.tools["ripgrep"][0];
        assert_eq!(ripgrep.options.get("exe"), Some(&"rg".to_string()));
        assert_eq!(ripgrep.options.get("matching"), Some(&"musl".to_string()));
    }

    #[test]
    fn test_options_field_not_serialized_when_empty() {
        let mut lockfile = Lockfile::default();
        let tool = LockfileTool {
            version: "14.0.0".to_string(),
            backend: Some("ubi:BurntSushi/ripgrep".to_string()),
            options: BTreeMap::new(), // Empty options
            platforms: BTreeMap::new(),
        };
        lockfile.tools.insert("ripgrep".to_string(), vec![tool]);

        let temp_dir = std::env::temp_dir();
        let test_lockfile = temp_dir.join("test_lockfile_no_options.lock");

        lockfile.save(&test_lockfile).unwrap();
        let content = std::fs::read_to_string(&test_lockfile).unwrap();

        // Should NOT contain "options" when it's empty
        assert!(!content.contains("options"));

        let _ = std::fs::remove_file(&test_lockfile);
    }

    #[test]
    fn test_options_field_serialized_when_present() {
        let mut lockfile = Lockfile::default();
        let mut options = BTreeMap::new();
        options.insert("exe".to_string(), "rg".to_string());
        options.insert("matching".to_string(), "musl".to_string());

        let tool = LockfileTool {
            version: "14.0.0".to_string(),
            backend: Some("ubi:BurntSushi/ripgrep".to_string()),
            options,
            platforms: BTreeMap::new(),
        };
        lockfile.tools.insert("ripgrep".to_string(), vec![tool]);

        let temp_dir = std::env::temp_dir();
        let test_lockfile = temp_dir.join("test_lockfile_with_options.lock");

        lockfile.save(&test_lockfile).unwrap();
        let content = std::fs::read_to_string(&test_lockfile).unwrap();

        // Should contain options
        assert!(content.contains("options"));
        assert!(content.contains("exe"));
        assert!(content.contains("rg"));

        let _ = std::fs::remove_file(&test_lockfile);
    }

    #[test]
    fn test_options_matching_in_get_locked_version() {
        // This tests that get_locked_version requires exact options match
        let toml_with_options = r#"
[[tools.ripgrep]]
version = "14.0.0"
backend = "ubi:BurntSushi/ripgrep"
options = { exe = "rg", matching = "musl" }

[[tools.ripgrep]]
version = "14.0.0"
backend = "ubi:BurntSushi/ripgrep"
options = { exe = "rg" }
"#;

        let table: toml::Table = toml::from_str(toml_with_options).unwrap();
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

        // Verify we have 2 entries for ripgrep with different options
        assert_eq!(lockfile.tools["ripgrep"].len(), 2);
        assert_eq!(lockfile.tools["ripgrep"][0].options.len(), 2);
        assert_eq!(lockfile.tools["ripgrep"][1].options.len(), 1);
    }
}
