use crate::config::{Config, Settings};
use crate::env;
use crate::file;
use crate::file::display_path;
use crate::path::PathExt;
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
use xx::regex;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    #[serde(skip)]
    tools: BTreeMap<String, Vec<LockfileTool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockfileTool {
    pub version: String,
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub options: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub platforms: BTreeMap<String, PlatformInfo>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    // TODO: Add size back if we find a good way to generate it with `mise lock`
    #[serde(skip_serializing, default)]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_api: Option<String>,
}

impl PlatformInfo {
    /// Returns true if this PlatformInfo has no meaningful data (for serde skip)
    pub fn is_empty(&self) -> bool {
        self.checksum.is_none() && self.url.is_none() && self.url_api.is_none()
    }
}

impl TryFrom<toml::Value> for PlatformInfo {
    type Error = Report;
    fn try_from(value: toml::Value) -> Result<Self> {
        match value {
            toml::Value::String(checksum) => Ok(PlatformInfo {
                checksum: Some(checksum),
                size: None,
                url: None,
                url_api: None,
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
                let url_api = match t.remove("url_api") {
                    Some(toml::Value::String(s)) => Some(s),
                    _ => None,
                };
                Ok(PlatformInfo {
                    checksum,
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
    /// Merges with existing info, preserving fields we don't have new values for
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
            // Merge with existing platform info, preferring new values when present
            let merged = if let Some(existing) = tool.platforms.get(platform_key) {
                PlatformInfo {
                    checksum: platform_info.checksum.or_else(|| existing.checksum.clone()),
                    size: platform_info.size.or(existing.size),
                    url: platform_info.url.or_else(|| existing.url.clone()),
                    url_api: platform_info.url_api.or_else(|| existing.url_api.clone()),
                }
            } else {
                platform_info
            };
            // Only insert non-empty platform info to avoid `"platforms.linux-x64" = {}`
            if !merged.is_empty() {
                tool.platforms.insert(platform_key.to_string(), merged);
            }
        } else {
            let mut platforms = BTreeMap::new();
            // Only insert non-empty platform info
            if !platform_info.is_empty() {
                platforms.insert(platform_key.to_string(), platform_info);
            }
            tools.push(LockfileTool {
                version: version.to_string(),
                backend: backend.map(|s| s.to_string()),
                options: options.clone(),
                env: None,
                platforms,
            });
        }
    }

    /// Save the lockfile to disk (public for mise lock command)
    pub fn write<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.save(path)
    }
}

/// Determines the lockfile path for a given config file path
/// Returns (lockfile_path, is_local)
///
/// Lockfiles are placed alongside their config files:
/// - `mise.toml` -> `mise.lock`
/// - `.config/mise.toml` -> `.config/mise.lock`
/// - `.mise/config.toml` -> `.mise/mise.lock`
/// - `.mise/conf.d/foo.toml` -> `.mise/mise.lock` (conf.d files share parent's lockfile)
pub fn lockfile_path_for_config(config_path: &Path) -> (PathBuf, bool) {
    let is_local = is_local_config(config_path);
    let lockfile_name = if is_local {
        "mise.local.lock"
    } else {
        "mise.lock"
    };

    let parent = config_path.parent().unwrap_or(Path::new("."));
    let parent_name = parent
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    // For conf.d files, place lockfile at parent of conf.d so all conf.d files share one lockfile
    let lockfile_dir = if parent_name == "conf.d" {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };

    (lockfile_dir.join(lockfile_name), is_local)
}

/// Checks if a config path is a "local" config (should go to mise.local.lock)
fn is_local_config(path: &Path) -> bool {
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    filename.contains(".local.")
}

/// Extracts environment name from config filename
/// e.g., "mise.test.toml" -> Some("test"), "mise.test.local.toml" -> Some("test"), "mise.toml" -> None
fn extract_env_from_config_path(path: &Path) -> Option<String> {
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    // Pattern matches:
    // - mise.{env}.toml -> captures env
    // - mise.{env}.local.toml -> captures env (env-specific local config)
    // - .mise.{env}.toml -> captures env
    // - config.{env}.toml -> captures env
    // Does NOT match (returns None):
    // - mise.toml, .mise.toml, config.toml (base configs)
    // - mise.local.toml (local without env - filtered by "local" check)
    let re = regex!(r"^(?:\.?mise|config)\.([^.]+)(?:\.local)?\.toml$");
    re.captures(filename)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .filter(|s| s != "local")
}

pub fn update_lockfiles(config: &Config, ts: &Toolset, new_versions: &[ToolVersion]) -> Result<()> {
    if !Settings::get().lockfile || !Settings::get().experimental {
        return Ok(());
    }

    // Collect tools by source (config file)
    let mut tools_by_source: HashMap<ToolSource, HashMap<String, ToolVersionList>> = HashMap::new();
    for (source, group) in &ts.versions.iter().chunk_by(|(_, tvl)| &tvl.source) {
        for (ba, tvl) in group {
            tools_by_source
                .entry(source.clone())
                .or_default()
                .insert(ba.short.to_string(), tvl.clone());
        }
    }

    // Add versions added within this session (from `mise use` or `mise up`)
    for (backend, group) in &new_versions.iter().chunk_by(|tv| tv.ba()) {
        let tvs = group.cloned().collect_vec();
        let source = tvs[0].request.source().clone();
        let source_tools = tools_by_source.entry(source.clone()).or_default();

        if let Some(existing_tvl) = source_tools.get_mut(&backend.short) {
            for new_tv in tvs {
                existing_tvl
                    .versions
                    .retain(|tv| tv.request.version() != new_tv.request.version());
                existing_tvl.versions.push(new_tv);
            }
        } else {
            let mut tvl = ToolVersionList::new(Arc::new(backend.clone()), source.clone());
            tvl.versions.extend(tvs);
            source_tools.insert(backend.short.to_string(), tvl);
        }
    }

    // Group config files by target lockfile path
    // Key: lockfile path, Value: list of (config_path, env) tuples
    let mut lockfile_configs: HashMap<PathBuf, Vec<(PathBuf, Option<String>)>> = HashMap::new();
    for (config_path, cf) in config.config_files.iter().rev() {
        if !cf.source().is_mise_toml() {
            continue;
        }
        let (lockfile_path, _is_local) = lockfile_path_for_config(config_path);
        let env = extract_env_from_config_path(config_path);
        lockfile_configs
            .entry(lockfile_path)
            .or_default()
            .push((config_path.clone(), env));
    }

    debug!("updating {} lockfiles", lockfile_configs.len());

    // Process each lockfile
    for (lockfile_path, configs) in lockfile_configs {
        // Only update existing lockfiles - creation is done elsewhere (e.g., by `mise lock`)
        if !lockfile_path.exists() {
            continue;
        }

        trace!(
            "updating lockfile {} from {} config files",
            display_path(&lockfile_path),
            configs.len()
        );

        let mut existing_lockfile = Lockfile::read(&lockfile_path)
            .unwrap_or_else(|err| handle_missing_lockfile(err, &lockfile_path));

        // Collect all tools from all contributing configs with their env context
        // Key: tool short name, Value: list of (LockfileTool, env)
        let mut tools_with_env: HashMap<String, Vec<(LockfileTool, Option<String>)>> =
            HashMap::new();

        for (config_path, env) in &configs {
            let tool_source = ToolSource::MiseToml(config_path.clone());
            if let Some(tools) = tools_by_source.get(&tool_source) {
                for (short, tvl) in tools {
                    let lockfile_tools: Vec<LockfileTool> = tvl.clone().into();
                    for tool in lockfile_tools {
                        tools_with_env
                            .entry(short.clone())
                            .or_default()
                            .push((tool, env.clone()));
                    }
                }
            }
        }

        // Preserve base entries from existing lockfile that were overridden by env configs
        // Without this, base entries (env=None) get dropped when env configs override them
        // Only preserve if ALL new entries are env-specific - if any new entry has env=None,
        // it means the base config was updated and old entries should be replaced, not preserved
        for (short, existing_entries) in &existing_lockfile.tools {
            if let Some(new_entries) = tools_with_env.get_mut(short) {
                // Only preserve if all new entries are env-specific (no base config update)
                let all_env_specific = new_entries.iter().all(|(_, env)| env.is_some());
                if all_env_specific {
                    for existing in existing_entries {
                        // If existing entry has no env (base) and isn't already in new_entries, preserve it
                        if existing.env.is_none()
                            && !new_entries.iter().any(|(t, _)| {
                                t.version == existing.version && t.options == existing.options
                            })
                        {
                            new_entries.push((existing.clone(), None));
                        }
                    }
                }
            }
        }

        // Process each tool with deduplication and env merging
        for (short, entries) in tools_with_env {
            let merged_tools =
                merge_tool_entries_with_env(entries, existing_lockfile.tools.get(&short));
            existing_lockfile.tools.insert(short, merged_tools);
        }

        existing_lockfile.save(&lockfile_path)?;
    }

    Ok(())
}

/// Merge tool entries with environment tracking and deduplication
/// Rules:
/// - Same version+options: if any has no env (base), keep only base entry; otherwise merge env arrays
/// - Different version/options: separate entries
/// - Preserve existing env-specific entries that aren't in new entries (env configs may not be loaded)
#[allow(clippy::type_complexity)]
fn merge_tool_entries_with_env(
    entries: Vec<(LockfileTool, Option<String>)>,
    existing_tools: Option<&Vec<LockfileTool>>,
) -> Vec<LockfileTool> {
    // Group by (version, options) - the key for deduplication
    let mut by_key: HashMap<
        (String, BTreeMap<String, String>),
        (LockfileTool, BTreeSet<String>, bool),
    > = HashMap::new();

    for (tool, env) in entries {
        let key = (tool.version.clone(), tool.options.clone());
        let entry = by_key
            .entry(key)
            .or_insert_with(|| (tool.clone(), BTreeSet::new(), false));

        // Merge platforms
        for (platform, info) in tool.platforms {
            entry.0.platforms.entry(platform).or_insert(info);
        }

        // Track env - if any entry has no env, mark as base
        if let Some(e) = env {
            entry.1.insert(e);
        } else {
            entry.2 = true; // has_base
        }
    }

    // Merge with existing tools to preserve platform info AND env-specific entries
    if let Some(existing) = existing_tools {
        for existing_tool in existing {
            let key = (existing_tool.version.clone(), existing_tool.options.clone());
            if let Some(entry) = by_key.get_mut(&key) {
                // Merge platform info from existing
                for (platform, info) in &existing_tool.platforms {
                    entry
                        .0
                        .platforms
                        .entry(platform.clone())
                        .or_insert(info.clone());
                }
                // Preserve existing env if we have no new env info
                if entry.1.is_empty()
                    && !entry.2
                    && let Some(ref existing_env) = existing_tool.env
                {
                    for e in existing_env {
                        entry.1.insert(e.clone());
                    }
                }
            } else if existing_tool.env.is_some() {
                // Check if this env is already covered by a new entry
                // If so, the existing entry is stale and should not be preserved
                let existing_envs = existing_tool.env.as_ref().unwrap();
                let env_already_covered = by_key
                    .values()
                    .any(|(_, new_envs, _)| existing_envs.iter().any(|e| new_envs.contains(e)));

                if !env_already_covered {
                    // Preserve env-specific entries that have no match in new entries
                    // and whose env is not covered by any new entry
                    // This handles the case where env configs (e.g., mise.test.toml) aren't loaded
                    // but we don't want to lose their lockfile entries
                    by_key.insert(
                        key,
                        (
                            existing_tool.clone(),
                            existing_tool
                                .env
                                .clone()
                                .unwrap_or_default()
                                .into_iter()
                                .collect(),
                            false,
                        ),
                    );
                }
            }
        }
    }

    // Convert to final list
    by_key
        .into_values()
        .map(|(mut tool, envs, has_base)| {
            // If has_base (any entry had no env), don't set env field
            // Otherwise, set env field with merged envs
            tool.env = if has_base || envs.is_empty() {
                None
            } else {
                Some(envs.into_iter().sorted().collect())
            };
            tool
        })
        .sorted_by(|a, b| a.version.cmp(&b.version))
        .collect()
}

fn read_all_lockfiles(config: &Config) -> Arc<Lockfile> {
    // Cache by sorted config paths to avoid recomputing on every call
    static CACHE: Lazy<Mutex<HashMap<Vec<PathBuf>, Arc<Lockfile>>>> = Lazy::new(Default::default);

    // Create a cache key from the config file paths
    let cache_key: Vec<PathBuf> = config.config_files.keys().cloned().collect();

    let mut cache = CACHE.lock().unwrap();
    if let Some(cached) = cache.get(&cache_key) {
        return Arc::clone(cached);
    }

    let mut seen_roots: HashSet<PathBuf> = HashSet::new();
    let mut all: Vec<Lockfile> = Vec::new();

    for (path, cf) in config.config_files.iter().rev() {
        if !cf.source().is_mise_toml() {
            continue;
        }

        let (lockfile_path, _) = lockfile_path_for_config(path);
        let root = lockfile_path.parent().unwrap_or(path).to_path_buf();
        if seen_roots.contains(&root) {
            continue;
        }
        seen_roots.insert(root.clone());

        // Read both lockfiles (local takes precedence)
        let local_path = root.join("mise.local.lock");
        if let Ok(local) = Lockfile::read(&local_path) {
            all.push(local);
        }
        let main_path = root.join("mise.lock");
        if let Ok(main) = Lockfile::read(&main_path) {
            all.push(main);
        }
    }

    let result = all.into_iter().fold(Lockfile::default(), |mut acc, l| {
        for (short, tools) in l.tools {
            let existing = acc.tools.entry(short).or_default();
            for tool in tools {
                // Avoid duplicates (same version+options+env)
                if !existing.iter().any(|t| {
                    t.version == tool.version && t.options == tool.options && t.env == tool.env
                }) {
                    existing.push(tool);
                }
            }
        }
        acc
    });

    let result = Arc::new(result);
    cache.insert(cache_key, Arc::clone(&result));
    result
}

fn read_lockfile_for(path: &Path) -> Arc<Lockfile> {
    // Cache by config path to avoid recomputing lockfile_path_for_config on every call
    static CACHE: Lazy<Mutex<HashMap<PathBuf, Arc<Lockfile>>>> = Lazy::new(Default::default);

    let mut cache = CACHE.lock().unwrap();
    if let Some(cached) = cache.get(path) {
        return Arc::clone(cached);
    }

    // Only compute lockfile path when not cached
    let (lockfile_path, _is_local) = lockfile_path_for_config(path);
    let lockfile = Lockfile::read(&lockfile_path)
        .unwrap_or_else(|err| handle_missing_lockfile(err, &lockfile_path));

    let lockfile = Arc::new(lockfile);
    cache.insert(path.to_path_buf(), Arc::clone(&lockfile));
    lockfile
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

    let current_envs: HashSet<&str> = env::MISE_ENV.iter().map(|s| s.as_str()).collect();

    let lockfile = match path {
        Some(path) => {
            trace!(
                "[{short}@{prefix}] reading lockfile for {}",
                display_path(path)
            );
            read_lockfile_for(path)
        }
        None => {
            trace!("[{short}@{prefix}] reading all lockfiles");
            read_all_lockfiles(config)
        }
    };

    if let Some(tools) = lockfile.tools.get(short) {
        // Filter by version prefix and options
        let mut matching: Vec<_> = tools
            .iter()
            .filter(|v| {
                let version_matches = prefix == "latest" || v.version.starts_with(prefix);
                let options_match = &v.options == request_options;
                version_matches && options_match
            })
            .collect();

        // Only sort when prefix is "latest" and we have multiple matches
        // This is expensive, so avoid it for specific version prefixes
        if prefix == "latest" && matching.len() > 1 {
            matching.sort_by(|a, b| {
                versions::Versioning::new(&b.version).cmp(&versions::Versioning::new(&a.version))
            });
        }

        // Priority: 1) env-specific match, 2) base entry (no env)
        if !current_envs.is_empty()
            && let Some(env_match) = matching.iter().find(|t| {
                t.env
                    .as_ref()
                    .is_some_and(|envs| envs.iter().any(|e| current_envs.contains(e.as_str())))
            })
        {
            trace!(
                "[{short}@{prefix}] found {} in lockfile (env-specific: {:?})",
                env_match.version, env_match.env
            );
            return Ok(Some((*env_match).clone()));
        }

        // Fall back to base entry (no env field)
        if let Some(base) = matching.iter().find(|t| t.env.is_none()) {
            trace!(
                "[{short}@{prefix}] found {} in lockfile (base)",
                base.version
            );
            return Ok(Some((*base).clone()));
        }

        // Last resort: any matching entry
        if let Some(any) = matching.first() {
            trace!(
                "[{short}@{prefix}] found {} in lockfile (fallback)",
                any.version
            );
            return Ok(Some((*any).clone()));
        }
    }

    Ok(None)
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
                env: None,
                platforms: Default::default(),
            },
            toml::Value::Table(mut t) => {
                let mut platforms = BTreeMap::new();
                // Handle nested platforms table format: [tools.X.platforms.linux-x64]
                if let Some(platforms_table) = t.remove("platforms") {
                    let platforms_table: toml::Table = platforms_table.try_into()?;
                    for (platform, platform_info) in platforms_table {
                        platforms.insert(platform, platform_info.try_into()?);
                    }
                }
                // Handle inline table format: "platforms.linux-x64" = { ... }
                let platform_keys: Vec<_> = t
                    .keys()
                    .filter(|k| k.starts_with("platforms."))
                    .cloned()
                    .collect();
                for key in platform_keys {
                    if let Some(platform_info) = t.remove(&key) {
                        let platform_name = key.strip_prefix("platforms.").unwrap().to_string();
                        platforms.insert(platform_name, platform_info.try_into()?);
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
                let env = t.remove("env").and_then(|v| match v {
                    toml::Value::Array(arr) => Some(
                        arr.into_iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                    ),
                    _ => None,
                });
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
                    env,
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
        if let Some(env) = self.env {
            let env_arr: toml::Value = env
                .into_iter()
                .map(toml::Value::String)
                .collect::<Vec<_>>()
                .into();
            table.insert("env".to_string(), env_arr);
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
                    env: None, // Set by merge_tool_entries_with_env based on config source
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
                        if b == "version" {
                            return std::cmp::Ordering::Greater;
                        }
                        a.to_string().cmp(&b.to_string())
                    });
                    // Convert platforms to inline tables with dotted keys
                    if let Some(toml_edit::Item::Table(platforms_table)) = t.remove("platforms") {
                        for (platform_key, platform_value) in platforms_table.iter() {
                            if let toml_edit::Item::Table(platform_info) = platform_value {
                                let mut inline = toml_edit::InlineTable::new();
                                for (k, v) in platform_info.iter() {
                                    if let toml_edit::Item::Value(val) = v {
                                        inline.insert(k, val.clone());
                                    }
                                }
                                inline.sort_values();
                                let dotted_key = format!("platforms.{}", platform_key);
                                t.insert(&dotted_key, toml_edit::Item::Value(inline.into()));
                            }
                        }
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
                size: Some(12345678),
                url: Some("https://example.com/node.tar.gz".to_string()),
                url_api: Some("https://api.github.com.com/repos/test/1234".to_string()),
            },
        );

        let tool = LockfileTool {
            version: "20.10.0".to_string(),
            backend: Some("core:node".to_string()),
            options: BTreeMap::new(),
            env: None,
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
            env: None,
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
            env: None,
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

    #[test]
    fn test_lockfile_path_for_config() {
        // Simple case: mise.toml in project root
        let (path, is_local) = lockfile_path_for_config(Path::new("/foo/bar/mise.toml"));
        assert_eq!(path, PathBuf::from("/foo/bar/mise.lock"));
        assert!(!is_local);

        // Local config
        let (path, is_local) = lockfile_path_for_config(Path::new("/foo/bar/mise.local.toml"));
        assert_eq!(path, PathBuf::from("/foo/bar/mise.local.lock"));
        assert!(is_local);

        // Config in .config directory
        let (path, is_local) = lockfile_path_for_config(Path::new("/foo/bar/.config/mise.toml"));
        assert_eq!(path, PathBuf::from("/foo/bar/.config/mise.lock"));
        assert!(!is_local);

        // Config in .mise directory
        let (path, is_local) = lockfile_path_for_config(Path::new("/foo/bar/.mise/config.toml"));
        assert_eq!(path, PathBuf::from("/foo/bar/.mise/mise.lock"));
        assert!(!is_local);

        // Config in conf.d directory - should go to parent of conf.d
        let (path, is_local) =
            lockfile_path_for_config(Path::new("/foo/bar/.mise/conf.d/foo.toml"));
        assert_eq!(path, PathBuf::from("/foo/bar/.mise/mise.lock"));
        assert!(!is_local);

        // Config in .config/mise/conf.d directory
        let (path, is_local) =
            lockfile_path_for_config(Path::new("/foo/bar/.config/mise/conf.d/foo.toml"));
        assert_eq!(path, PathBuf::from("/foo/bar/.config/mise/mise.lock"));
        assert!(!is_local);
    }
}
