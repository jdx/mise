use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::config::Settings;
use crate::http::HTTP;
use crate::toolset::{RawBackendOptions, ToolVersionOptions};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{dirs, file};
use eyre::{Context, Result, bail, ensure};
use flate2::read::GzDecoder;
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
pub static REGISTRY: Lazy<&'static Registry> = Lazy::new(|| {
    if !Settings::get().registry_floating {
        return &BAKED_REGISTRY;
    }

    if !registry_cache_path().exists() {
        return &BAKED_REGISTRY;
    }

    match load_cached_floating_registry() {
        Ok(registry) => Box::leak(Box::new(registry)),
        Err(err) => {
            warn!("failed to load floating mise registry, using baked-in registry: {err:#}");
            &BAKED_REGISTRY
        }
    }
});

const MISE_REGISTRY_ARCHIVE_URL: &str = "https://mise.jdx.dev/registry/latest.tar.gz";

pub struct Registry {
    entries: &'static [(&'static str, RegistryTool)],
    lookup: RegistryLookup,
}

enum RegistryLookup {
    Static(phf::Map<&'static str, usize>),
    Dynamic(HashMap<&'static str, usize>),
}

impl Registry {
    pub fn get(&self, name: &str) -> Option<&'static RegistryTool> {
        self.lookup.get(name).map(|index| &self.entries[*index].1)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.lookup.get(name).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &'static RegistryTool)> {
        self.entries.iter().map(|(name, tool)| (*name, tool))
    }

    pub fn keys(&self) -> impl Iterator<Item = &'static str> {
        self.entries.iter().map(|(name, _)| *name)
    }

    pub fn values(&self) -> impl Iterator<Item = &'static RegistryTool> {
        self.entries.iter().map(|(_, tool)| tool)
    }

    fn dynamic(entries: BTreeMap<String, RegistryTool>) -> Self {
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
pub struct RegistryTool {
    pub short: &'static str,
    pub description: Option<&'static str>,
    pub backends: &'static [RegistryBackend],
    #[allow(unused)]
    pub aliases: &'static [&'static str],
    pub overrides: &'static [&'static str],
    pub test: &'static Option<RegistryToolTest>,
    pub os: &'static [&'static str],
    pub idiomatic_files: &'static [&'static str],
    pub detect: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct RegistryToolTest {
    pub cmd: &'static str,
    pub expected: &'static str,
    pub tools: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct RegistryBackend {
    pub full: &'static str,
    pub platforms: &'static [&'static str],
    pub options: &'static [(&'static str, &'static str)],
}

fn registry_cache_path() -> PathBuf {
    dirs::CACHE.join("mise-registry").join("registry.tar.gz")
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
pub async fn refresh() {
    let settings = Settings::get();
    if !settings.registry_floating || settings.prefer_offline() {
        return;
    }

    let cache_path = registry_cache_path();
    if cache_is_fresh(&cache_path, settings.registry_cache_ttl()) {
        return;
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
        #[cfg(windows)]
        if cache_path.exists() {
            file::remove_file(cache_path)?;
        }
        file::rename(&download_path, cache_path)?;
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

fn parse_registry_archive(path: &Path) -> Result<Registry> {
    let file = File::open(path)?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut sources = BTreeMap::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
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

fn registry_from_sources(sources: BTreeMap<String, String>) -> Result<Registry> {
    let mut entries = BTreeMap::new();
    for (short, source) in sources {
        let value: toml::Value = toml::from_str(&source)
            .wrap_err_with(|| format!("failed to parse registry/{short}.toml"))?;
        let tool = parse_registry_tool(&short, &value)
            .wrap_err_with(|| format!("invalid registry/{short}.toml"))?;
        entries.insert(short, tool.clone());
        for alias in tool.aliases {
            entries.insert((*alias).to_string(), tool.clone());
        }
    }
    Ok(Registry::dynamic(entries))
}

fn parse_registry_tool(short: &str, value: &toml::Value) -> Result<RegistryTool> {
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

    let aliases = string_array(table.get("aliases"), "aliases")?;
    let overrides = string_array(table.get("overrides"), "overrides")?;
    let os = string_array(table.get("os"), "os")?;
    let idiomatic_files = string_array(table.get("idiomatic_files"), "idiomatic_files")?;
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

    Ok(RegistryTool {
        short: leak_string(short.to_string()),
        description,
        backends: leak_vec(backends),
        aliases: leak_vec(aliases),
        overrides: leak_vec(overrides),
        test: Box::leak(Box::new(test)),
        os: leak_vec(os),
        idiomatic_files: leak_vec(idiomatic_files),
        detect: leak_vec(detect),
    })
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
    pub fn backends(&self) -> Vec<&'static str> {
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

    pub fn is_supported_os(&self) -> bool {
        self.os.is_empty() || self.os.contains(&OS)
    }

    pub fn ba(&self) -> Option<BackendArg> {
        self.backends()
            .first()
            .map(|f| BackendArg::new(self.short.to_string(), Some(f.to_string())))
    }

    /// Get RegistryBackend for a specific full backend string
    pub fn get_backend(&self, full: &str) -> Option<&RegistryBackend> {
        self.backends.iter().find(|rb| rb.full == full)
    }

    /// Get options for a specific backend
    pub fn backend_options(&self, full: &str) -> ToolVersionOptions {
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
}

/// Matches registry backend selectors using the schema's normalized platform names.
///
/// Unlike `backends.options.platforms.*` lookup, this is deliberately not
/// alias-tolerant: registry selectors use canonical names such as `macos-x64`,
/// while option lookup accepts release asset aliases such as `darwin-amd64`.
fn backend_matches_platform(platforms: &[&str], settings: &Settings) -> bool {
    let os = settings.os();
    let arch = settings.arch();
    let platform = format!("{os}-{arch}");

    platforms.is_empty()
        || platforms.contains(&os)
        || platforms.contains(&arch)
        || platforms.contains(&platform.as_str())
}

pub fn shorts_for_full(full: &str) -> &'static Vec<&'static str> {
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

pub fn is_trusted_plugin(name: &str, remote: &str) -> bool {
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

pub fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url
        .host_str()
        .ok_or_else(|| eyre::eyre!("URL has no host: {remote}"))?;
    let path = url.path().trim_end_matches(".git");
    Ok(format!("{host}{path}"))
}

pub fn full_to_url(full: &str) -> String {
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
pub fn tool_enabled<T: Ord>(
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
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Cursor;

        let file = tempfile::NamedTempFile::new().unwrap();
        let encoder = GzEncoder::new(file.reopen().unwrap(), Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
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
backends = [
  "aqua:example/tool",
  { full = "github:example/tool", platforms = ["linux-x64"], options = { bin = "example" } },
]
test = { cmd = "example --version", expected = "{{version}}", tools = ["node"] }
"#
            .to_string(),
        )]))
        .unwrap();

        let tool = registry.get("example-alias").unwrap();
        assert_eq!(tool.short, "example");
        assert_eq!(tool.description, Some("Example tool"));
        assert_eq!(tool.backends[0].full, "aqua:example/tool");
        assert_eq!(tool.backends[1].platforms, &["linux-x64"]);
        assert_eq!(
            tool.backend_options("github:example/tool").get("bin"),
            Some("example")
        );
        assert_eq!(tool.test.as_ref().unwrap().tools, &["node"]);
    }

    #[test]
    fn test_registry_archive_only_reads_top_level_registry_directory() {
        use super::*;

        let archive = registry_archive(&[
            ("registry/example.toml", "backends = [\"aqua:good/tool\"]"),
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
            backends: BACKENDS,
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
