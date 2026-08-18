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
use crate::config::{AliasMap, Settings, settings};
use crate::deps::DepsConfig;
use crate::errors::Error::UntrustedConfig;
use crate::file::display_path;
use crate::hash::hash_to_str;
use crate::hooks::Hook;
use crate::redactions::Redactions;
use crate::task::{Task, TaskRustCacheConfig, TaskTemplate};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource, ToolVersionList, Toolset};
use crate::ui::{prompt, style};
use crate::watch_files::WatchFile;
use crate::{
    backend::{self, Backend},
    config, dirs, env, file, hash,
    registry::REGISTRY,
};
use eyre::{Result, eyre};
use idiomatic_version::IdiomaticVersionFile;
use indexmap::IndexMap;
use serde::Deserialize;
use std::sync::LazyLock as Lazy;
use tool_versions::ToolVersions;

use super::Config;

pub mod config_root;
pub mod diagnostic;
pub mod idiomatic_version;
pub mod min_version;
pub mod mise_toml;
pub mod toml;
pub mod tool_versions;

#[derive(Debug, PartialEq)]
pub enum ConfigFileType {
    MiseToml,
    ToolVersions,
    IdiomaticVersion(Vec<Arc<dyn Backend>>),
}

pub trait ConfigFile: Debug + Send + Sync {
    fn get_path(&self) -> &Path;
    fn min_version(&self) -> Option<&MinVersionSpec> {
        None
    }
    /// gets the project directory for the config
    /// if it's a global/system config, returns None
    /// files like ~/src/foo/.mise/config.toml will return ~/src/foo
    /// and ~/src/foo/.mise.config.toml will return None
    fn project_root(&self) -> Option<PathBuf> {
        let p = self.get_path();
        if config::is_global_config(p) {
            return None;
        }
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
    pub async fn add_runtimes(
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
    pub fn display_runtime(&self, runtimes: &[ToolArg]) -> eyre::Result<bool> {
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
    match detect_config_file_type(path).await {
        Some(ConfigFileType::MiseToml) => Ok(Arc::new(MiseToml::init(path))),
        Some(ConfigFileType::ToolVersions) => Ok(Arc::new(ToolVersions::init(path))),
        Some(ConfigFileType::IdiomaticVersion(backends)) => Ok(Arc::new(
            IdiomaticVersionFile::parse(path.to_path_buf(), backends).await?,
        )),
        None => Err(unsupported_config_file_error(path)),
    }
}

pub async fn parse_or_init(path: &Path) -> eyre::Result<Arc<dyn ConfigFile>> {
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

/// Lock a config file for a read-modify-write operation, then read its latest contents.
///
/// Callers must keep the returned lock alive until after [`ConfigFile::save`]. Acquiring the
/// lock before re-reading is what prevents two mise processes from both modifying the same stale
/// snapshot and silently overwriting one another's changes.
pub async fn lock_and_parse_or_init(
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

pub async fn parse(path: &Path) -> Result<Arc<dyn ConfigFile>> {
    if let Ok(settings) = Settings::try_get()
        && settings.paranoid
    {
        trust_check(path)?;
    }
    match detect_config_file_type(path).await {
        Some(ConfigFileType::MiseToml) => Ok(Arc::new(MiseToml::from_file(path)?)),
        Some(ConfigFileType::ToolVersions) => Ok(Arc::new(ToolVersions::from_file(path)?)),
        Some(ConfigFileType::IdiomaticVersion(backends)) => Ok(Arc::new(
            IdiomaticVersionFile::parse(path.to_path_buf(), backends).await?,
        )),
        None => Err(unsupported_config_file_error(path)),
    }
}

/// Whether parsing `path` requires a trust record.
///
/// Tracked config loading uses this to avoid interactive prompts without
/// discarding plain version files that never require trust.
pub async fn path_requires_trust(path: &Path) -> bool {
    if Settings::safe_mode() {
        return false;
    }
    if Settings::try_get().is_ok_and(|settings| settings.paranoid) {
        return true;
    }
    match detect_config_file_type(path).await {
        Some(ConfigFileType::MiseToml) => !MiseToml::path_is_trust_exempt(path),
        Some(ConfigFileType::ToolVersions) => ToolVersions::path_requires_trust(path),
        Some(ConfigFileType::IdiomaticVersion(_)) | None => false,
    }
}

pub fn config_trust_root(path: &Path) -> PathBuf {
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
pub fn is_path_trusted(path: &Path) -> bool {
    is_trusted(&config_trust_root(path)) || is_trusted(path)
}

static IMPLICITLY_TRUST_ACTIVE_CONFIG: AtomicBool = AtomicBool::new(false);

pub fn set_implicitly_trust_active_config(enabled: bool) {
    IMPLICITLY_TRUST_ACTIVE_CONFIG.store(enabled, Ordering::Relaxed);
}

pub fn trust_active_config() -> Result<()> {
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

pub fn trust_check(path: &Path) -> eyre::Result<()> {
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
        let ans = (settings::is_loaded() && Settings::get().yes)
            || prompt::confirm_with_all(format!(
                "{} config files in {} are not trusted. Trust them?",
                style::eyellow("mise"),
                style::epath(&config_root)
            ))?;
        if ans {
            trust(&config_root)?;
            return Ok(());
        } else if console::user_attended_stderr() {
            add_ignored(config_root.to_path_buf())?;
        }
    }
    Err(UntrustedConfig(path.into()))?
}

pub fn is_trusted(path: &Path) -> bool {
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
pub fn add_ignored(path: PathBuf) -> Result<()> {
    let path = path.canonicalize()?;
    file::create_dir_all(&*dirs::IGNORED_CONFIGS)?;
    file::make_symlink_or_file(&path, &ignore_path(&path))?;
    IS_IGNORED.lock().unwrap().insert(path);
    Ok(())
}
pub fn rm_ignored(path: PathBuf) -> Result<()> {
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
pub fn is_trusted_via_config_paths(path: &Path) -> bool {
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

/// Whether `path` is under an explicitly-configured `ignored_config_paths`
/// (`MISE_IGNORED_CONFIG_PATHS`) entry.
///
/// This is an explicit "never load this config" instruction and is a hard
/// block: it takes precedence over `trusted_config_paths`.
pub fn is_ignored_via_setting(path: &Path) -> bool {
    path_is_under_any(path, &env::MISE_IGNORED_CONFIG_PATHS)
}

/// Whether `path` is in the persisted ignore list.
///
/// Entries are recorded when the user answers "No" to a trust prompt or runs
/// `mise trust --ignore`. Unlike [`is_ignored_via_setting`], this only records
/// a dismissed prompt, so it is overridden by `trusted_config_paths` (see
/// [`is_trusted_via_config_paths`]).
pub fn is_persisted_ignored(path: &Path) -> bool {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if !dirs::IGNORED_CONFIGS.exists() {
            return;
        }
        let mut is_ignored = IS_IGNORED.lock().unwrap();
        for entry in file::ls(&dirs::IGNORED_CONFIGS).unwrap_or_default() {
            if let Ok(canonicalized_path) = entry.canonicalize() {
                is_ignored.insert(canonicalized_path);
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
pub fn is_ignored(path: &Path) -> bool {
    is_ignored_via_setting(path) || is_persisted_ignored(path)
}

pub fn trust(path: &Path) -> Result<()> {
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
pub fn mark_as_monorepo_root(path: &Path) -> Result<()> {
    let config_root = config_trust_root(path);
    let hashed_path = trust_path(&config_root);
    let monorepo_marker = with_appended_extension(&hashed_path, "monorepo");
    if !monorepo_marker.exists() {
        file::create_dir_all(monorepo_marker.parent().unwrap())?;
        file::write(&monorepo_marker, "")?;
    }
    Ok(())
}

pub fn untrust(path: &Path) -> eyre::Result<()> {
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
fn with_appended_extension(path: &Path, ext: &str) -> PathBuf {
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
        .flat_map(|rt| rt.idiomatic_files.iter().map(|f| f.path));
    !matching_idiomatic_filenames(path, filenames).is_empty()
}

fn unsupported_config_file_error(path: &Path) -> eyre::Report {
    if path_matches_registry_idiomatic(path) {
        eyre!(
            "cannot update idiomatic version file {}; use mise.toml, .tool-versions, or --path to choose a writable config file",
            display_path(path)
        )
    } else {
        eyre!("unknown config file type: {}", display_path(path))
    }
}

async fn path_is_idiomatic(path: &Path) -> Option<Vec<Arc<dyn Backend>>> {
    let (enable_tools, disable_files) = Settings::try_get()
        .map(|settings| {
            (
                settings.idiomatic_version_file_enable_tools.clone(),
                settings.idiomatic_version_file_disable_files.clone(),
            )
        })
        .unwrap_or_default();
    path_is_idiomatic_for_enabled_tools(path, &enable_tools, &disable_files).await
}

async fn path_is_idiomatic_for_enabled_tools(
    path: &Path,
    enable_tools: &BTreeSet<String>,
    disable_files: &BTreeSet<String>,
) -> Option<Vec<Arc<dyn Backend>>> {
    // Idiomatic version files are opt-in per tool. Skipping non-enabled backends is
    // also what keeps `idiomatic_filenames()` from booting a Lua VM for every
    // installed vfox plugin on every invocation just to classify a config path.
    if enable_tools.is_empty() {
        return None;
    }
    let mut backends_by_filename = BTreeMap::<String, Vec<Arc<dyn Backend>>>::new();
    for b in backend::list() {
        if !enable_tools.contains(b.id()) {
            continue;
        }
        match b.idiomatic_filenames().await {
            Ok(filenames) => {
                for filename in filenames {
                    if super::idiomatic_version_file_disabled(disable_files, b.id(), &filename) {
                        continue;
                    }
                    backends_by_filename
                        .entry(filename)
                        .or_default()
                        .push(b.clone());
                }
            }
            Err(e) => debug!("idiomatic_filenames failed for {}: {:?}", b, e),
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
    if backends.is_empty() {
        None
    } else {
        Some(backends)
    }
}

async fn detect_config_file_type(path: &Path) -> Option<ConfigFileType> {
    match path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("mise.toml")
    {
        f if env::MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES
            .as_ref()
            .is_some_and(|o| o.contains(f)) =>
        {
            Some(ConfigFileType::ToolVersions)
        }
        f if env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str() == f => {
            Some(ConfigFileType::ToolVersions)
        }
        f if env::MISE_OVERRIDE_CONFIG_FILENAMES.contains(f) => Some(ConfigFileType::MiseToml),
        f if env::MISE_DEFAULT_CONFIG_FILENAME.as_str() == f => Some(ConfigFileType::MiseToml),
        f => {
            if let Some(backends) = path_is_idiomatic(path).await {
                Some(ConfigFileType::IdiomaticVersion(backends))
            } else if path_matches_registry_idiomatic(path) {
                // Known idiomatic filenames stay unrecognized until the tool is
                // opted in. Do not fall through to MiseToml for names like
                // rust-toolchain.toml.
                None
            } else if f.ends_with(".toml") {
                Some(ConfigFileType::MiseToml)
            } else {
                None
            }
        }
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
pub struct TaskConfig {
    pub cascade: Option<bool>,
    pub includes: Option<Vec<String>>,
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
pub struct ToolConfig {
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
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn test_detect_config_file_type() {
        env::set_var("MISE_EXPERIMENTAL", "true");
        backend::load_tools().await.unwrap();
        // Idiomatic version files are opt-in; with the default (empty)
        // `idiomatic_version_file_enable_tools` they are not detected.
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.nvmrc")).await,
            None
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/package.json")).await,
            None
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/rust-toolchain.toml")).await,
            None
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.test-tool-versions")).await,
            Some(ConfigFileType::ToolVersions)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/mise.toml")).await,
            Some(ConfigFileType::MiseToml)
        );
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
    async fn test_path_is_idiomatic_for_enabled_tools() -> Result<()> {
        backend::load_tools().await?;
        let disable_files = BTreeSet::new();
        for (enabled, path) in [
            ("node", "/foo/bar/.nvmrc"),
            ("ruby", "/foo/bar/.ruby-version"),
            ("rust", "/foo/bar/rust-toolchain.toml"),
            ("goreleaser", "/foo/bar/.config/goreleaser.yaml"),
        ] {
            let enable_tools = BTreeSet::from([enabled.to_string()]);
            let backends =
                path_is_idiomatic_for_enabled_tools(Path::new(path), &enable_tools, &disable_files)
                    .await
                    .unwrap_or_else(|| panic!("{path} should be idiomatic for {enabled}"));
            assert!(backends.iter().any(|b| b.id() == enabled));
            // A file for a non-enabled tool must not match.
            assert!(
                path_is_idiomatic_for_enabled_tools(
                    Path::new(path),
                    &BTreeSet::from(["zig".to_string()]),
                    &disable_files,
                )
                .await
                .is_none()
            );
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

        let backends = path_is_idiomatic_for_enabled_tools(
            &path,
            &BTreeSet::from(["goreleaser".to_string()]),
            &BTreeSet::new(),
        )
        .await
        .expect("goreleaser should be matched from its nested idiomatic path");
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
    async fn test_path_is_idiomatic_respects_disabled_files() -> Result<()> {
        backend::load_tools().await?;
        let enabled = BTreeSet::from(["node".to_string(), "pnpm".to_string()]);
        let disabled = BTreeSet::from(["node:package.json".to_string()]);

        let backends =
            path_is_idiomatic_for_enabled_tools(Path::new("package.json"), &enabled, &disabled)
                .await
                .expect("package.json should remain idiomatic for package managers");

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
