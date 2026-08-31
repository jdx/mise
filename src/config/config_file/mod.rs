use std::ffi::OsStr;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::config_file::min_version::MinVersionSpec;
use crate::config::config_file::mise_toml::{MiseToml, MonorepoConfig};
use crate::config::env_directive::EnvDirective;
use crate::config::provenance::ConfigProvenance;
use crate::config::settings::IdiomaticVersionFileSettings;
use crate::config::{AliasMap, CommandWrapper, Settings, settings};
use crate::deps::DepsConfig;
use crate::errors::Error::UntrustedConfig;
use crate::file::display_path;
use crate::hash::hash_to_str;
use crate::hooks::Hook;
use crate::redactions::Redactions;
use crate::task::{Task, TaskRustCacheConfig, TaskTemplate};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource, ToolVersionList, Toolset};
use crate::ui::prompt::Confirmation;
use crate::ui::{prompt, style};
use crate::watch_files::WatchFile;
use crate::{
    backend::{self, Backend},
    config, dirs, env, file, hash,
    registry::REGISTRY,
};
use eyre::{Result, eyre};
use globset::{GlobBuilder, GlobMatcher};
use idiomatic_version::IdiomaticVersionFile;
use indexmap::IndexMap;
use serde::Deserialize;
use std::sync::LazyLock as Lazy;
use tool_versions::ToolVersions;

use super::Config;

pub(crate) mod config_root;
pub(crate) mod diagnostic;
pub(crate) mod idiomatic_version;
pub(crate) mod min_version;
pub(crate) mod mise_toml;
pub(crate) mod toml;
pub(crate) mod tool_versions;

#[derive(Debug, PartialEq)]
pub(crate) enum ConfigFileType {
    MiseToml,
    ToolVersions,
    IdiomaticVersion(Vec<Arc<dyn Backend>>),
}

/// Classification result used to share one discovery pass between trust checks and parsing.
pub(super) enum ConfigFileDetection {
    Recognized(ConfigFileType),
    DisabledIdiomatic,
    Unknown,
    DiscoveryFailed(eyre::Report),
}

fn idiomatic_version_file_write_error(path: &Path) -> eyre::Report {
    eyre!(
        "cannot update idiomatic version file {}; use mise.toml, .tool-versions, or --path to choose a writable config file",
        display_path(path)
    )
}

fn detection_error(path: &Path, detection: ConfigFileDetection) -> eyre::Report {
    match detection {
        ConfigFileDetection::DisabledIdiomatic => idiomatic_version_file_write_error(path),
        ConfigFileDetection::Unknown => {
            eyre!("unknown config file type: {}", display_path(path))
        }
        ConfigFileDetection::DiscoveryFailed(err) => err,
        ConfigFileDetection::Recognized(_) => {
            unreachable!("recognized config detection cannot be converted to an error")
        }
    }
}

pub(crate) trait ConfigFile: Debug + Send + Sync {
    fn get_path(&self) -> &Path;
    fn provenance(&self) -> ConfigProvenance {
        ConfigProvenance::from_path(self.get_path())
    }
    fn min_version(&self) -> Option<&MinVersionSpec> {
        None
    }
    /// gets the project directory for the config
    /// if it's a global/system config, returns None
    /// files like ~/src/foo/.mise/config.toml will return ~/src/foo
    /// and ~/src/foo/.mise.config.toml will return None
    fn project_root(&self) -> Option<PathBuf> {
        let provenance = self.provenance();
        if !provenance.scope().is_project() {
            return None;
        }
        let p = provenance.path();
        match p.parent() {
            Some(dir) => match dir {
                dir if dir.starts_with(*dirs::CONFIG) => None,
                dir if dir.starts_with(*dirs::SYSTEM_CONFIG) => None,
                dir if dir == *dirs::HOME => None,
                _ => Some(config_root::config_root(p)),
            },
            None => None,
        }
    }
    fn config_root(&self) -> PathBuf {
        config_root::config_root(self.get_path())
    }
    fn plugins(&self) -> Result<HashMap<String, String>> {
        Ok(Default::default())
    }
    fn env_entries(&self) -> Result<Vec<EnvDirective>> {
        Ok(Default::default())
    }
    fn vars_entries(&self) -> Result<Vec<EnvDirective>> {
        Ok(Default::default())
    }
    fn tasks(&self) -> Vec<&Task> {
        Default::default()
    }
    fn remove_tool(&self, ba: &BackendArg) -> eyre::Result<()>;
    fn replace_versions(&self, ba: &BackendArg, versions: Vec<ToolRequest>) -> eyre::Result<()>;
    fn save(&self) -> eyre::Result<()>;
    fn dump(&self) -> eyre::Result<String>;
    fn source(&self) -> ToolSource;
    fn to_toolset(&self) -> eyre::Result<Toolset> {
        Ok(self.to_tool_request_set()?.into())
    }
    fn to_tool_request_set(&self) -> eyre::Result<ToolRequestSet>;
    fn aliases(&self) -> eyre::Result<AliasMap> {
        Ok(Default::default())
    }

    fn settings(&self) -> Option<&settings::SettingsPartial> {
        None
    }

    fn shell_aliases(&self) -> eyre::Result<IndexMap<String, String>> {
        Ok(Default::default())
    }

    fn command_wrappers(&self) -> eyre::Result<IndexMap<String, CommandWrapper>> {
        Ok(Default::default())
    }

    fn task_config(&self) -> &TaskConfig {
        static DEFAULT_TASK_CONFIG: Lazy<TaskConfig> = Lazy::new(TaskConfig::default);
        &DEFAULT_TASK_CONFIG
    }

    fn tool_config(&self) -> &ToolConfig {
        static DEFAULT_TOOL_CONFIG: Lazy<ToolConfig> = Lazy::new(ToolConfig::default);
        &DEFAULT_TOOL_CONFIG
    }

    fn task_config_includes(&self) -> eyre::Result<Option<Vec<String>>> {
        Ok(self.task_config().includes.clone())
    }

    fn task_config_excludes(&self) -> eyre::Result<Option<Vec<String>>> {
        Ok(self.task_config().excludes.clone())
    }

    fn task_templates(&self) -> IndexMap<String, TaskTemplate> {
        IndexMap::new()
    }

    fn monorepo_root(&self) -> Option<bool> {
        None
    }

    fn monorepo(&self) -> Option<&MonorepoConfig> {
        None
    }

    fn redactions(&self) -> &Redactions {
        static DEFAULT_REDACTIONS: Lazy<Redactions> = Lazy::new(Redactions::default);
        &DEFAULT_REDACTIONS
    }

    fn watch_files(&self) -> Result<Vec<WatchFile>> {
        Ok(Default::default())
    }

    fn hooks(&self) -> Result<Vec<Hook>> {
        Ok(Default::default())
    }

    fn deps_config(&self) -> Result<Option<DepsConfig>> {
        Ok(None)
    }

    fn oci_config(&self) -> Option<crate::oci::OciConfig> {
        None
    }

    fn bootstrap_config(&self) -> Option<crate::system::BootstrapTomlConfig> {
        None
    }

    fn dotfiles_config(&self) -> Option<crate::system::DotfilesTomlConfig> {
        None
    }
}

impl dyn ConfigFile {
    pub(crate) async fn add_runtimes(
        &self,
        config: &Arc<Config>,
        tools: &[ToolArg],
        pin: bool,
    ) -> eyre::Result<()> {
        // TODO: this has become a complete mess and could probably be greatly simplified
        let mut ts = self.to_toolset()?.to_owned();
        ts.resolve(config).await?;
        trace!("resolved toolset");
        let mut plugins_to_update = HashMap::new();
        for ta in tools {
            if let Some(tv) = &ta.tvr {
                plugins_to_update
                    .entry(ta.ba.clone())
                    .or_insert_with(Vec::new)
                    .push(tv);
            }
        }
        trace!("plugins to update: {plugins_to_update:?}");
        for (ba, versions) in &plugins_to_update {
            let mut tvl = ToolVersionList::new(
                ba.clone(),
                ts.source.clone().unwrap_or(ToolSource::Argument),
            );
            for tv in versions {
                tvl.requests.push((*tv).clone());
            }
            ts.versions.insert(ba.clone(), tvl);
        }
        trace!("resolving toolset 2");
        ts.resolve(config).await?;
        trace!("resolved toolset 2");
        for (ba, versions) in plugins_to_update {
            let mut new = vec![];
            for tr in versions {
                let mut tr = tr.clone();
                if pin {
                    let tv = tr.resolve(config, &Default::default()).await?;
                    if let ToolRequest::Version {
                        version: _version,
                        source,
                        options,
                        backend,
                    } = tr
                    {
                        tr = ToolRequest::Version {
                            version: tv.version,
                            source,
                            options,
                            backend,
                        };
                    }
                }
                new.push(tr);
            }
            trace!("replacing versions {new:?}");
            self.replace_versions(&ba, new)?;
        }
        trace!("done adding runtimes");

        Ok(())
    }

    /// this is for `mise local|global TOOL` which will display the version instead of setting it
    /// it's only valid to use a single tool in this case
    /// returns "true" if the tool was displayed which means the CLI should exit
    pub(crate) fn display_runtime(&self, runtimes: &[ToolArg]) -> eyre::Result<bool> {
        // in this situation we just print the current version in the config file
        if runtimes.len() == 1 && runtimes[0].tvr.is_none() {
            let fa = &runtimes[0].ba;
            let tvl = self
                .to_toolset()?
                .versions
                .get(fa)
                .ok_or_else(|| {
                    eyre!(
                        "no version set for {} in {}",
                        fa.to_string(),
                        display_path(self.get_path())
                    )
                })?
                .requests
                .iter()
                .map(|tvr| tvr.version())
                .collect::<Vec<_>>();
            miseprintln!("{}", tvl.join(" "));
            return Ok(true);
        }
        // check for something like `mise local node python@latest` which is invalid
        if runtimes.iter().any(|r| r.tvr.is_none()) {
            return Err(eyre!(
                "invalid input, specify a version for each tool. Or just specify one tool to print the current version"
            ));
        }
        Ok(false)
    }
}

async fn init(path: &Path) -> Result<Arc<dyn ConfigFile>> {
    let settings = IdiomaticVersionFileSettings::current();
    let detection = detect_config_file_with_settings(path, &settings).await;
    match detection {
        ConfigFileDetection::Recognized(ConfigFileType::MiseToml) => {
            Ok(Arc::new(MiseToml::init(path)))
        }
        ConfigFileDetection::Recognized(ConfigFileType::ToolVersions) => {
            Ok(Arc::new(ToolVersions::init(path)))
        }
        ConfigFileDetection::Recognized(ConfigFileType::IdiomaticVersion(backends)) => Ok(
            Arc::new(IdiomaticVersionFile::parse(path.to_path_buf(), backends).await?),
        ),
        detection => Err(detection_error(path, detection)),
    }
}

pub(crate) async fn parse_or_init(path: &Path) -> eyre::Result<Arc<dyn ConfigFile>> {
    let path = if path.is_dir() {
        path.join(&*env::MISE_DEFAULT_CONFIG_FILENAME)
    } else {
        path.into()
    };
    let cf = match path.exists() {
        true => parse(&path).await?,
        false => init(&path).await?,
    };
    Ok(cf)
}

/// Refuse a path mise would not read back as a TOML config.
///
/// [`parse_or_init`] gives this check to every caller that goes through a [`ConfigFile`]. Callers
/// that write TOML directly — `mise set` builds a `MiseToml` itself — bypassed it and would happily
/// create a file that config detection then refuses to recognize, or write TOML into a name mise
/// reads as `.tool-versions`. One definition of "a path mise can write TOML to", rather than two
/// that drift apart.
pub(crate) async fn ensure_writable_as_toml(path: &Path) -> eyre::Result<()> {
    let settings = IdiomaticVersionFileSettings::current();
    match detect_config_file_with_settings(path, &settings).await {
        ConfigFileDetection::Recognized(ConfigFileType::MiseToml) => Ok(()),
        ConfigFileDetection::Recognized(ConfigFileType::ToolVersions) => Err(eyre!(
            "cannot write TOML to {}: mise reads that name as a .tool-versions file",
            display_path(path)
        )),
        ConfigFileDetection::Recognized(ConfigFileType::IdiomaticVersion(_))
        | ConfigFileDetection::DisabledIdiomatic => Err(idiomatic_version_file_write_error(path)),
        detection => Err(detection_error(path, detection)),
    }
}

/// Lock a config file for a read-modify-write operation, then read its latest contents.
///
/// Callers must keep the returned lock alive until after [`ConfigFile::save`]. Acquiring the
/// lock before re-reading is what prevents two mise processes from both modifying the same stale
/// snapshot and silently overwriting one another's changes.
pub(crate) async fn lock_and_parse_or_init(
    path: &Path,
) -> eyre::Result<(fslock::LockFile, Arc<dyn ConfigFile>)> {
    lock_and_parse_or_init_with_callback(path, |path| {
        debug!("waiting for config lock on {}", display_path(path));
    })
    .await
}

async fn lock_and_parse_or_init_with_callback<F>(
    path: &Path,
    on_locked: F,
) -> eyre::Result<(fslock::LockFile, Arc<dyn ConfigFile>)>
where
    F: Fn(&Path) + 'static,
{
    // Use the same target as the atomic writer so a symlink and its real path cannot produce
    // independent lock identities for the same config file.
    let target = file::atomic_write_target(path)?;
    let lock = crate::lock_file::LockFile::new(&target)
        .with_callback(on_locked)
        .lock()?;
    let cf = parse_or_init(path).await?;
    Ok((lock, cf))
}

pub(crate) async fn parse(path: &Path) -> Result<Arc<dyn ConfigFile>> {
    let settings = IdiomaticVersionFileSettings::current();
    if let Ok(current_settings) = Settings::try_get()
        && current_settings.paranoid
    {
        trust_check(path)?;
    }
    let detection = detect_config_file_with_settings(path, &settings).await;
    parse_detected(path, detection).await
}

/// Parse a file from a previously computed detection result without repeating discovery.
/// Callers are responsible for applying the appropriate trust policy first.
pub(super) async fn parse_detected(
    path: &Path,
    detection: ConfigFileDetection,
) -> Result<Arc<dyn ConfigFile>> {
    match detection {
        ConfigFileDetection::Recognized(ConfigFileType::MiseToml) => {
            Ok(Arc::new(MiseToml::from_file(path)?))
        }
        ConfigFileDetection::Recognized(ConfigFileType::ToolVersions) => {
            Ok(Arc::new(ToolVersions::from_file(path)?))
        }
        ConfigFileDetection::Recognized(ConfigFileType::IdiomaticVersion(backends)) => Ok(
            Arc::new(IdiomaticVersionFile::parse(path.to_path_buf(), backends).await?),
        ),
        detection => Err(detection_error(path, detection)),
    }
}

/// Whether a detected config requires trust before tracked loading may parse it.
pub(super) fn detection_requires_trust(path: &Path, detection: &ConfigFileDetection) -> bool {
    if Settings::safe_mode() {
        return false;
    }
    if Settings::try_get().is_ok_and(|settings| settings.paranoid) {
        return true;
    }
    match detection {
        ConfigFileDetection::Recognized(ConfigFileType::MiseToml) => {
            !MiseToml::path_is_trust_exempt(path)
        }
        ConfigFileDetection::Recognized(ConfigFileType::ToolVersions) => {
            ToolVersions::path_requires_trust(path)
        }
        ConfigFileDetection::Recognized(ConfigFileType::IdiomaticVersion(_))
        | ConfigFileDetection::DisabledIdiomatic
        | ConfigFileDetection::Unknown
        | ConfigFileDetection::DiscoveryFailed(_) => false,
    }
}

pub(crate) fn config_trust_root(path: &Path) -> PathBuf {
    if settings::is_loaded() && Settings::get().paranoid {
        path.to_path_buf()
    } else {
        config_root::config_root(path)
    }
}

/// Whether the file or its trust root has been trusted.
///
/// Unlike a passing [`trust_check`], this is false for files that merely do
/// not *need* trust (e.g. safe configs loaded without it).
pub(crate) fn is_path_trusted(path: &Path) -> bool {
    is_trusted(&config_trust_root(path)) || is_trusted(path)
}

static IMPLICITLY_TRUST_ACTIVE_CONFIG: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_implicitly_trust_active_config(enabled: bool) {
    IMPLICITLY_TRUST_ACTIVE_CONFIG.store(enabled, Ordering::Relaxed);
}

pub(crate) fn trust_active_config() -> Result<()> {
    if !IMPLICITLY_TRUST_ACTIVE_CONFIG.load(Ordering::Relaxed) {
        return Ok(());
    }
    let Ok(settings) = Settings::try_get() else {
        return Ok(());
    };
    if settings.paranoid || Settings::safe_mode() {
        return Ok(());
    }
    for path in config::load_config_paths(&config::DEFAULT_CONFIG_FILENAMES, false) {
        if config::is_global_config(&path) {
            continue;
        }
        let config_root = config_trust_root(&path);
        if is_ignored(&config_root) || is_ignored(&path) {
            continue;
        }
        if !is_trusted(&config_root) {
            trust(&config_root)?;
        }
    }
    Ok(())
}

pub(crate) fn trust_check(path: &Path) -> eyre::Result<()> {
    // In safe mode, config is inert (no code execution, no env injection — see
    // MISE_SAFE / the `safe` setting), so loading an untrusted config is
    // harmless and no trust is required. `safe` is global-only, so a project
    // config cannot disable it for itself.
    if Settings::safe_mode() {
        return Ok(());
    }
    // Commands that execute project-defined behavior are an explicit signal
    // to trust their active config in normal mode. Persist the decision here
    // so unsafe config can load before the command starts; safe config is
    // persisted by `trust_active_config` after settings initialization.
    if IMPLICITLY_TRUST_ACTIVE_CONFIG.load(Ordering::Relaxed)
        && !ci_info::is_ci()
        && Settings::try_get().is_ok_and(|settings| !settings.paranoid)
    {
        let config_root = config_trust_root(path);
        if is_path_trusted(path) || (!is_ignored(&config_root) && !is_ignored(path)) {
            trust(&config_root)?;
            return Ok(());
        }
    }
    static MUTEX: Mutex<()> = Mutex::new(());
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple checks at once so we don't prompt multiple times for the same path
    let config_root = config_trust_root(path);
    let default_cmd = String::new();
    let args = env::ARGS.read().unwrap();
    let cmd = args.get(1).unwrap_or(&default_cmd).as_str();
    if is_path_trusted(path) || cmd == "trust" || cfg!(test) {
        return Ok(());
    }
    if cmd != "hook-env" && !is_ignored(&config_root) && !is_ignored(path) {
        let ans = if settings::is_loaded() && Settings::get().yes {
            Confirmation::Yes
        } else {
            prompt::confirm_with_all(format!(
                "{} config files in {} are not trusted. Trust them?",
                style::eyellow("mise"),
                style::epath(&config_root)
            ))?
        };
        match ans {
            Confirmation::Yes => {
                trust(&config_root)?;
                return Ok(());
            }
            // Only a real decline is worth remembering. An ignore marker is
            // sticky and silent, so recording one for an answer nobody gave
            // would stop the config from applying with nothing to explain why.
            Confirmation::No => add_ignored(config_root.to_path_buf())?,
            Confirmation::Unavailable => {}
        }
    }
    Err(UntrustedConfig(path.into()))?
}

pub(crate) fn is_trusted(path: &Path) -> bool {
    let canonicalized_path = match path.canonicalize() {
        Ok(p) => p,
        Err(err) => {
            debug!("trust canonicalize: {err}");
            return false;
        }
    };
    // The `ignored_config_paths` setting is an explicit "never load this
    // config" instruction and is a hard block that takes precedence over
    // everything, including `trusted_config_paths`.
    if is_ignored_via_setting(canonicalized_path.as_path()) {
        return false;
    }
    if IS_TRUSTED
        .lock()
        .unwrap()
        .contains(canonicalized_path.as_path())
    {
        return true;
    }
    // `trusted_config_paths` is trusted by configuration and overrides the
    // persisted ignore list (a dismissed trust prompt or `mise trust
    // --ignore`), matching the "still trusted via settings" warnings emitted by
    // `mise trust`. It is therefore checked *before* the persisted ignore list.
    if trusted_config_path_matches(canonicalized_path.as_path()) {
        add_trusted(canonicalized_path.to_path_buf());
        return true;
    }
    // The persisted ignore list blocks trust only when the path is not trusted
    // via `trusted_config_paths` above.
    if is_persisted_ignored(canonicalized_path.as_path()) {
        return false;
    }
    if config::is_global_config(path) {
        add_trusted(canonicalized_path.to_path_buf());
        return true;
    }

    // Check if this path is within a trusted monorepo root
    // Monorepo roots are marked with a special marker file when trusted
    if let Some(parent) = canonicalized_path.parent() {
        let mut current = parent;
        while let Some(dir) = current.parent() {
            let monorepo_marker = with_appended_extension(&trust_path(dir), "monorepo");
            if monorepo_marker.exists() {
                add_trusted(canonicalized_path.to_path_buf());
                return true;
            }
            current = dir;
        }
    }
    let settings = Settings::get();
    if settings.paranoid {
        let trusted = trust_file_hash(path).unwrap_or_else(|e| {
            warn!("trust_file_hash: {e}");
            false
        });
        if !trusted {
            return false;
        }
    } else if cfg!(test) || ci_info::is_ci() {
        // in tests/CI we trust everything
        return true;
    } else if !trust_path(path).exists() {
        // No direct trust record. A config inside a linked git worktree
        // shares trust with the equivalent path in the repo's main checkout.
        // Not applicable in paranoid mode (excluded above), where trust is
        // tied to file contents that can differ between worktree branches.
        if let Some(main_path) = crate::git::main_checkout_equivalent(&canonicalized_path)
            && is_trusted(&main_path)
        {
            add_trusted(canonicalized_path.to_path_buf());
            return true;
        }
        return false;
    }
    add_trusted(canonicalized_path.to_path_buf());
    true
}

static IS_TRUSTED: Lazy<Mutex<HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static IS_IGNORED: Lazy<Mutex<HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(HashSet::new()));

fn add_trusted(path: PathBuf) {
    IS_TRUSTED.lock().unwrap().insert(path);
}
pub(crate) fn add_ignored(path: PathBuf) -> Result<()> {
    let path = path.canonicalize()?;
    file::create_dir_all(&*dirs::IGNORED_CONFIGS)?;
    file::make_symlink_or_file(&path, &ignore_path(&path))?;
    IS_IGNORED.lock().unwrap().insert(path);
    Ok(())
}
pub(crate) fn rm_ignored(path: PathBuf) -> Result<()> {
    let path = path.canonicalize()?;
    let ignore_path = ignore_path(&path);
    if ignore_path.exists() {
        file::remove_file(&ignore_path)?;
    }
    IS_IGNORED.lock().unwrap().remove(&path);
    Ok(())
}
/// Whether an already-canonicalized path lives under a `trusted_config_paths`
/// entry. Callers with a raw path use [`is_trusted_via_config_paths`], which
/// canonicalizes first.
fn trusted_config_path_matches(canonicalized_path: &Path) -> bool {
    Settings::get()
        .trusted_config_paths()
        .any(|p| canonicalized_path.starts_with(p))
}

/// Whether `path` is trusted purely via the `trusted_config_paths` setting.
///
/// This is the signal that overrides the persisted ignore list (a dismissed
/// trust prompt or `mise trust --ignore`) in both [`is_trusted`] and config
/// discovery. It does not consider global config or per-file trust records.
pub(crate) fn is_trusted_via_config_paths(path: &Path) -> bool {
    // Config discovery calls this, and the initial `Settings` load itself runs
    // config discovery (`load_config_paths`). Reading `trusted_config_paths`
    // before settings are loaded would re-enter `Settings::get()` and recurse,
    // so treat the path as not-yet-trusted until settings exist; discovery runs
    // again once they do.
    if !settings::is_loaded() {
        return false;
    }
    match path.canonicalize() {
        Ok(canonicalized_path) => trusted_config_path_matches(&canonicalized_path),
        Err(_) => false,
    }
}

/// Whether `path` sits under one of `ignored`.
///
/// Compared twice: as written, then with both sides canonicalized.
///
/// `Path::starts_with` is component-wise, so the as-written pass already handles
/// separator and case conventions for ordinary input — it is what the
/// directory-level checks in `config` do. The canonical pass is what lets a
/// plainly-written setting value match a candidate that arrives canonicalized:
/// on Windows `canonicalize` yields a `\\?\` verbatim path that no plain prefix
/// matches, and on unix it resolves symlinks. `Settings::trusted_config_paths`
/// canonicalizes its own entries for the same reason.
fn path_is_under_any(path: &Path, ignored: &[PathBuf]) -> bool {
    if ignored.iter().any(|p| path.starts_with(p)) {
        return true;
    }
    let Some(canonical) = file::canonicalize_cached(path) else {
        // Nothing on disk to resolve — the as-written pass above was the only
        // chance to match.
        return false;
    };
    ignored
        .iter()
        .filter_map(|p| file::canonicalize_cached(p))
        .any(|p| canonical.starts_with(p))
}

#[derive(Default)]
struct IgnoredConfigPathMatcher {
    literals: Vec<PathBuf>,
    globs: Vec<GlobMatcher>,
}

impl IgnoredConfigPathMatcher {
    fn new(paths: &[PathBuf]) -> Self {
        let mut matcher = Self::default();
        for path in paths {
            let pattern = path.to_string_lossy();
            if !pattern.contains(['*', '?', '[', '{']) {
                matcher.literals.push(path.clone());
                continue;
            }
            match GlobBuilder::new(&pattern).literal_separator(true).build() {
                Ok(glob) => matcher.globs.push(glob.compile_matcher()),
                Err(err) => {
                    warn!("invalid ignored_config_paths glob {pattern}: {err}");
                    matcher.literals.push(path.clone());
                }
            }
        }
        matcher
    }

    fn is_match(&self, path: &Path) -> bool {
        path_is_under_any(path, &self.literals)
            || self.globs.iter().any(|glob| glob.is_match(path))
            || file::canonicalize_cached(path)
                .is_some_and(|path| self.globs.iter().any(|glob| glob.is_match(&path)))
    }
}

static IGNORED_CONFIG_PATH_MATCHER: Lazy<IgnoredConfigPathMatcher> =
    Lazy::new(|| IgnoredConfigPathMatcher::new(&env::MISE_IGNORED_CONFIG_PATHS));

/// Whether `path` is under an explicitly-configured `ignored_config_paths`
/// (`MISE_IGNORED_CONFIG_PATHS`) entry.
///
/// This is an explicit "never load this config" instruction and is a hard
/// block: it takes precedence over `trusted_config_paths`.
pub(crate) fn is_ignored_via_setting(path: &Path) -> bool {
    IGNORED_CONFIG_PATH_MATCHER.is_match(path)
}

/// The config path an ignore-list entry records.
///
/// Resolve the entry; do not canonicalize it. mise writes these as symlinks on unix and, since
/// Windows symlinks need a privilege mise does not require, as plain files holding the path there
/// (`file::make_symlink_or_file`). `Path::canonicalize` follows a symlink, so unix happened to come
/// out right — on Windows it returned the entry's own path inside `ignored-configs`, which is never
/// a config path, so the loaded set matched nothing and `mise trust --ignore` had no effect beyond
/// the process that ran it.
///
/// No second `canonicalize` on the result: [`add_ignored`] canonicalizes before writing, so the
/// recorded form already matches what the caller below compares against.
fn ignored_entry_path(entry: &Path) -> Option<PathBuf> {
    file::resolve_symlink(entry).ok().flatten()
}

/// Whether `path` is in the persisted ignore list.
///
/// Entries are recorded when the user answers "No" to a trust prompt or runs
/// `mise trust --ignore`. Unlike [`is_ignored_via_setting`], this only records
/// a dismissed prompt, so it is overridden by `trusted_config_paths` (see
/// [`is_trusted_via_config_paths`]).
pub(crate) fn is_persisted_ignored(path: &Path) -> bool {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if !dirs::IGNORED_CONFIGS.exists() {
            return;
        }
        let mut is_ignored = IS_IGNORED.lock().unwrap();
        for entry in file::ls(&dirs::IGNORED_CONFIGS).unwrap_or_default() {
            if let Some(path) = ignored_entry_path(&entry) {
                is_ignored.insert(path);
            }
        }
    });
    match path.canonicalize() {
        Ok(path) => IS_IGNORED.lock().unwrap().contains(&path),
        Err(_) => {
            debug!("is_persisted_ignored: path canonicalize failed");
            true
        }
    }
}

/// Whether `path` is ignored, by either the `ignored_config_paths` setting or
/// the persisted ignore list. Callers that need to respect the
/// `trusted_config_paths` override use the finer-grained variants directly.
pub(crate) fn is_ignored(path: &Path) -> bool {
    is_ignored_via_setting(path) || is_persisted_ignored(path)
}

pub(crate) fn trust(path: &Path) -> Result<()> {
    rm_ignored(path.to_path_buf())?;
    let hashed_path = trust_path(path);
    if !hashed_path.exists() {
        file::create_dir_all(hashed_path.parent().unwrap())?;
        file::make_symlink_or_file(path.canonicalize()?.as_path(), &hashed_path)?;
    }
    if Settings::get().paranoid {
        let trust_hash_path = with_appended_extension(&hashed_path, "hash");
        let hash = hash::file_hash_sha256(path, None)?;
        file::write(trust_hash_path, hash)?;
    }
    Ok(())
}

/// Marks a trusted config as a monorepo root, allowing all descendant configs to be trusted
pub(crate) fn mark_as_monorepo_root(path: &Path) -> Result<()> {
    let config_root = config_trust_root(path);
    let hashed_path = trust_path(&config_root);
    let monorepo_marker = with_appended_extension(&hashed_path, "monorepo");
    if !monorepo_marker.exists() {
        file::create_dir_all(monorepo_marker.parent().unwrap())?;
        file::write(&monorepo_marker, "")?;
    }
    Ok(())
}

pub(crate) fn untrust(path: &Path) -> eyre::Result<()> {
    rm_ignored(path.to_path_buf())?;
    let hashed_path = trust_path(path);
    if hashed_path.exists() {
        file::remove_file(&hashed_path)?;
    }
    let hash_path = with_appended_extension(&hashed_path, "hash");
    if hash_path.exists() {
        file::remove_file(&hash_path)?;
    }
    let monorepo_path = with_appended_extension(&hashed_path, "monorepo");
    if monorepo_path.exists() {
        file::remove_file(&monorepo_path)?;
    }
    Ok(())
}

/// generates a path like ~/.mise/trusted-configs/dir-file-3e8b8c44c3.toml
fn trust_path(path: &Path) -> PathBuf {
    dirs::TRUSTED_CONFIGS.join(hashed_path_filename(path))
}

fn ignore_path(path: &Path) -> PathBuf {
    dirs::IGNORED_CONFIGS.join(hashed_path_filename(path))
}

/// Appends an extension to a path without replacing existing dots in the filename.
/// Unlike `Path::with_extension`, this preserves the full filename.
/// e.g. "foo-bar.toml-abc123" + "hash" → "foo-bar.toml-abc123.hash"
///
/// NOTE: This changes the filename convention for .hash and .monorepo files.
/// Existing files from prior versions will not be found, requiring a one-time
/// re-trust of previously trusted configs after upgrade.
pub(crate) fn with_appended_extension(path: &Path, ext: &str) -> PathBuf {
    let mut os_string = path.as_os_str().to_owned();
    os_string.push(".");
    os_string.push(ext);
    PathBuf::from(os_string)
}

/// creates the filename portion of trust/ignore files, e.g.:
fn hashed_path_filename(path: &Path) -> String {
    let canonicalized_path = path.canonicalize().unwrap();
    let hash = hash_to_str(&canonicalized_path);
    let trunc_str = |s: &OsStr| {
        let mut s = s.to_str().unwrap().to_string();
        s = s.chars().take(20).collect();
        s
    };
    let trust_path = dirs::TRUSTED_CONFIGS.join(hash_to_str(&hash));
    if trust_path.exists() {
        return trust_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
    }
    let parent = canonicalized_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default()
        .file_name()
        .map(trunc_str);
    let filename = canonicalized_path.file_name().map(trunc_str);
    [parent, filename, Some(hash)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("-")
}

fn trust_file_hash(path: &Path) -> eyre::Result<bool> {
    let trust_path = trust_path(path);
    let trust_hash_path = with_appended_extension(&trust_path, "hash");
    if !trust_hash_path.exists() {
        return Ok(false);
    }
    let hash = file::read_to_string(&trust_hash_path)?;
    let actual = hash::file_hash_sha256(path, None)?;
    Ok(hash == actual)
}

pub(crate) fn matching_idiomatic_filenames<'a>(
    path: &Path,
    filenames: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    let matches = filenames
        .into_iter()
        .filter(|filename| path.ends_with(filename))
        .collect::<Vec<_>>();
    let max_components = matches
        .iter()
        .map(|filename| Path::new(filename).components().count())
        .max()
        .unwrap_or_default();
    matches
        .into_iter()
        .filter(|filename| Path::new(filename).components().count() == max_components)
        .collect()
}

fn path_matches_registry_idiomatic(path: &Path) -> bool {
    let filenames = REGISTRY
        .values()
        .flat_map(|rt| rt.idiomatic_files.iter().map(|file| file.path));
    !matching_idiomatic_filenames(path, filenames).is_empty()
}

fn registry_tool_matches_idiomatic_path(tool: &str, path: &Path) -> bool {
    REGISTRY.get(tool).is_some_and(|registry_tool| {
        let filenames = registry_tool.idiomatic_files.iter().map(|file| file.path);
        !matching_idiomatic_filenames(path, filenames).is_empty()
    })
}

/// Registry fallback for paths whose matching backends are not enabled and therefore not queried.
fn path_is_disabled_registry_idiomatic(
    path: &Path,
    settings: &IdiomaticVersionFileSettings,
) -> bool {
    let active_filenames = REGISTRY
        .values()
        .filter(|rt| settings.enable_tools.contains(rt.short))
        .flat_map(|rt| {
            rt.idiomatic_files
                .iter()
                .filter(|file| {
                    !super::idiomatic_version_file_disabled(
                        &settings.disable_files,
                        rt.short,
                        file.path,
                    )
                })
                .map(|file| file.path)
        })
        .collect::<Vec<_>>();
    path_matches_registry_idiomatic(path)
        && matching_idiomatic_filenames(path, active_filenames).is_empty()
}

enum IdiomaticBackendDetection {
    Recognized(Vec<Arc<dyn Backend>>),
    Disabled,
    NoMatch,
}

async fn detect_idiomatic_backends(
    path: &Path,
    settings: &IdiomaticVersionFileSettings,
) -> Result<IdiomaticBackendDetection> {
    // Idiomatic version files are opt-in per tool. Skipping non-enabled backends is
    // also what keeps `idiomatic_filenames()` from booting a Lua VM for every
    // installed vfox plugin on every invocation just to classify a config path.
    if settings.enable_tools.is_empty() {
        return Ok(IdiomaticBackendDetection::NoMatch);
    }
    let mut first_error = None;
    let mut unseen_tools = settings.enable_tools.clone();
    let mut disabled_filenames = BTreeSet::new();
    let mut backends_by_filename = BTreeMap::<String, Vec<Arc<dyn Backend>>>::new();
    for b in backend::list() {
        if !settings.enable_tools.contains(b.id()) {
            continue;
        }
        unseen_tools.remove(b.id());
        match b.idiomatic_filenames().await {
            Ok(filenames) => {
                for filename in filenames {
                    if super::idiomatic_version_file_disabled(
                        &settings.disable_files,
                        b.id(),
                        &filename,
                    ) {
                        disabled_filenames.insert(filename);
                        continue;
                    }
                    backends_by_filename
                        .entry(filename)
                        .or_default()
                        .push(b.clone());
                }
            }
            Err(err) => {
                debug!("idiomatic_filenames failed for {}: {:?}", b, err);
                if first_error.is_none() {
                    first_error = Some(err.wrap_err(format!(
                        "failed to discover idiomatic filenames for {}",
                        b.id()
                    )));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    let backends =
        matching_idiomatic_filenames(path, backends_by_filename.keys().map(String::as_str))
            .into_iter()
            .flat_map(|filename| backends_by_filename.get(filename).into_iter().flatten())
            .filter(|backend| seen.insert(backend.id().to_string()))
            .cloned()
            .collect::<Vec<_>>();
    if !backends.is_empty() {
        return Ok(IdiomaticBackendDetection::Recognized(backends));
    }
    if let Some(err) = first_error {
        return Err(err);
    }
    if let Some(tool) = unseen_tools
        .into_iter()
        .find(|tool| registry_tool_matches_idiomatic_path(tool, path))
    {
        return Err(eyre!(
            "enabled idiomatic backend {tool} is not available for discovery"
        ));
    }
    if !matching_idiomatic_filenames(path, disabled_filenames.iter().map(String::as_str)).is_empty()
    {
        return Ok(IdiomaticBackendDetection::Disabled);
    }
    Ok(IdiomaticBackendDetection::NoMatch)
}

/// Detect a config type that is determined solely by its filename.
pub(super) fn detect_config_file_type_by_filename(path: &Path) -> Option<ConfigFileType> {
    let filename = path.file_name().and_then(|f| f.to_str())?;
    if env::MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES
        .as_ref()
        .is_some_and(|filenames| filenames.contains(filename))
        || env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str() == filename
    {
        return Some(ConfigFileType::ToolVersions);
    }
    if env::MISE_OVERRIDE_CONFIG_FILENAMES.contains(filename)
        || env::MISE_DEFAULT_CONFIG_FILENAME.as_str() == filename
    {
        return Some(ConfigFileType::MiseToml);
    }
    None
}

/// Detect a config file while preserving disabled and backend-discovery outcomes.
pub(super) async fn detect_config_file_with_settings(
    path: &Path,
    settings: &IdiomaticVersionFileSettings,
) -> ConfigFileDetection {
    if let Some(config_type) = detect_config_file_type_by_filename(path) {
        return ConfigFileDetection::Recognized(config_type);
    }
    let filename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("mise.toml");

    let registry_idiomatic = path_matches_registry_idiomatic(path);
    match detect_idiomatic_backends(path, settings).await {
        Ok(IdiomaticBackendDetection::Recognized(backends)) => {
            ConfigFileDetection::Recognized(ConfigFileType::IdiomaticVersion(backends))
        }
        Ok(IdiomaticBackendDetection::Disabled) => ConfigFileDetection::DisabledIdiomatic,
        Ok(IdiomaticBackendDetection::NoMatch)
            if path_is_disabled_registry_idiomatic(path, settings) =>
        {
            ConfigFileDetection::DisabledIdiomatic
        }
        Ok(IdiomaticBackendDetection::NoMatch) if registry_idiomatic => {
            ConfigFileDetection::Unknown
        }
        Ok(IdiomaticBackendDetection::NoMatch) if filename.ends_with(".toml") => {
            ConfigFileDetection::Recognized(ConfigFileType::MiseToml)
        }
        Ok(IdiomaticBackendDetection::NoMatch) => ConfigFileDetection::Unknown,
        // An unrelated enabled backend failure must not prevent ordinary TOML parsing.
        Err(_) if filename.ends_with(".toml") && !registry_idiomatic => {
            ConfigFileDetection::Recognized(ConfigFileType::MiseToml)
        }
        Err(err) => ConfigFileDetection::DiscoveryFailed(err.wrap_err(format!(
            "failed to classify config file {}",
            display_path(path)
        ))),
    }
}

impl Display for dyn ConfigFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let toolset = self.to_toolset().unwrap().to_string();
        write!(f, "{}: {toolset}", display_path(self.get_path()))
    }
}

impl PartialEq for dyn ConfigFile {
    fn eq(&self, other: &Self) -> bool {
        self.get_path() == other.get_path()
    }
}

impl Eq for dyn ConfigFile {}

impl Hash for dyn ConfigFile {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.get_path().hash(state);
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TaskConfig {
    pub cascade: Option<bool>,
    pub includes: Option<Vec<String>>,
    pub excludes: Option<Vec<String>>,
    pub dir: Option<String>,
    pub shell: Option<String>,
    pub cache: Option<crate::task::TaskCacheConfig>,
    pub rust_cache: Option<TaskRustCacheConfig>,
    pub global_env: Vec<String>,
    pub global_pass_through_env: Vec<String>,
    pub global_inputs: Vec<String>,
    pub input_groups: IndexMap<String, Vec<String>>,
}

/// Policy applied to tools declared by configs sharing this config's root.
///
/// Unlike `[settings]`, this is intentionally config-root-owned rather than
/// an invocation-wide merged value.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ToolConfig {
    pub locked: bool,
}

/// Deliberately not `#[cfg(unix)]` like the module below: the bug these cover is
/// one that only shows up once `canonicalize` rewrites the path, which is the
/// normal case on Windows.
#[cfg(test)]
mod ignored_config_path_tests {
    use super::*;

    /// The setting is written as an ordinary path while the candidate can arrive
    /// canonicalized — `is_trusted` passes one straight in. On Windows that is a
    /// `\\?\` verbatim path; on macOS a tempdir under `/var` resolves to
    /// `/private/var`. Neither matches a plainly-written prefix, so the two sides
    /// have to be resolved together.
    #[test]
    fn plain_entry_matches_canonicalized_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let ignored_dir = tmp.path().join("cfgdir");
        std::fs::create_dir_all(&ignored_dir).unwrap();
        let cfg = ignored_dir.join("config.toml");
        std::fs::write(&cfg, "").unwrap();

        let ignored = vec![ignored_dir];
        assert!(path_is_under_any(&cfg, &ignored));
        assert!(path_is_under_any(&cfg.canonicalize().unwrap(), &ignored));
    }

    #[test]
    fn sibling_directory_is_not_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let ignored_dir = tmp.path().join("cfgdir");
        let other_dir = tmp.path().join("other");
        std::fs::create_dir_all(&ignored_dir).unwrap();
        std::fs::create_dir_all(&other_dir).unwrap();
        let cfg = other_dir.join("config.toml");
        std::fs::write(&cfg, "").unwrap();

        let ignored = vec![ignored_dir];
        assert!(!path_is_under_any(&cfg, &ignored));
        assert!(!path_is_under_any(&cfg.canonicalize().unwrap(), &ignored));
    }

    /// The same defect without leaving unix: an entry that reaches its directory
    /// through a symlink never matches a candidate expressed by its real path.
    ///
    /// This is the case that fails on an ordinary Linux runner. The test above
    /// only bites where the platform rewrites the path on its own — a `\\?\`
    /// prefix on Windows, `/var` → `/private/var` on macOS — so on Linux it
    /// passes with or without the fix.
    #[cfg(unix)]
    #[test]
    fn symlinked_entry_matches_real_candidate() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("target");
        let ignored_link = tmp.path().join("ignored-link");
        std::fs::create_dir_all(&target_dir).unwrap();
        symlink(&target_dir, &ignored_link).unwrap();

        let cfg = target_dir.join("config.toml");
        std::fs::write(&cfg, "").unwrap();

        let ignored = vec![ignored_link];
        // The as-written pass cannot see through the link...
        assert!(!cfg.starts_with(&ignored[0]));
        // ...so only resolving both sides finds the match.
        assert!(path_is_under_any(&cfg, &ignored));
    }

    /// A path that does not exist cannot be canonicalized, so the as-written
    /// comparison is the only one that can match it.
    #[test]
    fn missing_path_still_matches_as_written() {
        let tmp = tempfile::tempdir().unwrap();
        let ignored_dir = tmp.path().join("cfgdir");

        let ignored = vec![ignored_dir.clone()];
        assert!(path_is_under_any(
            &ignored_dir.join("config.toml"),
            &ignored
        ));
        assert!(!path_is_under_any(
            &tmp.path().join("other").join("config.toml"),
            &ignored
        ));
    }

    #[test]
    fn recursive_glob_matches_config_path_but_not_sibling_name() {
        let root = tempfile::tempdir().unwrap();
        let pattern = root.path().join("vendor").join("**").join("mise.toml");
        let matcher = IgnoredConfigPathMatcher::new(&[pattern]);

        assert!(matcher.is_match(&root.path().join("vendor/jj/.config/mise.toml")));
        assert!(!matcher.is_match(&root.path().join("vendor/jj/.config/.mise.toml")));
    }

    #[test]
    fn literal_entries_keep_directory_prefix_behavior() {
        let root = tempfile::tempdir().unwrap();
        let ignored = root.path().join("vendor");
        let matcher = IgnoredConfigPathMatcher::new(&[ignored]);

        assert!(matcher.is_match(&root.path().join("vendor/jj/mise.toml")));
        assert!(!matcher.is_match(&root.path().join("vendor-other/mise.toml")));
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::collections::BTreeSet;

    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn test_detect_config_file_type() {
        env::set_var("MISE_EXPERIMENTAL", "true");
        backend::load_tools().await.unwrap();
        let settings = IdiomaticVersionFileSettings::default();
        // Idiomatic version files are opt-in; with the default (empty)
        // `idiomatic_version_file_enable_tools` they are classified as disabled.
        for path in [
            "/foo/bar/.nvmrc",
            "/foo/bar/package.json",
            "/foo/bar/rust-toolchain.toml",
        ] {
            assert!(matches!(
                detect_config_file_with_settings(Path::new(path), &settings).await,
                ConfigFileDetection::DisabledIdiomatic
            ));
        }
        assert!(matches!(
            detect_config_file_with_settings(Path::new("/foo/bar/.test-tool-versions"), &settings)
                .await,
            ConfigFileDetection::Recognized(ConfigFileType::ToolVersions)
        ));
        assert!(matches!(
            detect_config_file_with_settings(Path::new("/foo/bar/mise.toml"), &settings).await,
            ConfigFileDetection::Recognized(ConfigFileType::MiseToml)
        ));
    }

    #[test]
    fn test_unavailable_backend_only_matters_for_matching_registry_path() {
        assert!(registry_tool_matches_idiomatic_path(
            "node",
            Path::new("/foo/package.json")
        ));
        assert!(!registry_tool_matches_idiomatic_path(
            "node",
            Path::new("/foo/.ruby-version")
        ));
        assert!(!registry_tool_matches_idiomatic_path(
            "missing",
            Path::new("/foo/package.json")
        ));
    }

    #[test]
    fn test_registry_idiomatic_file_is_disabled_only_when_all_matches_are_disabled() {
        let path = Path::new("/foo/package.json");
        let mut settings = IdiomaticVersionFileSettings {
            enable_tools: BTreeSet::from(["node".to_string(), "yarn".to_string()]),
            disable_files: BTreeSet::from(["yarn:package.json".to_string()]),
        };
        assert!(!path_is_disabled_registry_idiomatic(path, &settings));

        settings
            .disable_files
            .insert("node:package.json".to_string());
        assert!(path_is_disabled_registry_idiomatic(path, &settings));
    }

    #[test]
    fn test_disabled_nested_registry_idiomatic_falls_back_to_active_shorter_match() {
        let path = Path::new("/foo/.config/goreleaser.yaml");
        let mut settings = IdiomaticVersionFileSettings {
            enable_tools: BTreeSet::from(["goreleaser".to_string()]),
            disable_files: BTreeSet::from(["goreleaser:.config/goreleaser.yaml".to_string()]),
        };
        assert!(!path_is_disabled_registry_idiomatic(path, &settings));

        settings
            .disable_files
            .insert("goreleaser:goreleaser.yaml".to_string());
        assert!(path_is_disabled_registry_idiomatic(path, &settings));
    }

    #[tokio::test]
    async fn test_parse_or_init_rejects_disabled_idiomatic_file() {
        backend::load_tools().await.unwrap();
        let err = parse_or_init(Path::new("package.json"))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cannot update idiomatic version file"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_detect_idiomatic_backends_for_enabled_tools() -> Result<()> {
        backend::load_tools().await?;
        let disable_files = BTreeSet::new();
        for (enabled, path) in [
            ("node", "/foo/bar/.nvmrc"),
            ("ruby", "/foo/bar/.ruby-version"),
            ("rust", "/foo/bar/rust-toolchain.toml"),
            ("goreleaser", "/foo/bar/.config/goreleaser.yaml"),
        ] {
            let settings = IdiomaticVersionFileSettings {
                enable_tools: BTreeSet::from([enabled.to_string()]),
                disable_files: disable_files.clone(),
            };
            let backends = match detect_idiomatic_backends(Path::new(path), &settings).await? {
                IdiomaticBackendDetection::Recognized(backends) => backends,
                _ => panic!("{path} should be idiomatic for {enabled}"),
            };
            assert!(backends.iter().any(|b| b.id() == enabled));
            // A file for a non-enabled tool must not match.
            let settings = IdiomaticVersionFileSettings {
                enable_tools: BTreeSet::from(["zig".to_string()]),
                disable_files: disable_files.clone(),
            };
            assert!(matches!(
                detect_idiomatic_backends(Path::new(path), &settings).await?,
                IdiomaticBackendDetection::NoMatch
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_parse_nested_registry_idiomatic_file() -> Result<()> {
        env::set_var("MISE_EXPERIMENTAL", "true");
        backend::load_tools().await?;
        let dir = tempfile::tempdir()?;
        let config_dir = dir.path().join(".config");
        std::fs::create_dir_all(&config_dir)?;
        let path = config_dir.join("goreleaser.yaml");
        file::write(&path, "version: 2\n")?;

        let settings = IdiomaticVersionFileSettings {
            enable_tools: BTreeSet::from(["goreleaser".to_string()]),
            disable_files: BTreeSet::new(),
        };
        let backends = match detect_idiomatic_backends(&path, &settings).await? {
            IdiomaticBackendDetection::Recognized(backends) => backends,
            _ => panic!("goreleaser should be matched from its nested idiomatic path"),
        };
        let tools = IdiomaticVersionFile::parse(path.clone(), backends)
            .await?
            .to_tool_request_set()?
            .into_iter()
            .collect::<Vec<_>>();
        let (_, versions, _) = tools
            .iter()
            .find(|(backend, _, _)| backend.short == "goreleaser")
            .expect("goreleaser should be parsed from its nested idiomatic path");

        assert_eq!(versions[0].version(), "2");
        Ok(())
    }

    #[test]
    fn lock_and_parse_or_init_reads_after_acquiring_lock() -> Result<()> {
        use std::os::unix::fs::symlink;
        use std::sync::mpsc;
        use std::time::Duration;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(backend::load_tools())?;

        let dir = tempfile::tempdir()?;
        let path = dir.path().join("mise.toml");
        file::write(&path, "[tools]\ndummy = \"1\"\n")?;
        let alias = dir.path().join("linked.toml");
        symlink(&path, &alias)?;

        let target = file::atomic_write_target(&path)?;
        let lock = crate::lock_file::LockFile::new(&target).lock()?;
        let (waiting_tx, waiting_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || -> Result<String> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let (_lock, cf) = runtime
                .block_on(lock_and_parse_or_init_with_callback(&alias, move |_| {
                    waiting_tx.send(()).unwrap()
                }))?;
            acquired_tx.send(()).unwrap();
            cf.dump()
        });

        // The callback proves that the symlink spelling reached the contended lock for the real
        // path. Change the file while the waiter is blocked; it must parse this version only after
        // acquiring the lock.
        waiting_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            acquired_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        file::write_atomic(&path, "[tools]\ndummy = \"1\"\ntiny = \"2\"\n")?;
        drop(lock);

        acquired_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let contents = reader.join().unwrap()?;
        assert_eq!(contents, "[tools]\ndummy = \"1\"\ntiny = \"2\"\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_detect_idiomatic_backends_reports_disabled_match() -> Result<()> {
        backend::load_tools().await?;
        let settings = IdiomaticVersionFileSettings {
            enable_tools: BTreeSet::from(["node".to_string()]),
            disable_files: BTreeSet::from(["node:package.json".to_string()]),
        };

        assert!(matches!(
            detect_idiomatic_backends(Path::new("package.json"), &settings).await?,
            IdiomaticBackendDetection::Disabled
        ));
        Ok(())
    }

    #[tokio::test]
    async fn test_path_is_idiomatic_respects_disabled_files() -> Result<()> {
        backend::load_tools().await?;
        let settings = IdiomaticVersionFileSettings {
            enable_tools: BTreeSet::from(["node".to_string(), "pnpm".to_string()]),
            disable_files: BTreeSet::from(["node:package.json".to_string()]),
        };

        let backends = match detect_idiomatic_backends(Path::new("package.json"), &settings).await?
        {
            IdiomaticBackendDetection::Recognized(backends) => backends,
            _ => panic!("package.json should remain idiomatic for package managers"),
        };

        assert!(!backends.iter().any(|backend| backend.id() == "node"));
        assert!(backends.iter().any(|backend| backend.id() == "pnpm"));
        Ok(())
    }

    #[test]
    fn test_with_appended_extension() {
        let path = Path::new("/tmp/trusted/infra-mise.toml-a1b2c3d4e5f67890");
        let result = with_appended_extension(path, "hash");
        assert_eq!(
            result,
            Path::new("/tmp/trusted/infra-mise.toml-a1b2c3d4e5f67890.hash")
        );

        let result2 = with_appended_extension(path, "monorepo");
        assert_eq!(
            result2,
            Path::new("/tmp/trusted/infra-mise.toml-a1b2c3d4e5f67890.monorepo")
        );
    }
}

/// Deliberately not `#[cfg(unix)]` like the module above: Windows is the platform these are about.
/// The entry there is a plain file rather than a symlink, and reading it as one is the whole
/// defect — gated out, these would have passed by never being compiled. Same reasoning as
/// [`ignored_config_path_tests`].
#[cfg(test)]
mod ignore_entry_tests {
    use super::*;

    /// The entry goes in through `file::make_symlink_or_file`, the writer [`add_ignored`] uses, so
    /// each platform is exercised in the form it actually writes: a symlink on unix, a plain file
    /// holding the path on Windows.
    #[test]
    fn an_ignore_entry_resolves_to_the_config_it_records() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let store = tmp.path().join("ignored-configs");
        std::fs::create_dir_all(&store).unwrap();
        let entry = store.join("project-abc123");
        file::make_symlink_or_file(&project, &entry).unwrap();

        let resolved = ignored_entry_path(&entry).expect("an entry mise wrote has to resolve");

        assert_eq!(
            resolved, project,
            "it has to be the config that was ignored"
        );
        // And explicitly not the entry: `entry.canonicalize()` returned this on Windows, where the
        // entry is a plain file, which is why nothing ever matched the ignore list there.
        assert_ne!(resolved, entry, "never the entry's own path");
    }

    #[test]
    fn an_entry_mise_did_not_write_resolves_to_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("ignored-configs");
        let stray = store.join("a-directory");
        std::fs::create_dir_all(&stray).unwrap();

        assert_eq!(ignored_entry_path(&stray), None);
        assert_eq!(ignored_entry_path(&store.join("not-there")), None);
    }
}
