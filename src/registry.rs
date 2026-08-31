use crate::backend::backend_type::BackendType;
use crate::backend::options::VersionOrder;
use crate::cli::args::BackendArg;
use crate::config::Settings;
use crate::http::HTTP;
use crate::toolset::{RawBackendOptions, ToolVersionOptions};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{dirs, file};
use eyre::{Context, Result, bail, ensure};
use heck::ToShoutySnakeCase;
use indexmap::IndexMap;
use serde::Serialize as _;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env;
use std::env::consts::OS;
use std::fmt::Display;
use std::fs::File;
use std::io::Read;
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock as Lazy, Mutex};
use std::time::Duration;
use strum::IntoEnumIterator;
use url::Url;

// the registry is generated from registry/ in the project root
static BAKED_REGISTRY: Registry = include!(concat!(env!("OUT_DIR"), "/registry.rs"));

#[cfg(debug_assertions)]
pub(crate) fn baked_registry() -> &'static Registry {
    &BAKED_REGISTRY
}

pub(crate) static REGISTRY: Lazy<&'static Registry> = Lazy::new(|| {
    if !Settings::get().registry_floating {
        return &BAKED_REGISTRY;
    }

    if !registry_cache_path().exists() {
        return &BAKED_REGISTRY;
    }

    match load_cached_floating_registry() {
        Ok(registry) if !registry.missing_version_order => Box::leak(Box::new(registry)),
        Ok(_) => {
            warn!(
                "cached floating mise registry predates version-order metadata, using baked-in registry"
            );
            &BAKED_REGISTRY
        }
        Err(err) => {
            warn!("failed to load floating mise registry, using baked-in registry: {err:#}");
            &BAKED_REGISTRY
        }
    }
});

const MISE_REGISTRY_ARCHIVE_URL: &str = "https://mise.jdx.dev/registry/latest.tar.zst";
const MAX_REGISTRY_ARCHIVE_ENTRIES: usize = 4096;
const MAX_REGISTRY_ARCHIVE_ENTRY_SIZE: u64 = 1024 * 1024;
const MAX_REGISTRY_ARCHIVE_SIZE: u64 = 16 * 1024 * 1024;

pub(crate) struct Registry {
    entries: &'static [(&'static str, RegistryTool)],
    lookup: RegistryLookup,
    missing_version_order: bool,
}

enum RegistryLookup {
    Static(phf::Map<&'static str, usize>),
    Dynamic(HashMap<&'static str, usize>),
}

impl Registry {
    pub(crate) fn get(&self, name: &str) -> Option<&'static RegistryTool> {
        self.lookup.get(name).map(|index| &self.entries[*index].1)
    }

    pub(crate) fn contains_key(&self, name: &str) -> bool {
        self.lookup.get(name).is_some()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&'static str, &'static RegistryTool)> {
        self.entries.iter().map(|(name, tool)| (*name, tool))
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = &'static str> {
        self.entries.iter().map(|(name, _)| *name)
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &'static RegistryTool> {
        self.entries.iter().map(|(_, tool)| tool)
    }

    fn dynamic(entries: BTreeMap<String, RegistryTool>, missing_version_order: bool) -> Self {
        let entries = entries
            .into_iter()
            .map(|(name, tool)| (leak_string(name), tool))
            .collect::<Vec<_>>();
        let entries = leak_vec(entries);
        let lookup = entries
            .iter()
            .enumerate()
            .map(|(index, (name, _))| (*name, index))
            .collect();
        Self {
            entries,
            lookup: RegistryLookup::Dynamic(lookup),
            missing_version_order,
        }
    }
}

impl RegistryLookup {
    fn get(&self, name: &str) -> Option<&usize> {
        match self {
            Self::Static(lookup) => lookup.get(name),
            Self::Dynamic(lookup) => lookup.get(name),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RegistryTool {
    pub short: &'static str,
    pub description: Option<&'static str>,
    pub(crate) version_order: VersionOrder,
    pub backends: &'static [RegistryBackend],
    pub bins: &'static [&'static str],
    #[allow(unused)]
    pub aliases: &'static [&'static str],
    pub overrides: &'static [&'static str],
    pub test: &'static Option<RegistryToolTest>,
    pub os: &'static [&'static str],
    pub idiomatic_files: &'static [RegistryIdiomaticFile],
    pub detect: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub(crate) struct RegistryIdiomaticFile {
    pub path: &'static str,
    pub version_regex: Option<&'static str>,
    pub version_json_path: Option<&'static str>,
    pub version_expr: Option<&'static str>,
    /// Set when this file should no longer be read. The value is the reason, shown in
    /// the deprecation warning emitted when the file still resolves a version. Used for
    /// files that only declare a minimum compatible version rather than the version the
    /// project is built with.
    pub deprecated: Option<&'static str>,
}

impl RegistryIdiomaticFile {
    pub(crate) fn has_parser(&self) -> bool {
        self.version_regex.is_some()
            || self.version_json_path.is_some()
            || self.version_expr.is_some()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RegistryToolTest {
    pub cmd: &'static str,
    pub expected: &'static str,
    pub tools: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub(crate) struct RegistryBackend {
    pub full: &'static str,
    pub platforms: &'static [&'static str],
    pub options: &'static [(&'static str, &'static str)],
}

fn registry_cache_path() -> PathBuf {
    dirs::CACHE.join("mise-registry").join("registry.tar.zst")
}

fn load_cached_floating_registry() -> Result<Registry> {
    parse_registry_archive(&registry_cache_path())
        .wrap_err("failed to load cached floating mise registry")
}

fn cache_is_fresh(path: &Path, ttl: Duration) -> bool {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
        .is_ok_and(|age| age < ttl)
}

/// Refresh the floating mise registry before anything initializes [`REGISTRY`].
/// Fast and offline commands use the cached archive (or the baked registry) without networking.
pub(crate) async fn refresh() {
    let settings = Settings::get();
    if !settings.registry_floating || settings.prefer_offline() {
        return;
    }

    let cache_path = registry_cache_path();
    if cache_is_fresh(&cache_path, settings.registry_cache_ttl()) {
        match parse_registry_archive(&cache_path) {
            Ok(registry) if !registry.missing_version_order => return,
            Ok(_) => warn!(
                "cached floating mise registry predates version-order metadata; refreshing it"
            ),
            Err(_) => warn!("cached floating mise registry is invalid; refreshing it"),
        }
    }

    if let Err(err) = download_registry_archive(&cache_path).await {
        warn!("failed to refresh floating mise registry: {err:#}");
    }
}

async fn download_registry_archive(cache_path: &Path) -> Result<()> {
    let download_path = cache_path.with_extension(format!("download-{}", std::process::id()));
    let pr = MultiProgressReport::get().add_pre_backend("mise registry");
    if let Err(err) = HTTP
        .download_file(MISE_REGISTRY_ARCHIVE_URL, &download_path, Some(pr.as_ref()))
        .await
    {
        let _ = file::remove_file(&download_path);
        pr.abandon();
        return Err(err);
    }

    let result = (|| {
        parse_registry_archive(&download_path)
            .wrap_err("downloaded mise registry archive is invalid")?;
        replace_registry_cache(&download_path, cache_path)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            pr.finish();
            Ok(())
        }
        Err(err) => {
            let _ = file::remove_file(&download_path);
            pr.abandon();
            Err(err)
        }
    }
}

#[cfg(not(windows))]
fn replace_registry_cache(download_path: &Path, cache_path: &Path) -> Result<()> {
    file::rename(download_path, cache_path)
}

#[cfg(windows)]
fn replace_registry_cache(download_path: &Path, cache_path: &Path) -> Result<()> {
    let backup_path = cache_path.with_extension(format!("backup-{}", std::process::id()));
    let had_cache = cache_path.exists();
    if backup_path.exists() {
        file::remove_file(&backup_path)?;
    }
    if had_cache {
        file::rename(cache_path, &backup_path)?;
    }
    if let Err(install_err) = file::rename(download_path, cache_path) {
        if had_cache && let Err(restore_err) = file::rename(&backup_path, cache_path) {
            return Err(install_err).wrap_err(format!(
                "failed to install downloaded registry and restore cached registry: {restore_err:#}"
            ));
        }
        return Err(install_err).wrap_err("failed to install downloaded registry");
    }
    if had_cache {
        file::remove_file(&backup_path)?;
    }
    Ok(())
}

fn parse_registry_archive(path: &Path) -> Result<Registry> {
    let file = File::open(path)?;
    let decoder = zstd::Decoder::new(file)?;
    let mut archive = jdx_tar::Archive::new(decoder);
    let mut sources = BTreeMap::new();
    let mut archive_size = 0_u64;

    for (index, entry) in archive.entries()?.enumerate() {
        let mut entry = entry?;
        track_registry_archive_entry(index, entry.size(), &mut archive_size)?;
        if entry.entry_type() != jdx_tar::EntryType::File {
            continue;
        }
        let path = entry.path()?;
        let components = path
            .components()
            .map(|component| component.as_os_str())
            .collect::<Vec<_>>();
        if components.len() != 2 || components[0] != "registry" {
            continue;
        }
        let file_path = PathBuf::from(components[1]);
        if file_path
            .extension()
            .is_none_or(|extension| extension != "toml")
        {
            continue;
        }
        let short = file_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| eyre::eyre!("invalid registry filename: {}", path.display()))?
            .to_string();
        let mut source = String::new();
        entry.read_to_string(&mut source)?;
        sources.insert(short, source);
    }

    ensure!(
        !sources.is_empty(),
        "archive does not contain registry entries"
    );
    registry_from_sources(sources)
}

fn track_registry_archive_entry(
    index: usize,
    entry_size: u64,
    archive_size: &mut u64,
) -> Result<()> {
    ensure!(
        index < MAX_REGISTRY_ARCHIVE_ENTRIES,
        "registry archive contains too many entries"
    );
    ensure!(
        entry_size <= MAX_REGISTRY_ARCHIVE_ENTRY_SIZE,
        "registry archive entry is too large"
    );
    *archive_size = archive_size
        .checked_add(entry_size)
        .ok_or_else(|| eyre::eyre!("registry archive size overflow"))?;
    ensure!(
        *archive_size <= MAX_REGISTRY_ARCHIVE_SIZE,
        "registry archive is too large"
    );
    Ok(())
}

fn registry_from_sources(sources: BTreeMap<String, String>) -> Result<Registry> {
    let mut entries = BTreeMap::new();
    let mut missing_version_order = false;
    for (short, source) in sources {
        let value: toml::Value = toml::from_str(&source)
            .wrap_err_with(|| format!("failed to parse registry/{short}.toml"))?;
        let (tool, tool_missing_version_order) = parse_registry_tool(&short, &value)
            .wrap_err_with(|| format!("invalid registry/{short}.toml"))?;
        missing_version_order |= tool_missing_version_order;
        entries.insert(short, tool.clone());
        for alias in tool.aliases {
            entries.insert((*alias).to_string(), tool.clone());
        }
    }
    Ok(Registry::dynamic(entries, missing_version_order))
}

fn parse_registry_tool(short: &str, value: &toml::Value) -> Result<(RegistryTool, bool)> {
    let table = value
        .as_table()
        .ok_or_else(|| eyre::eyre!("registry tool must be a TOML table"))?;
    let backends = table
        .get("backends")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| eyre::eyre!("backends must be an array"))?
        .iter()
        .map(parse_registry_backend)
        .collect::<Result<Vec<_>>>()?;
    ensure!(!backends.is_empty(), "backends must not be empty");

    let missing_version_order = !table.contains_key("version_order");
    let version_order = match table.get("version_order").and_then(toml::Value::as_str) {
        Some("source") => VersionOrder::Source,
        Some("semver") => VersionOrder::Semver,
        Some(_) => bail!("version_order must be \"source\" or \"semver\""),
        None => VersionOrder::Source,
    };

    let aliases = string_array(table.get("aliases"), "aliases")?;
    let bins = string_array(table.get("bins"), "bins")?;
    let overrides = string_array(table.get("overrides"), "overrides")?;
    let os = string_array(table.get("os"), "os")?;
    let idiomatic_files = parse_registry_idiomatic_files(table.get("idiomatic_files"))?;
    let detect = string_array(table.get("detect"), "detect")?;
    let description = table
        .get("description")
        .map(|value| {
            value
                .as_str()
                .map(|value| leak_string(value.to_string()))
                .ok_or_else(|| eyre::eyre!("description must be a string"))
        })
        .transpose()?;
    let test = table.get("test").map(parse_registry_test).transpose()?;

    let tool = RegistryTool {
        short: leak_string(short.to_string()),
        description,
        version_order,
        backends: leak_vec(backends),
        bins: leak_vec(bins),
        aliases: leak_vec(aliases),
        overrides: leak_vec(overrides),
        test: Box::leak(Box::new(test)),
        os: leak_vec(os),
        idiomatic_files: leak_vec(idiomatic_files),
        detect: leak_vec(detect),
    };
    Ok((tool, missing_version_order))
}

fn parse_registry_idiomatic_files(
    value: Option<&toml::Value>,
) -> Result<Vec<RegistryIdiomaticFile>> {
    value
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| eyre::eyre!("idiomatic_files must be an array"))?
                .iter()
                .map(parse_registry_idiomatic_file)
                .collect()
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

fn parse_registry_idiomatic_file(value: &toml::Value) -> Result<RegistryIdiomaticFile> {
    match value {
        toml::Value::String(path) => Ok(RegistryIdiomaticFile {
            path: leak_string(path.clone()),
            version_regex: None,
            version_json_path: None,
            version_expr: None,
            deprecated: None,
        }),
        toml::Value::Table(table) => {
            for key in table.keys() {
                ensure!(
                    matches!(
                        key.as_str(),
                        "path"
                            | "version_regex"
                            | "version_json_path"
                            | "version_expr"
                            | "deprecated"
                    ),
                    "unknown idiomatic file field: {key}"
                );
            }
            let string = |key: &str| -> Result<Option<&'static str>> {
                table
                    .get(key)
                    .map(|value| {
                        value
                            .as_str()
                            .map(|value| leak_string(value.to_string()))
                            .ok_or_else(|| eyre::eyre!("idiomatic_files.{key} must be a string"))
                    })
                    .transpose()
            };
            let path = string("path")?
                .ok_or_else(|| eyre::eyre!("idiomatic_files.path must be a string"))?;
            Ok(RegistryIdiomaticFile {
                path,
                version_regex: string("version_regex")?,
                version_json_path: string("version_json_path")?,
                version_expr: string("version_expr")?,
                deprecated: string("deprecated")?,
            })
        }
        _ => Err(eyre::eyre!(
            "idiomatic_files entries must be strings or tables"
        )),
    }
}

fn parse_registry_backend(value: &toml::Value) -> Result<RegistryBackend> {
    match value {
        toml::Value::String(full) => Ok(RegistryBackend {
            full: leak_string(full.clone()),
            platforms: &[],
            options: &[],
        }),
        toml::Value::Table(table) => {
            let full = table
                .get("full")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| eyre::eyre!("backend full must be a string"))?;
            let platforms = string_array(table.get("platforms"), "backend platforms")?;
            let options = table
                .get("options")
                .and_then(toml::Value::as_table)
                .map(|options| {
                    options
                        .iter()
                        .map(|(key, value)| {
                            let mut serialized = String::new();
                            value.serialize(toml::ser::ValueSerializer::new(&mut serialized))?;
                            Ok((leak_string(key.clone()), leak_string(serialized)))
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            Ok(RegistryBackend {
                full: leak_string(full.to_string()),
                platforms: leak_vec(platforms),
                options: leak_vec(options),
            })
        }
        _ => bail!("backend must be a string or table"),
    }
}

fn parse_registry_test(value: &toml::Value) -> Result<RegistryToolTest> {
    let table = value
        .as_table()
        .ok_or_else(|| eyre::eyre!("test must be a table"))?;
    let cmd = table
        .get("cmd")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| eyre::eyre!("test.cmd must be a string"))?;
    let expected = table
        .get("expected")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| eyre::eyre!("test.expected must be a string"))?;
    let tools = string_array(table.get("tools"), "test.tools")?;
    Ok(RegistryToolTest {
        cmd: leak_string(cmd.to_string()),
        expected: leak_string(expected.to_string()),
        tools: leak_vec(tools),
    })
}

fn string_array(value: Option<&toml::Value>, name: &str) -> Result<Vec<&'static str>> {
    value
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| eyre::eyre!("{name} must be an array"))?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(|value| leak_string(value.to_string()))
                        .ok_or_else(|| eyre::eyre!("{name} must contain only strings"))
                })
                .collect()
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

fn leak_string(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn leak_vec<T>(value: Vec<T>) -> &'static [T] {
    Box::leak(value.into_boxed_slice())
}

// Cache for environment variable overrides
static ENV_BACKENDS: Lazy<Mutex<HashMap<String, &'static str>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

impl RegistryTool {
    pub(crate) fn provides_bin(&self, bin_name: &str) -> bool {
        let exe_suffix = std::env::consts::EXE_SUFFIX;
        let bin_name = if exe_suffix.is_empty() {
            bin_name
        } else {
            let suffix_start = bin_name.len().saturating_sub(exe_suffix.len());
            match (bin_name.get(..suffix_start), bin_name.get(suffix_start..)) {
                (Some(name), Some(suffix)) if suffix.eq_ignore_ascii_case(exe_suffix) => name,
                _ => bin_name,
            }
        };
        self.bins.iter().any(|bin| {
            if cfg!(windows) {
                bin.eq_ignore_ascii_case(bin_name)
            } else {
                *bin == bin_name
            }
        })
    }

    pub(crate) fn backends(&self) -> Vec<&'static str> {
        // Check for environment variable override first
        // e.g., MISE_BACKENDS_GRAPHITE='github:withgraphite/homebrew-tap[exe=gt]'
        let env_key = format!("MISE_BACKENDS_{}", self.short.to_shouty_snake_case());

        // Check cache first
        {
            let cache = ENV_BACKENDS.lock().unwrap();
            if let Some(&backend) = cache.get(&env_key) {
                return vec![backend];
            }
        }

        // Check environment variable
        if let Ok(env_value) = env::var(&env_key) {
            // Store in cache with 'static lifetime
            let leaked = Box::leak(env_value.into_boxed_str());
            let mut cache = ENV_BACKENDS.lock().unwrap();
            cache.insert(env_key.clone(), leaked);
            return vec![leaked];
        }

        static BACKEND_TYPES: Lazy<HashSet<String>> = Lazy::new(|| {
            let mut backend_types = BackendType::iter()
                .map(|b| b.to_string())
                .collect::<HashSet<_>>();
            time!("disable_backends");
            for backend in &Settings::get().disable_backends {
                backend_types.remove(backend);
            }
            time!("disable_backends");
            if cfg!(windows) {
                backend_types.remove("asdf");
            }
            backend_types
        });
        let settings = Settings::get();
        let experimental = settings.experimental;
        self.backends
            .iter()
            .filter(|rb| backend_matches_platform(rb.platforms, &settings))
            .map(|rb| rb.full)
            .filter(|full| {
                full.split(':')
                    .next()
                    .is_some_and(|b| BACKEND_TYPES.contains(b))
            })
            // Filter out experimental backends if experimental mode is disabled
            .filter(|full| {
                if experimental {
                    return true;
                }
                let backend_type = BackendType::guess(full);
                !backend_type.is_experimental()
            })
            .collect()
    }

    pub(crate) fn is_supported_os(&self) -> bool {
        self.os.is_empty() || self.os.contains(&OS)
    }

    pub(crate) fn ba(&self) -> Option<BackendArg> {
        self.backends()
            .first()
            .map(|f| BackendArg::new(self.short.to_string(), Some(f.to_string())))
    }

    /// Get RegistryBackend for a specific full backend string
    pub(crate) fn get_backend(&self, full: &str) -> Option<&RegistryBackend> {
        self.backends.iter().find(|rb| rb.full == full)
    }

    /// Get options for a specific backend
    pub(crate) fn backend_options(&self, full: &str) -> ToolVersionOptions {
        let mut opts = IndexMap::new();

        if let Some(backend) = self.get_backend(full) {
            for (k, v) in backend.options {
                let value = v.parse::<toml::Value>().unwrap_or_else(|e| {
                    panic!("failed to parse registry option {k} as a TOML value: {e}")
                });
                opts.insert(k.to_string(), value);
            }
        }

        ToolVersionOptions {
            opts: RawBackendOptions::from(opts),
            ..Default::default()
        }
    }

    pub(crate) fn version_order(&self, full: &str) -> Option<VersionOrder> {
        matches!(
            BackendType::guess(full),
            BackendType::Aqua
                | BackendType::Forgejo
                | BackendType::Github
                | BackendType::Gitlab
                | BackendType::Http
        )
        .then_some(self.version_order)
    }
}

/// Matches registry backend selectors using the schema's normalized platform names.
///
/// Unlike `backends.options.platforms.*` lookup, this is deliberately not
/// alias-tolerant: registry selectors use canonical names such as `macos-x64`,
/// while option lookup accepts release asset aliases such as `darwin-amd64`.
///
/// Windows on arm64 additionally matches the x64 selectors. That is not alias
/// tolerance creeping in — `windows-x64` still names a different platform than
/// `windows-arm64` — it is the same capability rule the aqua backend already
/// applies in `is_platform_supported`: Windows arm64 runs amd64 binaries under
/// emulation. Without it the registry drops a backend here, before aqua is ever
/// asked whether it supports the platform, and aqua would have said yes.
fn backend_matches_platform(platforms: &[&str], settings: &Settings) -> bool {
    let os = settings.os();
    let arch = settings.arch();
    let platform = format!("{os}-{arch}");

    platforms.is_empty()
        || platforms.contains(&os)
        || platforms.contains(&arch)
        || platforms.contains(&platform.as_str())
        || (os == "windows"
            && arch == "arm64"
            && (platforms.contains(&"x64") || platforms.contains(&"windows-x64")))
}

pub(crate) fn shorts_for_full(full: &str) -> &'static Vec<&'static str> {
    static EMPTY: Vec<&'static str> = vec![];
    static FULL_TO_SHORT: Lazy<HashMap<&'static str, Vec<&'static str>>> = Lazy::new(|| {
        let mut map: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
        for (short, rt) in REGISTRY.iter() {
            for full in rt.backends() {
                map.entry(full).or_default().push(short);
            }
        }
        map
    });
    FULL_TO_SHORT.get(full).unwrap_or(&EMPTY)
}

pub(crate) fn is_trusted_plugin(name: &str, remote: &str) -> bool {
    let Ok(normalized_url) = normalize_remote(remote) else {
        return false;
    };
    if normalized_url.starts_with("github.com/mise-plugins/") {
        return true;
    }

    let official_registry_plugin_remotes = || {
        static REMOTES: Lazy<HashSet<String>> = Lazy::new(|| {
            REGISTRY
                .values()
                .flat_map(|tool| tool.backends.iter().map(|backend| backend.full))
                .filter(|full| full.starts_with("asdf:") || full.starts_with("vfox:"))
                .filter_map(|full| normalize_remote(&full_to_url(full)).ok())
                .collect()
        });
        &*REMOTES
    };

    let name_matches_official_remote = REGISTRY.get(name).is_some_and(|tool| {
        tool.backends
            .iter()
            .map(|backend| backend.full)
            .filter(|full| full.starts_with("asdf:") || full.starts_with("vfox:"))
            .filter_map(|full| normalize_remote(&full_to_url(full)).ok())
            .any(|official_remote| official_remote == normalized_url)
    });

    name_matches_official_remote || official_registry_plugin_remotes().contains(&normalized_url)
}

pub(crate) fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url
        .host_str()
        .ok_or_else(|| eyre::eyre!("URL has no host: {remote}"))?;
    let path = url.path().trim_end_matches(".git");
    Ok(format!("{host}{path}"))
}

pub(crate) fn full_to_url(full: &str) -> String {
    if url_like(full) {
        return full.to_string();
    }
    let (_backend, url) = full.split_once(':').unwrap_or(("", full));
    if url_like(url) {
        url.to_string()
    } else {
        format!("https://github.com/{url}.git")
    }
}

pub(crate) fn url_like(s: &str) -> bool {
    s.starts_with("https://")
        || s.starts_with("http://")
        || s.starts_with("git@")
        || s.starts_with("ssh://")
        || s.starts_with("git://")
}

impl Display for RegistryTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short)
    }
}

/// Returns true when `name` passes the configured tool filter.
///
/// `None` means no allowlist is configured, so `disable_tools` excludes
/// individual tools. `Some(empty)` is an explicit empty allowlist and disables
/// every tool. When an allowlist is configured, it is authoritative and
/// `disable_tools` is not applied.
pub(crate) fn tool_enabled<T: Ord>(
    enable_tools: Option<&BTreeSet<T>>,
    disable_tools: &BTreeSet<T>,
    name: &T,
) -> bool {
    match enable_tools {
        Some(enable_tools) => enable_tools.contains(name),
        None => !disable_tools.contains(name),
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    fn registry_archive(entries: &[(&str, &str)]) -> tempfile::NamedTempFile {
        use std::io::Cursor;

        let file = tempfile::NamedTempFile::new().unwrap();
        let encoder = zstd::Encoder::new(file.reopen().unwrap(), 0).unwrap();
        let mut archive = jdx_tar::Builder::new(encoder);
        for (path, contents) in entries {
            let mut header = jdx_tar::Header::new_gnu(jdx_tar::EntryType::File);
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            archive
                .append_data(&mut header, path, Cursor::new(contents.as_bytes()))
                .unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap();
        file
    }

    #[test]
    fn test_dynamic_registry_parses_tools_aliases_and_options() {
        use super::*;

        let registry = registry_from_sources(BTreeMap::from([(
            "example".to_string(),
            r#"
aliases = ["example-alias"]
description = "Example tool"
version_order = "semver"
bins = ["example", "example-helper"]
backends = [
  "aqua:example/tool",
  { full = "github:example/tool", platforms = ["linux-x64"], options = { bin = "example" } },
]
idiomatic_files = [
  ".example-version",
  { path = "example.json", version_json_path = ".tool.version" },
  { path = "example.txt", version_regex = 'version=(\S+)', version_expr = "versions[0]" },
  { path = "example.conf", version_regex = 'minimum=(\S+)', deprecated = "it declares a minimum." },
]
test = { cmd = "example --version", expected = "{{version}}", tools = ["node"] }
"#
            .to_string(),
        )]))
        .unwrap();

        let tool = registry.get("example-alias").unwrap();
        assert_eq!(tool.short, "example");
        assert_eq!(tool.description, Some("Example tool"));
        assert_eq!(tool.bins, &["example", "example-helper"]);
        assert!(tool.provides_bin("example"));
        assert!(!tool.provides_bin("other"));
        if cfg!(windows) {
            assert!(tool.provides_bin("EXAMPLE.EXE"));
        }
        assert_eq!(tool.backends[0].full, "aqua:example/tool");
        assert_eq!(tool.backends[1].platforms, &["linux-x64"]);
        assert_eq!(
            tool.backend_options("github:example/tool").get("bin"),
            Some("example")
        );
        assert_eq!(
            tool.version_order("aqua:example/tool"),
            Some(VersionOrder::Semver)
        );
        assert_eq!(tool.idiomatic_files[0].path, ".example-version");
        assert!(!tool.idiomatic_files[0].has_parser());
        assert_eq!(tool.idiomatic_files[1].path, "example.json");
        assert_eq!(
            tool.idiomatic_files[1].version_json_path,
            Some(".tool.version")
        );
        assert_eq!(
            tool.idiomatic_files[2].version_regex,
            Some(r"version=(\S+)")
        );
        assert_eq!(tool.idiomatic_files[2].version_expr, Some("versions[0]"));
        assert_eq!(tool.idiomatic_files[2].deprecated, None);
        assert_eq!(tool.idiomatic_files[3].path, "example.conf");
        assert_eq!(
            tool.idiomatic_files[3].deprecated,
            Some("it declares a minimum.")
        );
        assert_eq!(tool.test.as_ref().unwrap().tools, &["node"]);
        assert!(!registry.missing_version_order);
    }

    #[test]
    fn test_dynamic_registry_defaults_missing_version_order_to_source() {
        use super::*;

        let registry = registry_from_sources(BTreeMap::from([(
            "example".to_string(),
            "backends = [\"aqua:example/tool\"]".to_string(),
        )]))
        .unwrap();

        assert_eq!(
            registry
                .get("example")
                .unwrap()
                .version_order("aqua:example/tool"),
            Some(VersionOrder::Source)
        );
        assert!(registry.missing_version_order);
    }

    #[test]
    fn test_dynamic_registry_rejects_unknown_idiomatic_file_fields() {
        use super::*;

        let err = registry_from_sources(BTreeMap::from([(
            "example".to_string(),
            r#"
backends = ["aqua:example/tool"]
version_order = "source"
idiomatic_files = [{ path = ".example-version", parser = "shell" }]
"#
            .to_string(),
        )]))
        .err()
        .unwrap();

        assert!(
            format!("{err:#}").contains("unknown idiomatic file field: parser"),
            "{err:#}"
        );
    }

    #[test]
    fn test_registry_archive_only_reads_top_level_registry_directory() {
        use super::*;

        let archive = registry_archive(&[
            (
                "registry/example.toml",
                "backends = [\"aqua:good/tool\"]\nversion_order = \"source\"",
            ),
            (
                "e2e/registry/example.toml",
                "backends = [\"aqua:wrong/tool\"]",
            ),
        ]);
        let registry = parse_registry_archive(archive.path()).unwrap();

        assert_eq!(
            registry.get("example").unwrap().backends[0].full,
            "aqua:good/tool"
        );
    }

    #[test]
    fn test_registry_archive_rejects_nested_registry_directory() {
        use super::*;

        let archive = registry_archive(&[(
            "e2e/registry/example.toml",
            "backends = [\"aqua:wrong/tool\"]",
        )]);

        assert!(parse_registry_archive(archive.path()).is_err());
    }

    #[test]
    fn test_registry_archive_limits() {
        use super::*;

        let mut size = 0;
        assert!(
            track_registry_archive_entry(MAX_REGISTRY_ARCHIVE_ENTRIES, 0, &mut size)
                .unwrap_err()
                .to_string()
                .contains("too many entries")
        );
        assert!(
            track_registry_archive_entry(0, MAX_REGISTRY_ARCHIVE_ENTRY_SIZE + 1, &mut size)
                .unwrap_err()
                .to_string()
                .contains("entry is too large")
        );
        size = MAX_REGISTRY_ARCHIVE_SIZE;
        assert!(
            track_registry_archive_entry(0, 1, &mut size)
                .unwrap_err()
                .to_string()
                .contains("archive is too large")
        );
    }

    #[test]
    fn test_tool_disabled() {
        use super::*;
        let name = "cargo";

        assert!(tool_enabled(None, &BTreeSet::new(), &name));
        assert!(!tool_enabled(
            Some(&BTreeSet::new()),
            &BTreeSet::new(),
            &name
        ));
        assert!(tool_enabled(
            Some(&BTreeSet::from(["cargo"])),
            &BTreeSet::new(),
            &name
        ));
        assert!(!tool_enabled(None, &BTreeSet::from(["cargo"]), &name));
        assert!(tool_enabled(
            Some(&BTreeSet::from(["cargo"])),
            &BTreeSet::from(["cargo"]),
            &name
        ));
    }

    #[test]
    fn test_registry_iteration_is_sorted() {
        use super::*;

        // The interactive tool selector and --all test-tool path consume registry
        // iteration order directly, so keep PHF lookup separate from sorted output.
        let keys = REGISTRY.keys().collect::<Vec<_>>();
        let mut sorted = keys.clone();
        sorted.sort_unstable();

        assert!(!keys.is_empty());
        assert_eq!(keys, sorted);
    }

    #[test]
    fn test_backend_platform_matching_normalizes_settings() {
        use super::*;

        for (raw_os, raw_arch, selector) in [
            ("windows", "x86_64", "windows-x64"),
            ("windows", "amd64", "x64"),
            ("linux", "aarch64", "linux-arm64"),
            ("darwin", "x86_64", "macos-x64"),
        ] {
            let settings = Settings {
                os: Some(raw_os.to_string()),
                arch: Some(raw_arch.to_string()),
                ..Default::default()
            };

            assert!(
                backend_matches_platform(&[selector], &settings),
                "{raw_os}-{raw_arch} should match normalized selector {selector}"
            );
        }
    }

    // A tool's `os` list drops it from the tool request set before any backend is consulted, so a
    // list that no longer matches what the backend can do makes mise refuse a tool that works.
    //
    // All 52 entries whose `os` line left out `windows` were put through `mise install` on Windows
    // and then had whatever landed on disk executed. Five of them produced a working Windows
    // executable -- `entire.exe version` reports `OS/Arch: windows/amd64`, `gitsign.exe --version`
    // reports `gitsign version v0.17.1` -- while their `os` line still said linux and macos only.
    // `acli`, `mimirtool` and `specstory` joined them later, measured the same way, once the
    // vendored aqua snapshot stopped restricting them.
    //
    // Installing is not evidence on its own: eight more installed successfully and unpacked no
    // Windows executable at all (`libsql-server` extracts a source tarball), so they keep their
    // restriction. So do the few the sweep could not settle for reasons that have nothing to do
    // with Windows -- `cocoapods` needs a ruby that is not there, `swift` overflowed the capture.
    // Unsettled is not the same as wrong, and only measured entries were changed.
    //
    // Read from `BAKED_REGISTRY` rather than `REGISTRY`: the claim under test is about the
    // `registry/*.toml` files in this commit, and `REGISTRY` hands back a cached floating registry
    // instead whenever `registry_floating` is on and the cache exists. That would let the test pass
    // against data this commit does not contain.
    //
    // Windows-only on purpose: off Windows every name here is allowed either way, so the assertion
    // would hold without the change and prove nothing.
    #[cfg(windows)]
    #[test]
    fn tools_that_run_on_windows_are_not_restricted_away_from_it() {
        use super::*;

        for short in [
            "entireio-cli",
            "gitsign",
            "go-swagger",
            "grpc-health-probe",
            "httpie-go",
            "acli",
            "mimirtool",
            "specstory",
        ] {
            let rt = BAKED_REGISTRY.get(short).unwrap();
            assert!(rt.is_supported_os(), "{short}: os = {:?}", rt.os);
        }

        // The controls, and they are the point: this is not "remove every os list". Each was
        // checked the same way and each stays. `kpt` ships no Windows asset at all.
        //
        // `docker-slim` is a control twice over: it is also the fixture in
        // `e2e-win/exec_os_unsupported_tool.Tests.ps1`, which asserts the exact message mise prints
        // for a tool this platform is not listed for. Dropping its `os` line would leave that test
        // with nothing to observe, so it fails here first, by name.
        for short in ["docker-slim", "kpt"] {
            let rt = BAKED_REGISTRY.get(short).unwrap();
            assert!(!rt.is_supported_os(), "{short}: os = {:?}", rt.os);
        }
    }

    // `mod tests` imports nothing at this level -- each test brings its own `use super::*` -- so
    // this one names the path.
    fn settings_for(os: &str, arch: &str) -> crate::config::Settings {
        crate::config::Settings {
            os: Some(os.to_string()),
            arch: Some(arch.to_string()),
            ..Default::default()
        }
    }

    // Windows arm64 runs amd64 binaries under emulation. The aqua backend already assumes this in
    // `is_platform_supported`, so a registry selector of `windows-x64` was dropping backends that
    // aqua itself would have accepted -- measured with `MISE_OS`/`MISE_ARCH`, where
    // `mise registry imagemagick` returned only `conda:imagemagick` on windows/arm64 while
    // windows/x64 got `aqua:ImageMagick/ImageMagick` first.
    #[test]
    fn windows_arm64_matches_x64_selectors_the_way_aqua_does() {
        use super::*;

        let settings = settings_for("windows", "arm64");
        for selector in ["windows-x64", "x64"] {
            assert!(
                backend_matches_platform(&[selector], &settings),
                "windows-arm64 should reach the {selector} backend under emulation"
            );
        }
    }

    // The controls. Each one fails if the rule above was written more broadly than intended, and
    // together they are what distinguishes "Windows emulates amd64" from "any arm64 takes x64".
    #[test]
    fn the_x64_fallback_is_confined_to_windows_arm64() {
        use super::*;

        for (os, arch, selector) in [
            // Linux on arm64 cannot execute an x86_64 build, so no backend is the right answer.
            ("linux", "arm64", "linux-x64"),
            // macOS has Rosetta, but aqua declares no rule for it and this change invents none.
            ("macos", "arm64", "macos-x64"),
            // Right platform, wrong OS in the selector.
            ("windows", "arm64", "linux-x64"),
            // Still not alias-tolerant, which the doc comment on the function promises: these name
            // the same machine as `windows-x64` but are not the canonical spelling.
            ("windows", "arm64", "windows-amd64"),
            ("windows", "arm64", "amd64"),
            // And the emulation direction is one-way -- x64 does not get to run arm64 builds.
            ("windows", "x64", "windows-arm64"),
        ] {
            let settings = settings_for(os, arch);
            assert!(
                !backend_matches_platform(&[selector], &settings),
                "{os}-{arch} should not match {selector}"
            );
        }
    }

    // Passing the backend filter is only half of it: the http backend then looks up
    // `platforms.<key>.url`, and `platform_aliases()` has no emulation rule of its own, so
    // windows/arm64 would resolve a backend and fail to find a URL. `android-cli` declares the
    // block explicitly rather than relying on a lookup fallback, and it has to stay pointed at the
    // x64 download -- Google publishes no arm64 build at all.
    #[test]
    fn android_cli_serves_windows_arm64_the_x64_download() {
        use super::*;

        let rt = BAKED_REGISTRY.get("android-cli").unwrap();
        let opts = rt.backend_options("http:android-cli");
        for key in ["url", "checksum_url", "bin"] {
            let arm64 = opts.get_nested_string(&format!("platforms.windows-arm64.{key}"));
            let x64 = opts.get_nested_string(&format!("platforms.windows-x64.{key}"));
            assert!(arm64.is_some(), "platforms.windows-arm64.{key} is missing");
            assert_eq!(arm64, x64, "windows-arm64 {key} should be the x64 one");
        }
    }

    // `pre-commit` ships a `.pyz` zipapp rather than a native binary, so aqua marks the package
    // `supported_envs: [darwin, linux]` and always will -- Windows has nothing to run a shebang
    // with. A short name resolves to `backends().first()` and there is no install-time fallback,
    // so without the `platforms` annotation Windows lands on that backend and stops, with the
    // registered `pipx:` one never reached. Split by platform rather than written as one test
    // because the pair is its own control: the same expression has to answer differently by
    // platform, which is the whole claim.
    //
    // Both first assert that `MISE_BACKENDS_PRE_COMMIT` is unset: `backends()` returns that
    // override ahead of every filter, so a process carrying one would make these pass or fail
    // without touching the registry at all. Asserted rather than cleared, because removing it
    // would mutate process-wide state other tests share.
    //
    // Read from `BAKED_REGISTRY` for the same reason the `os` test above does: the claim is about
    // `registry/pre-commit.toml` in this commit, and `REGISTRY` substitutes a cached floating
    // registry whenever `registry_floating` is on and the cache exists.
    #[cfg(windows)]
    #[test]
    fn pre_commit_falls_through_to_pipx_on_windows() {
        use super::*;

        assert!(env::var("MISE_BACKENDS_PRE_COMMIT").is_err());
        let backends = BAKED_REGISTRY.get("pre-commit").unwrap().backends();
        assert_eq!(
            backends.first().copied(),
            Some("pipx:pre-commit"),
            "{backends:?}"
        );
    }

    // Not `not(windows)`: mise runs on Android too, where `backend_matches_platform` sees `android`
    // and drops the aqua backend along with Windows, so this expectation would be wrong there.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn pre_commit_keeps_the_aqua_backend_off_windows() {
        use super::*;

        assert!(env::var("MISE_BACKENDS_PRE_COMMIT").is_err());
        let backends = BAKED_REGISTRY.get("pre-commit").unwrap().backends();
        assert_eq!(
            backends.first().copied(),
            Some("aqua:pre-commit/pre-commit"),
            "{backends:?}"
        );
    }

    #[test]
    fn test_backend_platform_matching_preserves_os_only_and_order() {
        use super::*;

        let settings = Settings {
            os: Some("darwin".to_string()),
            arch: Some("amd64".to_string()),
            ..Default::default()
        };
        let backends = [
            RegistryBackend {
                full: "aqua:first/tool",
                platforms: &["macos"],
                options: &[],
            },
            RegistryBackend {
                full: "github:second/tool",
                platforms: &["macos-x64"],
                options: &[],
            },
            RegistryBackend {
                full: "cargo:third-tool",
                platforms: &[],
                options: &[],
            },
            RegistryBackend {
                full: "npm:excluded-tool",
                platforms: &["linux"],
                options: &[],
            },
        ];

        let matching = backends
            .iter()
            .filter(|backend| backend_matches_platform(backend.platforms, &settings))
            .map(|backend| backend.full)
            .collect::<Vec<_>>();

        assert_eq!(
            matching,
            ["aqua:first/tool", "github:second/tool", "cargo:third-tool"]
        );

        let alias_selector = RegistryBackend {
            full: "github:owner/repo",
            platforms: &["darwin-amd64"],
            options: &[],
        };
        assert!(!backend_matches_platform(
            alias_selector.platforms,
            &settings
        ));
    }

    #[test]
    fn test_backend_options_parse_toml_values() {
        use super::*;

        static OPTIONS: &[(&str, &str)] = &[
            ("bin", r#""rg""#),
            ("prerelease", "true"),
            ("strip_components", "1"),
            (
                "targets",
                r#"["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]"#,
            ),
            (
                "platforms",
                r#"{ linux-x64 = { asset_pattern = "tool-linux.tar.gz" } }"#,
            ),
        ];
        static BACKENDS: &[RegistryBackend] = &[RegistryBackend {
            full: "github:owner/repo",
            platforms: &[],
            options: OPTIONS,
        }];
        let tool = RegistryTool {
            short: "test",
            description: None,
            version_order: VersionOrder::Source,
            backends: BACKENDS,
            bins: &[],
            aliases: &[],
            overrides: &[],
            test: &None,
            os: &[],
            idiomatic_files: &[],
            detect: &[],
        };

        let opts = tool.backend_options("github:owner/repo");

        assert_eq!(opts.get("bin"), Some("rg"));
        assert_eq!(
            opts.opts.get("prerelease"),
            Some(&toml::Value::Boolean(true))
        );
        assert_eq!(
            opts.opts.get("strip_components"),
            Some(&toml::Value::Integer(1))
        );
        assert!(opts.opts.get("targets").is_some_and(toml::Value::is_array));
        assert_eq!(
            opts.get_nested_string("platforms.linux-x64.asset_pattern"),
            Some("tool-linux.tar.gz".to_string())
        );
    }

    #[test]
    fn test_semver_registry_order_only_applies_to_supported_backends() {
        use super::*;

        static BACKENDS: &[RegistryBackend] = &[
            RegistryBackend {
                full: "aqua:owner/repo",
                platforms: &[],
                options: &[],
            },
            RegistryBackend {
                full: "npm:package",
                platforms: &[],
                options: &[],
            },
        ];
        let tool = RegistryTool {
            short: "test",
            description: None,
            version_order: VersionOrder::Semver,
            backends: BACKENDS,
            bins: &[],
            aliases: &[],
            overrides: &[],
            test: &None,
            os: &[],
            idiomatic_files: &[],
            detect: &[],
        };

        assert_eq!(
            tool.version_order("aqua:owner/repo"),
            Some(VersionOrder::Semver)
        );
        assert_eq!(tool.version_order("npm:package"), None);
    }

    #[tokio::test]
    async fn test_backend_env_override() {
        let _config = Config::get().await.unwrap();
        use super::*;

        // Clear the cache first
        ENV_BACKENDS.lock().unwrap().clear();

        // Test with a known tool from the registry
        if let Some(tool) = REGISTRY.get("node") {
            // First test without env var - should return default backends
            let default_backends = tool.backends();
            assert!(!default_backends.is_empty());

            // Test with env var override
            // SAFETY: This is safe in a test environment
            unsafe {
                env::set_var("MISE_BACKENDS_NODE", "test:backend");
            }
            let overridden_backends = tool.backends();
            assert_eq!(overridden_backends.len(), 1);
            assert_eq!(overridden_backends[0], "test:backend");

            // Clean up
            // SAFETY: This is safe in a test environment
            unsafe {
                env::remove_var("MISE_BACKENDS_NODE");
            }
            ENV_BACKENDS.lock().unwrap().clear();
        }
    }

    #[test]
    fn test_normalize_remote() {
        use super::*;

        // Standard HTTPS URLs should work
        let result = normalize_remote("https://github.com/mise-plugins/vfox-node.git");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "github.com/mise-plugins/vfox-node");

        // file:// URLs should return an error (no host)
        let result = normalize_remote("file:///path/to/repo");
        assert!(result.is_err());

        // Invalid URLs should return an error
        let result = normalize_remote("not-a-url");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_trusted_plugin_rejects_non_normalizable_remote() {
        use super::*;

        assert!(!is_trusted_plugin("cmake", "not-a-url"));
    }

    #[test]
    fn test_is_trusted_plugin_rejects_non_registry_plugin_url() {
        use super::*;

        assert!(!is_trusted_plugin(
            "vfox-attacker-evil",
            "https://github.com/attacker/evil.git"
        ));
    }

    #[test]
    fn test_is_trusted_plugin_accepts_official_registry_plugin_url() {
        use super::*;

        assert!(is_trusted_plugin(
            "cmake",
            "https://github.com/mise-plugins/vfox-cmake.git"
        ));
        assert!(is_trusted_plugin(
            "vfox-jdx-vfox-mongod",
            "https://github.com/jdx/vfox-mongod.git"
        ));
    }

    #[test]
    fn test_is_trusted_plugin_rejects_shorthand_mismatch() {
        use super::*;

        assert!(!is_trusted_plugin(
            "cmake",
            "https://github.com/attacker/vfox-cmake.git"
        ));
    }
}
