//! mise's settings: the types generated from `settings.toml` and the
//! process-wide cache that [`Settings::get`] reads.
//!
//! Loading settings means discovering config files, checking trust, and
//! rendering templates, all of which live in mise itself. mise registers that
//! loader once at startup with [`set_loader`]; [`Settings::try_get`] returns the
//! cached value or runs the loader. Keeping only the types and the cache here
//! lets lower-level crates read settings without depending on mise's config
//! system.

use confique::Config;
use confique::env::parse::{list_by_colon, list_by_comma};
use eyre::{Result, bail};
use indexmap::{IndexMap, indexmap};
use itertools::Itertools;
use serde::ser::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeSet, HashSet};
use std::env::consts::{ARCH, OS};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock as Lazy;
use std::sync::{Arc, OnceLock, RwLock};

// settings are generated from settings.toml in the project root
// make sure you run `mise run render` after updating settings.toml
include!(concat!(env!("OUT_DIR"), "/settings.rs"));

pub enum SettingsType {
    Bool,
    String,
    Integer,
    Duration,
    Path,
    Url,
    ListString,
    ListPath,
    SetString,
    IndexMap,
    BoolOrString,
}

pub struct SettingsMeta {
    // pub key: String,
    pub type_: SettingsType,
    pub description: &'static str,
    pub env: Option<&'static str>,
    pub deprecated: Option<&'static str>,
    pub deprecated_warn_at: Option<&'static str>,
    pub deprecated_remove_at: Option<&'static str>,
    pub global_only: bool,
    /// Consumed before config files are read, so a value in one can never apply.
    pub env_only: bool,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SettingsStatusMissingTools {
    /// never show the warning
    Never,
    /// hide this warning if the user hasn't installed at least 1 version of the tool before
    #[default]
    IfOtherVersionsInstalled,
    /// always show the warning if tools are missing
    Always,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum NpmPackageManager {
    #[default]
    Auto,
    Npm,
    Aube,
    AubeCli,
    Bun,
    Pnpm,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SystemDepsMode {
    /// prompt to install missing plugin system dependencies (falls back to `warn` non-interactively)
    #[default]
    Prompt,
    /// install missing plugin system dependencies without prompting
    Auto,
    /// print missing plugin system dependencies and continue
    Warn,
    /// skip the plugin system dependency check
    Ignore,
}

/// How task output is presented. mise's task runner resolves it further; see
/// `TaskOutputExt` there.
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    strum::Display,
    strum::EnumString,
    strum::EnumIs,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum TaskOutput {
    Interleave,
    KeepOrder,
    #[default]
    Prefix,
    Replacing,
    Timed,
    Quiet,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PythonUvVenvAuto {
    #[default]
    Off,
    Source,
    CreateSource,
    LegacyTrue,
}

impl PythonUvVenvAuto {
    pub fn should_source(self) -> bool {
        matches!(self, Self::Source | Self::CreateSource | Self::LegacyTrue)
    }

    pub fn should_create(self) -> bool {
        matches!(self, Self::CreateSource | Self::LegacyTrue)
    }

    /// `true`, which mise deprecated in favor of `"create|source"`. mise warns when a
    /// loaded setting still uses it.
    pub fn is_legacy_true(self) -> bool {
        matches!(self, Self::LegacyTrue)
    }
}

impl<'de> Deserialize<'de> for PythonUvVenvAuto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct PythonUvVenvAutoVisitor;

        impl<'de> Visitor<'de> for PythonUvVenvAutoVisitor {
            type Value = PythonUvVenvAuto;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a boolean, \"source\", or \"create|source\"")
            }

            fn visit_bool<E>(self, value: bool) -> Result<PythonUvVenvAuto, E>
            where
                E: de::Error,
            {
                Ok(if value {
                    PythonUvVenvAuto::LegacyTrue
                } else {
                    PythonUvVenvAuto::Off
                })
            }

            fn visit_str<E>(self, value: &str) -> Result<PythonUvVenvAuto, E>
            where
                E: de::Error,
            {
                let normalized = value.trim().to_ascii_lowercase();
                match normalized.as_str() {
                    "source" => Ok(PythonUvVenvAuto::Source),
                    "create|source" => Ok(PythonUvVenvAuto::CreateSource),
                    "true" | "yes" | "1" => self.visit_bool(true),
                    "false" | "no" | "0" => self.visit_bool(false),
                    _ => Err(E::invalid_value(de::Unexpected::Str(value), &self)),
                }
            }

            fn visit_string<E>(self, value: String) -> Result<PythonUvVenvAuto, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }
        }

        deserializer.deserialize_any(PythonUvVenvAutoVisitor)
    }
}

impl serde::Serialize for PythonUvVenvAuto {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            PythonUvVenvAuto::Off => serializer.serialize_bool(false),
            PythonUvVenvAuto::LegacyTrue => serializer.serialize_bool(true),
            PythonUvVenvAuto::Source => serializer.serialize_str("source"),
            PythonUvVenvAuto::CreateSource => serializer.serialize_str("create|source"),
        }
    }
}

fn remove_empty_nested_settings(table: &mut toml::Table, prefix: &str) {
    table.retain(|key, value| {
        let path = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        let Some(child) = value.as_table_mut() else {
            return true;
        };
        remove_empty_nested_settings(child, &path);
        !child.is_empty() || SETTINGS_META.contains_key(path.as_str())
    });
}

/// Replace secret values in a serialized settings table.
pub fn redact_settings_table(table: &mut toml::Table) {
    let Some(cache) = table
        .get_mut("task")
        .and_then(toml::Value::as_table_mut)
        .and_then(|task| task.get_mut("cache"))
        .and_then(toml::Value::as_table_mut)
    else {
        return;
    };
    if cache.contains_key("remote_token") {
        cache.insert(
            "remote_token".to_string(),
            toml::Value::String("[redacted]".to_string()),
        );
    }
}

pub type SettingsPartial = <Settings as Config>::Layer;

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match toml::to_string_pretty(self) {
            Ok(s) => write!(f, "{s}"),
            Err(e) => Err(std::fmt::Error::custom(e)),
        }
    }
}

impl SettingsStatus {
    pub fn missing_tools(&self) -> SettingsStatusMissingTools {
        SettingsStatusMissingTools::from_str(&self.missing_tools).unwrap()
    }
}

/// Deserialize a string to a boolean, accepting "false", "no", "0"
/// and their case-insensitive variants as `false`. Any other value (incl. "") is considered `true`.
fn bool_string<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "false" | "no" | "0" => Ok(false),
        _ => Ok(true),
    }
}

fn set_by_comma<T, C>(input: &str) -> Result<C, <T as FromStr>::Err>
where
    T: FromStr + Eq + Ord,
    C: FromIterator<T>,
{
    input
        .split(',')
        // Filter out empty strings
        .filter_map(|s| {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                Some(T::from_str(trimmed))
            } else {
                None
            }
        })
        // collect into BTreeSet to remove duplicates
        .collect::<Result<BTreeSet<_>, _>>()
        .map(|set| set.into_iter().collect())
}

fn validate_setting_enum_values<'a>(
    name: &str,
    values: impl IntoIterator<Item = &'a str>,
    allowed: &[&str],
) -> Result<()> {
    if let Some(invalid) = values.into_iter().find(|value| !allowed.contains(value)) {
        bail!(
            "invalid {name} value {invalid:?}; expected one of: {}",
            allowed.join(", ")
        );
    }
    Ok(())
}

fn normalize_tool_names(tools: &BTreeSet<String>) -> BTreeSet<String> {
    tools
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parse URL replacements from JSON string format
/// Expected format: {"source_domain": "replacement_domain", ...}
pub fn parse_url_replacements(input: &str) -> Result<IndexMap<String, String>, serde_json::Error> {
    serde_json::from_str(input)
}

/// Parse a path list from an environment variable using the OS-native path
/// separator (`:` on Unix, `;` on Windows). This correctly handles Windows
/// absolute paths whose drive letters contain `:` (e.g. `C:\foo`).
fn list_by_os_path_separator<C>(input: &str) -> Result<C, std::convert::Infallible>
where
    C: FromIterator<PathBuf>,
{
    Ok(std::env::split_paths(input)
        .filter(|p| !p.as_os_str().is_empty())
        .collect())
}

/// Builds settings from every source and returns them. [`Settings::try_get`]
/// caches the result unless [`clear`] or [`store`] ran while it was loading.
pub type Loader = fn() -> Result<Arc<Settings>>;

static LOADER: OnceLock<Loader> = OnceLock::new();
static CURRENT: SettingsCache = SettingsCache::new();

/// The cached settings, plus a generation that every [`clear`] and [`store`]
/// bumps. A load that started before a bump read inputs (CLI overrides, env,
/// config files) that may since have changed, so its result is not cached.
struct SettingsCache {
    state: RwLock<CacheState>,
}

struct CacheState {
    generation: u64,
    settings: Option<Arc<Settings>>,
}

impl SettingsCache {
    const fn new() -> Self {
        Self {
            state: RwLock::new(CacheState {
                generation: 0,
                settings: None,
            }),
        }
    }

    fn is_loaded(&self) -> bool {
        self.state.read().unwrap().settings.is_some()
    }

    fn store(&self, settings: Arc<Settings>) {
        let mut state = self.state.write().unwrap();
        state.generation += 1;
        state.settings = Some(settings);
    }

    fn clear(&self) {
        let mut state = self.state.write().unwrap();
        state.generation += 1;
        state.settings = None;
    }

    fn get_or_load(&self, loader: impl FnOnce() -> Result<Arc<Settings>>) -> Result<Arc<Settings>> {
        let generation = {
            let state = self.state.read().unwrap();
            if let Some(settings) = &state.settings {
                return Ok(settings.clone());
            }
            state.generation
        };
        let loaded = loader()?;
        let mut state = self.state.write().unwrap();
        if state.generation != generation {
            // Settings were cleared or replaced mid-load. The caller asked
            // before that happened, so it gets what it loaded, but the next
            // read must not see this stale snapshot.
            return Ok(loaded);
        }
        // Another thread in the same generation may have finished first;
        // keep its value so every reader shares one snapshot.
        Ok(state.settings.get_or_insert(loaded).clone())
    }
}

/// Register the function [`Settings::try_get`] calls when nothing is cached.
///
/// mise calls this before anything reads settings. Only the first registration
/// takes effect.
pub fn set_loader(loader: Loader) {
    let _ = LOADER.set(loader);
}

/// Whether settings have been loaded since the last [`clear`].
pub fn is_loaded() -> bool {
    CURRENT.is_loaded()
}

/// Cache `settings` as the value [`Settings::get`] returns until the next [`clear`].
pub fn store(settings: Arc<Settings>) {
    CURRENT.store(settings);
}

/// Drop the cached settings so the next [`Settings::get`] runs the loader again.
pub fn clear() {
    CURRENT.clear();
}

/// A [`Loader`] that ignores config files and the environment: every setting
/// takes its `settings.toml` default. For the unit tests of crates below mise,
/// which have no config system to load from.
pub fn load_defaults() -> Result<Arc<Settings>> {
    Ok(Arc::new(Settings::builder().load()?))
}

impl Settings {
    pub fn get() -> Arc<Self> {
        Self::try_get().unwrap()
    }

    pub fn try_get() -> Result<Arc<Self>> {
        CURRENT.get_or_load(|| {
            let loader = LOADER
                .get()
                .expect("mise_settings::set_loader must be called before reading settings");
            loader()
        })
    }

    pub fn parse_default_package_line(package: &str) -> Option<String> {
        let package = package.split('#').next().unwrap_or_default().trim();
        (!package.is_empty()).then(|| package.to_string())
    }

    pub fn hidden_configs() -> &'static HashSet<&'static str> {
        static HIDDEN_CONFIGS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
            [
                "ci",
                "cd",
                "debug",
                "env_file",
                "install_before",
                "trace",
                "log_level",
            ]
            .into()
        });
        &HIDDEN_CONFIGS
    }

    pub fn lockfile_enabled(&self) -> bool {
        self.lockfile.unwrap_or(true)
    }

    pub fn generate_lockfiles(&self) -> bool {
        self.lockfile_mode.as_deref() == Some("generate")
    }

    pub fn validate_lockfile_mode(&self) -> Result<()> {
        validate_setting_enum_values(
            "lockfile_mode",
            self.lockfile_mode.as_deref(),
            &["merge", "generate"],
        )
    }

    pub fn lockfile_creation_enabled(&self) -> bool {
        self.lockfile == Some(true)
    }

    pub fn force_provenance_verify(&self) -> bool {
        self.locked_verify_provenance || self.paranoid
    }

    pub fn ensure_experimental(&self, what: &str) -> Result<()> {
        if !self.experimental {
            bail!("{what} is experimental. Enable it with `mise settings experimental=true`");
        }
        Ok(())
    }

    pub fn log_level(&self) -> log::LevelFilter {
        self.log_level.parse().unwrap_or(log::LevelFilter::Info)
    }

    pub fn disable_tools(&self) -> BTreeSet<String> {
        normalize_tool_names(&self.disable_tools)
    }

    pub fn enable_tools(&self) -> Option<BTreeSet<String>> {
        self.enable_tools.as_ref().map(normalize_tool_names)
    }

    pub fn partial_as_dict(partial: &SettingsPartial) -> eyre::Result<toml::Table> {
        let s = toml::to_string(partial)?;
        let mut table = toml::from_str(&s)?;
        remove_empty_nested_settings(&mut table, "");
        redact_settings_table(&mut table);
        Ok(table)
    }

    pub fn os(&self) -> &str {
        match self.os.as_deref().unwrap_or(OS) {
            "darwin" | "macos" => "macos",
            "linux" => "linux",
            "windows" => "windows",
            other => other,
        }
    }

    pub fn arch(&self) -> &str {
        match self.arch.as_deref().unwrap_or(ARCH) {
            "x86_64" | "amd64" => "x64",
            "aarch64" | "arm64" => "arm64",
            other => other,
        }
    }

    pub fn libc(&self) -> Option<&str> {
        match self.libc.as_deref()?.to_ascii_lowercase().as_str() {
            "glibc" | "gnu" => Some("gnu"),
            "musl" => Some("musl"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Arc<Settings> {
        load_defaults().unwrap()
    }

    #[test]
    fn test_get_or_load_caches_result() {
        let cache = SettingsCache::new();
        let loaded = cache.get_or_load(|| Ok(defaults())).unwrap();
        assert!(cache.is_loaded());
        let cached = cache.get_or_load(|| panic!("loader rerun")).unwrap();
        assert!(Arc::ptr_eq(&loaded, &cached));
    }

    /// Thread A starts loading, thread B changes an input and clears, then A
    /// finishes. A's snapshot predates B's change, so it must not be cached.
    #[test]
    fn test_get_or_load_discards_load_cleared_midway() {
        let cache = SettingsCache::new();
        let stale = cache
            .get_or_load(|| {
                let stale = defaults();
                cache.clear();
                Ok(stale)
            })
            .unwrap();
        assert!(!cache.is_loaded());

        let fresh = cache.get_or_load(|| Ok(defaults())).unwrap();
        assert!(!Arc::ptr_eq(&stale, &fresh));
        let cached = cache.get_or_load(|| panic!("loader rerun")).unwrap();
        assert!(Arc::ptr_eq(&fresh, &cached));
    }

    /// A load must not overwrite settings another thread stored mid-load.
    #[test]
    fn test_get_or_load_keeps_store_made_midway() {
        let cache = SettingsCache::new();
        let stored = defaults();
        let stale = cache
            .get_or_load(|| {
                cache.store(stored.clone());
                Ok(defaults())
            })
            .unwrap();
        let cached = cache.get_or_load(|| panic!("loader rerun")).unwrap();
        assert!(Arc::ptr_eq(&stored, &cached));
        assert!(!Arc::ptr_eq(&stale, &cached));
    }

    /// Two loads in the same generation: the first to finish wins, and the
    /// second caller gets that same snapshot.
    #[test]
    fn test_get_or_load_concurrent_same_generation_shares_snapshot() {
        let cache = SettingsCache::new();
        let first = std::cell::OnceCell::new();
        let second = cache
            .get_or_load(|| {
                first
                    .set(cache.get_or_load(|| Ok(defaults())).unwrap())
                    .unwrap();
                Ok(defaults())
            })
            .unwrap();
        assert!(Arc::ptr_eq(first.get().unwrap(), &second));
    }

    #[test]
    fn test_set_by_comma_empty_string() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BTreeSet::new());
    }

    #[test]
    fn test_set_by_comma_whitespace_only() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("  ");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BTreeSet::new());
    }

    #[test]
    fn test_set_by_comma_single_value() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo");
        assert!(result.is_ok());
        let expected: BTreeSet<String> = ["foo".to_string()].into_iter().collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_multiple_values() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo,bar,baz");
        assert!(result.is_ok());
        let expected: BTreeSet<String> = ["foo".to_string(), "bar".to_string(), "baz".to_string()]
            .into_iter()
            .collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_with_whitespace() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo, bar, baz");
        assert!(result.is_ok());
        let expected: BTreeSet<String> = ["foo".to_string(), "bar".to_string(), "baz".to_string()]
            .into_iter()
            .collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_trailing_comma() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo,bar,");
        assert!(result.is_ok());
        let expected: BTreeSet<String> =
            ["foo".to_string(), "bar".to_string()].into_iter().collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_duplicate_values() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo,bar,foo");
        assert!(result.is_ok());
        let expected: BTreeSet<String> =
            ["foo".to_string(), "bar".to_string()].into_iter().collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_set_by_comma_empty_elements() {
        let result: Result<BTreeSet<String>, _> = set_by_comma("foo,,bar");
        assert!(result.is_ok());
        let expected: BTreeSet<String> =
            ["foo".to_string(), "bar".to_string()].into_iter().collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_normalize_tool_names() {
        let tools = BTreeSet::from([
            " node ".to_string(),
            "  ".to_string(),
            "ruby".to_string(),
            "".to_string(),
        ]);
        let expected = BTreeSet::from(["node".to_string(), "ruby".to_string()]);
        assert_eq!(normalize_tool_names(&tools), expected);
    }

    #[test]
    fn test_list_by_os_path_separator_empty() {
        let result: Result<Vec<PathBuf>, _> = list_by_os_path_separator("");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_list_by_os_path_separator_single() {
        #[cfg(not(windows))]
        let (input, expected) = ("/foo/bar", PathBuf::from("/foo/bar"));
        #[cfg(windows)]
        let (input, expected) = (r"C:\foo\bar", PathBuf::from(r"C:\foo\bar"));
        let result: Vec<PathBuf> = list_by_os_path_separator(input).unwrap();
        assert_eq!(result, vec![expected]);
    }

    #[test]
    #[cfg(unix)]
    fn test_list_by_os_path_separator_multiple_unix() {
        let result: Vec<PathBuf> = list_by_os_path_separator("/foo:/bar").unwrap();
        assert_eq!(result, vec![PathBuf::from("/foo"), PathBuf::from("/bar")]);
    }

    #[test]
    #[cfg(windows)]
    fn test_list_by_os_path_separator_multiple_windows() {
        let result: Vec<PathBuf> = list_by_os_path_separator(r"C:\foo;D:\bar").unwrap();
        assert_eq!(
            result,
            vec![PathBuf::from(r"C:\foo"), PathBuf::from(r"D:\bar")]
        );
    }

    #[test]
    fn test_list_by_os_path_separator_as_btreeset() {
        // Verify the function works with BTreeSet as the collection type,
        // matching the field types used in Settings (e.g. trusted_config_paths).
        #[cfg(not(windows))]
        let (input, a, b) = ("/foo:/bar", PathBuf::from("/foo"), PathBuf::from("/bar"));
        #[cfg(windows)]
        let (input, a, b) = (
            r"C:\foo;D:\bar",
            PathBuf::from(r"C:\foo"),
            PathBuf::from(r"D:\bar"),
        );
        let result: BTreeSet<PathBuf> = list_by_os_path_separator(input).unwrap();
        assert_eq!(result, [a, b].into_iter().collect());
    }

    /// The workspace-root settings.toml, or the copy a published package carries. Same
    /// precedence as `build.rs`.
    fn settings_toml_path() -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir.join("../..");
        let in_workspace = std::fs::canonicalize(root.join("crates/mise-settings"))
            .ok()
            .zip(std::fs::canonicalize(&manifest_dir).ok())
            .is_some_and(|(a, b)| a == b)
            && std::fs::read_to_string(root.join("Cargo.toml"))
                .ok()
                .and_then(|manifest| manifest.parse::<toml::Table>().ok())
                .is_some_and(|manifest| {
                    manifest
                        .get("package")
                        .and_then(|package| package.get("name"))
                        .and_then(toml::Value::as_str)
                        == Some("mise")
                });
        let workspace = root.join("settings.toml");
        if in_workspace && workspace.exists() {
            workspace
        } else {
            manifest_dir.join("settings.toml")
        }
    }

    #[test]
    fn test_settings_toml_is_sorted() {
        let content =
            std::fs::read_to_string(settings_toml_path()).expect("failed to read settings.toml");
        let table: toml::Table = content.parse().expect("failed to parse settings.toml");

        fn collect_keys(table: &toml::Table, prefix: &str) -> Vec<String> {
            let mut keys = Vec::new();
            for (key, value) in table {
                let full_key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if let toml::Value::Table(sub) = value {
                    // A nested table that has no "type" or "description" is a grouping table
                    // (e.g., [aqua], [node]), not a setting itself.
                    if !sub.contains_key("type") && !sub.contains_key("description") {
                        keys.extend(collect_keys(sub, &full_key));
                        continue;
                    }
                }
                keys.push(full_key);
            }
            keys
        }

        let keys = collect_keys(&table, "");
        let mut sorted = keys.clone();
        sorted.sort();

        for (i, (got, expected)) in keys.iter().zip(sorted.iter()).enumerate() {
            assert_eq!(
                got, expected,
                "settings.toml is not alphabetically sorted at index {i}: found \"{got}\", expected \"{expected}\". Run the sort script or reorder manually."
            );
        }
    }

    /// Every scalar setting type in settings.toml. Anything else is a collection.
    ///
    /// Spelled as "not a scalar" rather than by listing the collections on purpose. A new
    /// collection type — `SetPath`, say — has the same failure mode and is caught here
    /// automatically, whereas enumerating `List*`/`SetString`/`IndexMap<…>` would let it through
    /// silently, which is the exact failure this guard exists to prevent. A new *scalar* type
    /// instead fails this test loudly and is fixed by adding one entry here, which is the cheaper
    /// mistake to make.
    const SCALAR_SETTING_TYPES: &[&str] = &[
        "Bool",
        "BoolOrString",
        "Duration",
        "Integer",
        "Path",
        "String",
        "Url",
    ];

    /// Collect settings that are readable from the environment, hold a collection, and declare no
    /// `parse_env`.
    fn collect_settings_missing_parse_env(
        table: &toml::Table,
        prefix: &str,
        missing: &mut Vec<String>,
    ) {
        for (key, value) in table {
            let toml::Value::Table(setting) = value else {
                continue;
            };
            let full_key = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            // A nested table that has no "type" or "description" is a grouping table
            // (e.g., [aqua], [node]), not a setting itself.
            if !setting.contains_key("type") && !setting.contains_key("description") {
                collect_settings_missing_parse_env(setting, &full_key, missing);
                continue;
            }
            let is_collection = setting
                .get("type")
                .and_then(|type_| type_.as_str())
                .is_some_and(|type_| !SCALAR_SETTING_TYPES.contains(&type_));
            if is_collection && setting.contains_key("env") && !setting.contains_key("parse_env") {
                missing.push(full_key);
            }
        }
    }

    #[test]
    fn test_settings_toml_collection_settings_declare_parse_env() {
        // A collection setting that can be set from the environment needs `parse_env`. Without it
        // confique hands the raw string to a `Vec`, set or map deserializer and mise aborts before
        // doing anything: "failed to deserialize value `SettingsAge::identity_files` from
        // environment variable `MISE_AGE_IDENTITY_FILES`: invalid type: string "...", expected a
        // sequence".
        let content =
            std::fs::read_to_string(settings_toml_path()).expect("failed to read settings.toml");
        let table: toml::Table = content.parse().expect("failed to parse settings.toml");

        let mut missing = vec![];
        collect_settings_missing_parse_env(&table, "", &mut missing);

        assert!(
            missing.is_empty(),
            "these collection settings are readable from the environment but declare no \
             parse_env, so mise panics as soon as one is set: {missing:?}"
        );
    }

    /// The guard above only earns its place if it catches every collection type, not just `List*`.
    /// settings.toml also carries `SetString` and `IndexMap<String, String>`, which fail the same
    /// way, so a violation of each is checked against a fixture here rather than waiting for one
    /// to be committed.
    #[test]
    fn test_parse_env_guard_covers_sets_and_maps_too() {
        let table: toml::Table = r#"
            [bad_list]
            description = "x"
            type = "ListString"
            env = "MISE_BAD_LIST"

            [bad_set]
            description = "x"
            type = "SetString"
            env = "MISE_BAD_SET"

            [bad_map]
            description = "x"
            type = "IndexMap<String, String>"
            env = "MISE_BAD_MAP"

            [group.bad_nested]
            description = "x"
            type = "ListPath"
            env = "MISE_BAD_NESTED"

            [ok_has_parse_env]
            description = "x"
            type = "SetString"
            env = "MISE_OK_PARSE"
            parse_env = "set_by_comma"

            [ok_no_env]
            description = "x"
            type = "SetString"

            [ok_scalar]
            description = "x"
            type = "String"
            env = "MISE_OK_SCALAR"
        "#
        .parse()
        .expect("fixture parses");

        let mut missing = vec![];
        collect_settings_missing_parse_env(&table, "", &mut missing);
        missing.sort();

        assert_eq!(
            missing,
            vec!["bad_list", "bad_map", "bad_set", "group.bad_nested"]
        );
    }
}
