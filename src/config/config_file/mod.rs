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

use crate::args::{BackendArg, ToolArg};
use crate::config::config_file::min_version::MinVersionSpec;
use crate::config::config_file::mise_toml::{MiseToml, MonorepoConfig};
use crate::config::env_directive::EnvDirective;
use crate::config::provenance::ConfigProvenance;
use crate::config::settings::IdiomaticVersionFileSettings;
use crate::config::{AliasMap, CommandWrapper, Settings, SettingsExt, settings};
use crate::deps::DepsConfig;
use crate::errors::Error::{UntrustedConfig, UntrustedEnvFile};
use crate::errors::Untrusted;
use crate::file::display_path;
use crate::hash::hash_to_str;
use crate::hooks::Hook;
use crate::redactions::Redactions;
use crate::task::{Task, TaskTemplate};
use crate::toolset::{
    ToolRequest, ToolRequestSet, ToolSource, ToolVersionList, ToolVersionOptions, Toolset,
};
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
use indexmap::{IndexMap, IndexSet};
use std::sync::LazyLock as Lazy;
use tool_versions::ToolVersions;

use super::Config;

pub mod config_root;
pub mod diagnostic;
pub mod idiomatic_version;
pub(crate) mod min_version;
pub mod mise_toml;
pub(crate) mod tool_versions;
mod typed_consent;
mod unknown_key;

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

/// Refuse to write a version that the config file would render as a template when it is loaded.
///
/// `mise use --pin` and `mise upgrade --bump` write the version a version list or release API
/// reported, so the text is not the user's, and a template in a trusted config can run commands
/// with exec() and read the environment.
fn ensure_versions_are_not_templates(
    path: &Path,
    ba: &BackendArg,
    versions: &[ToolRequest],
) -> Result<()> {
    for tr in versions {
        let version = tr.version();
        if crate::tera::contains_template_syntax(&version) {
            return Err(eyre!(
                "refusing to write {}@{version} to {}: the version contains template syntax",
                ba.short,
                display_path(path)
            ));
        }
    }
    Ok(())
}

/// A templated tool entry's options as written and as the config that declares them renders
/// them, so a rewrite can put the template back in place of the value it rendered to.
#[derive(Debug, Clone)]
pub struct ToolOptionTemplate {
    /// The entry's version, rendered, to tell which entry a request came from.
    pub version: String,
    pub written: ToolVersionOptions,
    pub rendered: ToolVersionOptions,
}

pub trait ConfigFile: Debug + Send + Sync {
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
    /// The tools with a template in their version (`node = "{{ vars.node }}"`) or
    /// options that this platform resolves, each once.
    fn templated_tool_backends(&self) -> Vec<String> {
        vec![]
    }
    /// This config's entries for `ba` that have a template in them, each rendered in this
    /// config's own context.
    fn tool_option_templates(&self, _ba: &BackendArg) -> Vec<ToolOptionTemplate> {
        vec![]
    }
    /// A hash of the text this config was parsed from, for a config that keeps one,
    /// to tell that its file changed after it was loaded.
    fn loaded_hash(&self) -> Option<String> {
        None
    }
    /// Every tool this config sets, templated or not.
    fn tool_backends(&self) -> Vec<String> {
        self.to_tool_request_set()
            .map(|set| {
                set.list_tools()
                    .iter()
                    .map(|ba| ba.short.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Whether any tool's version or options is a template, on any platform.
    fn has_templated_tool_versions(&self) -> bool {
        !self.templated_tool_backends().is_empty()
    }
    /// [`ConfigFile::to_tool_request_set`] without the templated versions, for a
    /// caller that has another source for them.
    fn to_tool_request_set_skipping_templated(&self) -> eyre::Result<ToolRequestSet> {
        self.to_tool_request_set()
    }
    fn aliases(&self) -> eyre::Result<AliasMap> {
        Ok(Default::default())
    }
    /// The backend each `[tool_alias]` stands for, by alias name. Unlike [`Self::aliases`]
    /// it does not render version aliases, so one that fails does not hide these.
    fn backend_aliases(&self) -> IndexMap<String, String> {
        self.aliases()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(name, alias)| Some((name, alias.backend?)))
            .collect()
    }

    fn settings(&self) -> Option<&settings::SettingsPartial> {
        None
    }

    fn daemon_declarations(&self) -> IndexMap<String, crate::daemons::Declaration> {
        IndexMap::new()
    }

    fn daemon_providers(&self) -> IndexMap<String, ::toml::Table> {
        Default::default()
    }
    fn daemon_settings(&self) -> Option<crate::daemons::DaemonSettings> {
        None
    }
    fn daemon_group_declarations(&self) -> IndexMap<String, crate::daemons::GroupDeclaration> {
        IndexMap::new()
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

    /// The remote config fragments this file's `include` names.
    fn remote_includes(&self) -> eyre::Result<Vec<String>> {
        Ok(vec![])
    }

    /// A copy of this file with the fetched remote fragments (`(cache file,
    /// body)`, in include order) merged in below its own entries.
    fn with_remote_fragments(
        &self,
        _fragments: Vec<(PathBuf, String)>,
    ) -> eyre::Result<Option<Arc<dyn ConfigFile>>> {
        Ok(None)
    }

    /// The cache files of the fragments merged into this file.
    fn included_paths(&self) -> Vec<PathBuf> {
        vec![]
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

    fn doctor_config(&self) -> crate::config::doctor::DoctorConfig {
        Default::default()
    }

    /// Render a check's `run`, `description` and `hint` with this file's template context.
    fn render_doctor_check(
        &self,
        check: &crate::config::doctor::DoctorCheck,
    ) -> Result<crate::config::doctor::DoctorCheck> {
        Ok(check.clone())
    }

    fn bootstrap_config(&self) -> Option<mise_config::bootstrap::BootstrapTomlConfig> {
        None
    }

    fn secrets_config(&self) -> Option<::toml::Value> {
        None
    }

    fn dotfiles_config(&self) -> Option<mise_config::dotfiles::DotfilesTomlConfig> {
        None
    }

    fn dotfile_groups_config(&self) -> Option<mise_config::dotfiles::DotfileGroupsTomlConfig> {
        None
    }

    fn history_config(&self) -> Option<mise_config::history::HistoryTomlConfig> {
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
                        lockfile_scope,
                        options,
                        backend,
                    } = tr
                    {
                        tr = ToolRequest::Version {
                            version: tv.version,
                            source,
                            lockfile_scope,
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

/// Refuse a path mise would not read back as a TOML config.
///
/// [`parse_or_init`] gives this check to every caller that goes through a [`ConfigFile`]. Callers
/// that write TOML directly — `mise set` builds a `MiseToml` itself — bypassed it and would happily
/// create a file that config detection then refuses to recognize, or write TOML into a name mise
/// reads as `.tool-versions`. One definition of "a path mise can write TOML to", rather than two
/// that drift apart.
pub async fn ensure_writable_as_toml(path: &Path) -> eyre::Result<()> {
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
    let settings = IdiomaticVersionFileSettings::current();
    if let Ok(current_settings) = Settings::try_get()
        && current_settings.paranoid.config()
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

/// Whether a tool version string only takes effect once the config is trusted.
///
/// A `path:<dir>` version names a directory that is already on disk, so it needs
/// no install step: its `bin` is put on PATH as soon as the config loads, which
/// would let an untrusted repository shadow any command.
pub(crate) fn version_requires_trust(version: &str) -> bool {
    version.starts_with("path:")
}

/// Whether a `[tools]` key only takes effect once the config is trusted.
///
/// Inline options (`tool[postinstall=...]`) can run code or redirect a backend.
/// An `spm:` package named by the URL of a self-hosted forge makes mise derive
/// that host's API from the name and send it the user's forge token, as an
/// `api_url` option would.
fn tool_key_requires_trust(tool: &str) -> bool {
    tool.contains('[') || crate::backend::spm::tool_key_selects_api_host(tool)
}

/// Whether a detected config requires trust before tracked loading may parse it.
pub(super) async fn detection_requires_trust(path: &Path, detection: &ConfigFileDetection) -> bool {
    if Settings::safe_mode() {
        return false;
    }
    if Settings::try_get().is_ok_and(|settings| settings.paranoid.config()) {
        return true;
    }
    match detection {
        ConfigFileDetection::Recognized(ConfigFileType::MiseToml) => {
            !MiseToml::path_is_trust_exempt(path)
        }
        ConfigFileDetection::Recognized(ConfigFileType::ToolVersions) => {
            ToolVersions::path_requires_trust(path)
        }
        ConfigFileDetection::Recognized(ConfigFileType::IdiomaticVersion(backends)) => {
            IdiomaticVersionFile::path_requires_trust(path, backends).await
        }
        ConfigFileDetection::DisabledIdiomatic
        | ConfigFileDetection::Unknown
        | ConfigFileDetection::DiscoveryFailed(_) => false,
    }
}

/// The trust root `mise trust` and `mise untrust` act on for a path the user named.
///
/// With `env_files` on and `config` off, an env file is its own root so that approving it is
/// bound to its content while config files keep the normal trust root.
pub fn trust_root_for_cli(path: &Path) -> PathBuf {
    if settings::is_loaded() {
        let paranoid = &Settings::get().paranoid;
        if paranoid.env_files() && !paranoid.config() && !looks_like_config_file(path) {
            return path.to_path_buf();
        }
    }
    config_trust_root(path)
}

/// Whether a path the user names is a mise config file, as opposed to an env file or a script.
///
/// Only mise's own config names and the ones the user's environment sets count. The names in
/// effect also hold the overrides a project `.miserc.toml` sets without trust, and one that
/// lists `.env` would turn an approval of that file into trust of the whole project.
fn looks_like_config_file(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    name.ends_with(".toml") && (name.contains("mise") || name == "config.toml")
        || name.contains("tool-versions")
        || config::has_own_config_filename(path)
}

pub fn config_trust_root(path: &Path) -> PathBuf {
    if settings::is_loaded() && Settings::get().paranoid.config() {
        path.to_path_buf()
    } else {
        config_root::linked_project(path).unwrap_or_else(|| config_root::config_root(path))
    }
}

/// Whether the file or its trust root has been trusted.
///
/// Unlike a passing [`trust_check`], this is false for files that merely do
/// not *need* trust (e.g. safe configs loaded without it).
///
/// A config reached through a symlink inside its project is judged by that project alone:
/// trust in what the link leads to, another project or a file approved by content, is not trust
/// in the project that holds the link.
pub fn is_path_trusted(path: &Path) -> bool {
    if let Some(project) = config_root::linked_project(path) {
        return is_trusted(&project);
    }
    is_trusted(&config_trust_root(path)) || is_trusted(path)
}

static IMPLICITLY_TRUST_ACTIVE_CONFIG: AtomicBool = AtomicBool::new(false);

/// Set while `mise trust` resolves the env to find files that need trust. Loading an untrusted
/// config there would run its templates just to look for files. With this set, an untrusted
/// config is skipped like it is for `hook-env`, and nothing prompts.
static STRICT_TRUST: AtomicBool = AtomicBool::new(false);

/// Env files and scripts that paranoid mode skipped as untrusted, with the config that named
/// each. A file the `env_file` setting names has no config.
static UNTRUSTED_ENV_FILES: Mutex<Vec<(PathBuf, Option<PathBuf>)>> = Mutex::new(Vec::new());

/// The mise.toml files this process wrote, such as the one `mise use` or
/// `mise unuse` edited. The lockfiles regenerated after an install include
/// theirs, wherever they live.
static SAVED_CONFIGS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn note_saved(path: &Path) {
    SAVED_CONFIGS.lock().unwrap().push(path.to_path_buf());
}

/// The mise.toml files this process has written so far.
pub fn saved_configs() -> Vec<PathBuf> {
    SAVED_CONFIGS.lock().unwrap().clone()
}

/// Makes untrusted config skip instead of load or prompt, even for `mise trust`.
pub fn require_strict_trust() {
    STRICT_TRUST.store(true, Ordering::Relaxed);
}

/// The env files and scripts skipped as untrusted so far, each with the config that named it.
pub fn untrusted_env_files() -> Vec<(PathBuf, Option<PathBuf>)> {
    UNTRUSTED_ENV_FILES.lock().unwrap().clone()
}

/// Set on a `mise` that acts for someone other than the user who typed it,
/// such as the children the MCP server starts. Such a process trusts nothing
/// on its own: not the config its command would implicitly trust, and not on
/// a `yes` setting. Untrusted config fails with [`UntrustedConfig`] instead,
/// and so do the task files a typed `mise run` would be the consent to run.
/// The variable is inherited, so a `mise` a task starts follows it too.
pub const REQUIRE_EXPLICIT_TRUST_ENV: &str = "__MISE_REQUIRE_EXPLICIT_TRUST";

static REQUIRE_EXPLICIT_TRUST: Lazy<bool> =
    Lazy::new(|| std::env::var_os(REQUIRE_EXPLICIT_TRUST_ENV).is_some_and(|v| !v.is_empty()));

/// Whether this process trusts nothing on its own. See [`REQUIRE_EXPLICIT_TRUST_ENV`].
pub fn requires_explicit_trust() -> bool {
    *REQUIRE_EXPLICIT_TRUST
}

pub fn set_implicitly_trust_active_config(enabled: bool) {
    IMPLICITLY_TRUST_ACTIVE_CONFIG.store(enabled, Ordering::Relaxed);
}

fn implicitly_trusts_active_config() -> bool {
    IMPLICITLY_TRUST_ACTIVE_CONFIG.load(Ordering::Relaxed) && !*REQUIRE_EXPLICIT_TRUST
}

pub fn trust_active_config() -> Result<()> {
    if !implicitly_trusts_active_config() {
        return Ok(());
    }
    let Ok(settings) = Settings::try_get() else {
        return Ok(());
    };
    if settings.paranoid.config() || Settings::safe_mode() {
        return Ok(());
    }
    let mut newly_trusted = false;
    for path in config::load_config_paths(&config::DEFAULT_CONFIG_FILENAMES, false) {
        if config::is_global_config(&path) {
            continue;
        }
        let config_root = config_trust_root(&path);
        if !implicit_trust_may_record(&path, &config_root) {
            continue;
        }
        if !is_trusted(&config_root) {
            trust(&config_root)?;
            newly_trusted = true;
        }
    }
    // The settings snapshot was built before this config was trusted, so it left out the
    // project's `[settings]`.
    if newly_trusted {
        Settings::reload()?;
    }
    Ok(())
}

/// Trusts a config root. The settings snapshot was built without the project's
/// `[settings]` while it was untrusted, so rebuild it to pick them up.
fn trust_and_reload_settings(config_root: &Path) -> eyre::Result<()> {
    trust_then_reload_settings(config_root, || {})
}

/// [`trust_and_reload_settings`] that runs `on_trusted` once the record is saved, before the
/// reload, which can fail on the project's own `[settings]`.
fn trust_then_reload_settings(config_root: &Path, on_trusted: impl FnOnce()) -> eyre::Result<()> {
    let was_trusted = is_trusted(config_root);
    trust(config_root)?;
    on_trusted();
    if !was_trusted && settings::is_loaded() {
        Settings::reload()?;
    }
    Ok(())
}

/// Whether a command that implies trust may record it for `path`. An ignore always wins: the user
/// refused that config, and trust implied by a command neither overrides nor removes the refusal,
/// even where something else (a trusted monorepo root, a file record) already trusts the file.
fn implicit_trust_may_record(path: &Path, config_root: &Path) -> bool {
    !is_ignored(config_root) && !is_ignored(path) && !project_is_ignored(path)
}

/// Whether `path` sits in a config root nested in a project (a conf.d folder, `.config/mise`)
/// whose project is ignored. Ignoring a project ignores those roots too, including one that
/// arrives after the ignore.
pub(crate) fn project_is_ignored(path: &Path) -> bool {
    config_root::nested_root_project(path).is_some_and(|project| is_ignored(&project))
}

/// Checks whether a config may load, prompting for trust when appropriate.
/// Automatic yes decisions cannot grant trust in paranoid mode.
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
    if implicitly_trusts_active_config()
        && !*mise_util::env::IS_CI
        && Settings::try_get().is_ok_and(|settings| !settings.paranoid.config())
    {
        let config_root = config_trust_root(path);
        if implicit_trust_may_record(path, &config_root) {
            trust_and_reload_settings(&config_root)?;
            return Ok(());
        }
    }
    static MUTEX: Mutex<()> = Mutex::new(());
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple checks at once so we don't prompt multiple times for the same path
    let config_root = config_trust_root(path);
    let default_cmd = String::new();
    let args = env::ARGS.read().unwrap();
    // A shim's arguments belong to the tool it stands for, not to mise.
    let cmd = if *env::IS_RUNNING_AS_SHIM {
        default_cmd.as_str()
    } else {
        args.get(1).unwrap_or(&default_cmd).as_str()
    };
    let strict = STRICT_TRUST.load(Ordering::Relaxed);
    // No command is exempt: `mise trust` decides about config without loading it, and the
    // word `trust` in argv says nothing about who is asking (`docker trust …` through a shim).
    if is_path_trusted(path) || mise_util::testing::in_tests() {
        return Ok(());
    }
    if *REQUIRE_EXPLICIT_TRUST {
        Err(UntrustedConfig(path.into(), Untrusted::NotTrusted))?
    }
    ensure_trust_can_approve(path)?;
    if cmd != "hook-env" && !strict && !is_ignored(&config_root) && !is_ignored(path) {
        // A typed command implies trust like the commands above, so an ignored project's
        // nested root is left to the prompt too.
        if implicit_trust_may_record(path, &config_root)
            && typed_consent::trust_typed_command(path, &config_root)?
        {
            return Ok(());
        }
        let ans = if settings::is_loaded()
            && Settings::try_get().is_ok_and(|settings| settings.yes && !settings.paranoid.config())
        {
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
                trust_and_reload_settings(&config_root)?;
                return Ok(());
            }
            // Only a real decline is worth remembering. An ignore marker is
            // sticky and silent, so recording one for an answer nobody gave
            // would stop the config from applying with nothing to explain why.
            Confirmation::No => add_ignored(config_root.to_path_buf())?,
            Confirmation::Unanswered | Confirmation::Unavailable => {}
        }
    }
    Err(UntrustedConfig(path.into(), Untrusted::NotTrusted))?
}

/// Fails with [`Untrusted::LinkedInProject`] for a config that paranoid mode reaches through a
/// symlink inside a project. Paranoid mode approves a file by its content, and a link inside a
/// project is that project's to change, so no trust record can approve such a config: loading it
/// and `mise trust` both refuse it.
pub fn ensure_trust_can_approve(path: &Path) -> eyre::Result<()> {
    if Settings::try_get().is_ok_and(|settings| settings.paranoid.config())
        && let Some(project) = config_root::linked_project(path)
    {
        Err(UntrustedConfig(
            path.into(),
            Untrusted::LinkedInProject(project),
        ))?
    }
    Ok(())
}

/// Whether paranoid mode lets a file named by `env._.file` load.
///
/// Paranoid mode binds trust to content, so approving `mise.toml` must not approve the dotenv,
/// JSON, YAML or TOML file it points at: an edit to that file changes the environment of every
/// command. A file under `trusted_config_paths`, which only global config can set, stays trusted
/// by path.
///
/// An untrusted file is an error, except where untrusted config is being skipped (`hook-env`,
/// run on entering a directory): there it is left out and reported with the skipped configs,
/// so the trusted ones still apply. `Ok(false)` means "do not load it": the file is untrusted
/// and skipped, or it is not there.
pub(crate) fn env_file_trust_check(path: &Path) -> eyre::Result<bool> {
    env_file_trust_check_from(path, None)
}

/// [`env_file_trust_check`] for a file that `declared_in` names, so a skipped file is recorded
/// with the config that asked for it.
///
/// In safe mode it refuses every file. Safe mode drops the `_.file` and `_.source` entries of
/// project config before they get here, and callers do not ask about a file the operator named
/// outright, so what is left was found by searching (`env_file`, `find_up`, a glob) and can be a
/// file a repository supplies. Trust does not bring it back, as it does not bring back a
/// trusted project's `[env]`.
pub(crate) fn env_file_trust_check_from(
    path: &Path,
    declared_in: Option<&Path>,
) -> eyre::Result<bool> {
    if Settings::safe_mode() {
        debug!("safe mode: not loading env file {}", display_path(path));
        return Ok(false);
    }
    named_env_file_trust_check(path, declared_in)
}

/// [`env_file_trust_check_from`] for a file the operator named outright, which safe mode keeps.
/// Paranoid mode still needs it approved.
pub(crate) fn named_env_file_trust_check(
    path: &Path,
    declared_in: Option<&Path>,
) -> eyre::Result<bool> {
    if !Settings::try_get().is_ok_and(|settings| settings.paranoid.env_files()) {
        return Ok(true);
    }
    // A file that is not there is not loaded. Returning false rather than true means one that
    // appears before the caller reads it is not loaded unchecked either. Any other failure to
    // resolve it is not proof it is absent, so it must not load unchecked.
    let canonical = match path.canonicalize() {
        Ok(canonical) => canonical,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(eyre!(
                "cannot check paranoid-mode trust for {}: {err}",
                display_path(path)
            ));
        }
    };
    // An env file is its own trust root, and its trust is always bound to its content, even
    // when the `config` category is off and project config is trusted the normal way.
    if check_trust(path, &canonical, true, TrustAs::EnvFile) {
        return Ok(true);
    }
    if config::skip_untrusted_path(path) {
        let mut files = UNTRUSTED_ENV_FILES.lock().unwrap();
        let entry = (path.to_path_buf(), declared_in.map(Path::to_path_buf));
        if !files.contains(&entry) {
            files.push(entry);
        }
        return Ok(false);
    }
    Err(UntrustedEnvFile(path.into()))?
}

pub fn is_trusted(path: &Path) -> bool {
    is_trusted_with(path, Settings::get().paranoid.config())
}

/// Whether an env file has content-bound trust, which a trusted project config does not give.
pub fn is_env_file_trusted(path: &Path) -> bool {
    match path.canonicalize() {
        Ok(canonical) => check_trust(path, &canonical, true, TrustAs::EnvFile),
        Err(err) => {
            debug!("trust canonicalize: {err}");
            false
        }
    }
}

/// What a trust check approves the file for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TrustAs {
    /// Config, which an approval of the file as an env file does not trust, and which the
    /// process remembers once trusted.
    Config,
    /// An env file, whose trust is bound to its content and checked again on every call, so a
    /// long-lived process (a reload after the file was edited) does not keep accepting changed
    /// contents.
    EnvFile,
}

/// [`is_trusted`] with the trust rules chosen by the caller: `content_bound` ties trust to the
/// file's hash and turns off the shortcuts that trust by path, namely a monorepo marker, a
/// sibling git worktree, and CI.
fn is_trusted_with(path: &Path, content_bound: bool) -> bool {
    match path.canonicalize() {
        Ok(canonical) => check_trust(path, &canonical, content_bound, TrustAs::Config),
        Err(err) => {
            debug!("trust canonicalize: {err}");
            false
        }
    }
}

/// The trust check for `path`, whose canonical form the caller resolved once.
fn check_trust(path: &Path, canonical: &Path, content_bound: bool, kind: TrustAs) -> bool {
    // The `ignored_config_paths` setting is an explicit "never load this
    // config" instruction and is a hard block that takes precedence over
    // everything, including `trusted_config_paths`.
    if is_config_ignored_via_setting(path) {
        return false;
    }
    if kind == TrustAs::Config && IS_TRUSTED.lock().unwrap().contains(canonical) {
        return true;
    }
    let record = TrustRecord::find(canonical);
    // An env file approved as one is approved for that, never as config: the same path can
    // load as config through a link, a directory that replaced it or a filename setting.
    // Only `trusted_config_paths`, which global config sets, trusts it as config. Such a file
    // is never remembered as trusted, so the memo above cannot answer for it.
    let env_file_approval = kind == TrustAs::Config && record.is_env_file_approval();
    let trusted = if env_file_approval {
        trusted_config_path_matches(canonical)
    } else {
        // `trusted_config_paths` is trusted by configuration and overrides the
        // persisted ignore list (a dismissed trust prompt or `mise trust
        // --ignore`), matching the "still trusted via settings" warnings emitted by
        // `mise trust`. It only ever turns a "not trusted" answer into "trusted",
        // so it is the fallback: matching it canonicalizes every entry, which is a
        // real filesystem call each (slow on autofs/network mounts) and would
        // otherwise be paid on every startup even for configs trusted by record.
        is_trusted_without_config_paths(path, canonical, content_bound, &record)
            || trusted_config_path_matches(canonical)
    };
    if trusted && kind == TrustAs::Config && !env_file_approval {
        add_trusted(canonical.to_path_buf());
    }
    trusted
}

/// Whether the trust root a file belongs to is on the ignore list. `mise trust --ignore` and a
/// declined prompt record the root, so the files in it count as ignored too, ahead of every rule
/// that trusts a file by where it lies (a trusted monorepo root, a sibling git worktree, CI).
/// A directory is a trust root of its own and was looked up by itself.
fn root_is_ignored(path: &Path, canonicalized_path: &Path) -> bool {
    !canonicalized_path.is_dir()
        && (is_persisted_ignored(&config_trust_root(path)) || project_is_ignored(path))
}

/// The trust rules that need no `trusted_config_paths` lookup.
fn is_trusted_without_config_paths(
    path: &Path,
    canonicalized_path: &Path,
    content_bound: bool,
    record: &TrustRecord,
) -> bool {
    // The persisted ignore list blocks trust only when the path is not trusted
    // via `trusted_config_paths`, which the caller checks afterwards.
    if is_canonical_path_persisted_ignored(canonicalized_path) {
        return false;
    }
    if config::is_global_config(path) {
        return true;
    }
    if !content_bound && root_is_ignored(path, canonicalized_path) {
        return false;
    }
    // The marker trusts descendants by path, so paranoid mode, which binds
    // trust to file contents, ignores it and checks each file's hash below.
    if !content_bound && trusted_monorepo_ancestor(canonicalized_path) {
        return true;
    }
    if content_bound {
        let trusted = record.matches_content(path).unwrap_or_else(|e| {
            warn!("trust_file_hash: {e}");
            false
        });
        if !trusted {
            return false;
        }
    } else if mise_util::testing::in_tests() || *mise_util::env::IS_CI {
        // in tests/CI we trust everything
        return true;
    } else if !record.owned {
        // No direct trust record. A config inside a linked git worktree
        // shares trust with the equivalent path in the repo's main checkout.
        // Not applicable in paranoid mode (excluded above), where trust is
        // tied to file contents that can differ between worktree branches,
        // nor to a main-checkout record with a hash, which approves the
        // content of the file there and not the path.
        return crate::git::main_checkout_equivalent(canonicalized_path)
            .is_some_and(|main_path| !has_trust_hash(&main_path) && is_trusted(&main_path));
    } else if record.has_hash() && !record.matches_content(path).unwrap_or(false) {
        // A record with a hash approves the content of one file, so it stops counting once
        // that content changes, also when the path is loaded as config.
        return false;
    }
    true
}

/// Whether a directory above `canonical` is a trusted monorepo root, which trusts the configs
/// below it. A root is marked with a sidecar of its trust record when trusted. The marker only
/// extends the root's own trust to its descendants, so it counts for nothing unless the root is
/// still trusted itself: trust that ended (`mise trust --ignore`, a `trusted_config_paths` entry
/// removed, a CI run) must not live on in a marker.
///
/// `canonical` has no symlinks, and so neither has any of its ancestors: each one's record name
/// is derived from it as it is, and only a directory that has a marker is checked further.
fn trusted_monorepo_ancestor(canonical: &Path) -> bool {
    canonical.ancestors().skip(2).any(|dir| {
        let record = dirs::TRUSTED_CONFIGS.join(record_name(dir));
        with_appended_extension(&record, "monorepo").exists() && is_trusted_with(dir, false)
    })
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
pub(crate) fn rm_ignored(path: PathBuf) -> Result<()> {
    let path = path.canonicalize()?;
    for ignore_path in records_for(&dirs::IGNORED_CONFIGS, &path) {
        file::remove_file(&ignore_path)?;
    }
    IS_IGNORED.lock().unwrap().remove(&path);
    Ok(())
}
/// Whether an already-canonicalized path lives under a `trusted_config_paths`
/// entry. Callers with a raw path use [`is_trusted_via_config_paths`], which
/// canonicalizes first.
fn trusted_config_path_matches(canonicalized_path: &Path) -> bool {
    let settings = Settings::get();
    // A canonical path has no symlinks, so an entry written as a prefix of it is a real
    // directory and matches without resolving anything. Only when none does are the entries
    // canonicalized, which costs a filesystem call each.
    settings
        .trusted_config_paths_as_written()
        .any(|p| canonicalized_path.starts_with(p))
        || settings
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

    /// Matches `path` as written, without resolving symlinks.
    fn is_match_as_written(&self, path: &Path) -> bool {
        self.literals.iter().any(|p| path.starts_with(p))
            || self.globs.iter().any(|glob| glob.is_match(path))
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
static GLOBAL_IGNORED_CONFIG_PATH_MATCHER: Lazy<IgnoredConfigPathMatcher> =
    Lazy::new(|| IgnoredConfigPathMatcher::new(&env::MISE_GLOBAL_IGNORED_CONFIG_PATHS));

tokio::task_local! {
    static GLOBAL_IGNORE_SCOPE: ();
}

/// Runs `future` with `ignored_config_paths` limited to the env var and the
/// system/global miserc files. Tracked configs belong to other projects, so an
/// ignore from the invoking directory's `.miserc.toml` must not hide them from
/// `mise prune` or `mise upgrade` (which would drop versions they still pin).
pub(crate) async fn with_global_ignored_config_paths<T>(
    future: impl std::future::Future<Output = T>,
) -> T {
    GLOBAL_IGNORE_SCOPE.scope((), future).await
}

/// Whether `path` is under an explicitly-configured `ignored_config_paths`
/// (`MISE_IGNORED_CONFIG_PATHS`) entry.
///
/// This is an explicit "never load this config" instruction and is a hard
/// block: it takes precedence over `trusted_config_paths`.
pub(crate) fn is_ignored_via_setting(path: &Path) -> bool {
    if GLOBAL_IGNORE_SCOPE.try_with(|_| ()).is_ok() {
        GLOBAL_IGNORED_CONFIG_PATH_MATCHER.is_match(path)
    } else {
        IGNORED_CONFIG_PATH_MATCHER.is_match(path)
    }
}

/// Whether the config file at `path`, or its trust root, is ignored by the
/// `ignored_config_paths` setting.
///
/// A global or system config is matched as discovered, not by its symlink
/// target. With `~/.config/mise` symlinked into a project, an entry that
/// ignores the project's copy would otherwise resolve onto the global config
/// too and hide it.
///
/// Only the env var and the system/global miserc files can ignore a global or
/// system config. A project `.miserc.toml` loads without trust, so letting it
/// name that config would drop the global-only settings held there, such as
/// `paranoid` and `safe`.
pub(crate) fn is_config_ignored_via_setting(path: &Path) -> bool {
    let root = config_trust_root(path);
    if !config::is_global_config(path) {
        return is_ignored_via_setting(&root) || is_ignored_via_setting(path);
    }
    let matcher = &*GLOBAL_IGNORED_CONFIG_PATH_MATCHER;
    matcher.is_match_as_written(&root) || matcher.is_match_as_written(path)
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
    match file::canonicalize_cached(path) {
        Some(path) => is_canonical_path_persisted_ignored(&path),
        None => {
            debug!("is_persisted_ignored: path canonicalize failed");
            true
        }
    }
}

/// [`is_persisted_ignored`] for a path that is already canonical.
fn is_canonical_path_persisted_ignored(path: &Path) -> bool {
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
    IS_IGNORED.lock().unwrap().contains(path)
}

/// Whether `path` is ignored, by either the `ignored_config_paths` setting or
/// the persisted ignore list. Callers that need to respect the
/// `trusted_config_paths` override use the finer-grained variants directly.
pub(crate) fn is_ignored(path: &Path) -> bool {
    is_ignored_via_setting(path) || is_persisted_ignored(path)
}

pub fn trust(path: &Path) -> Result<()> {
    trust_as(path, TrustAs::Config)
}

/// Approves `path` as an env file, which is never trust in the same path as config, whichever
/// paranoid categories are on.
pub fn trust_env_file(path: &Path) -> Result<()> {
    trust_as(path, TrustAs::EnvFile)
}

/// Records trust in `path` for `kind`. [`trust`] approves an env file too, under only the
/// `env_files` category, when the file is not named like a config.
fn trust_as(path: &Path, kind: TrustAs) -> Result<()> {
    rm_ignored(path.to_path_buf())?;
    let hashed_path = trust_path(path);
    let canonicalized_path = path.canonicalize()?;
    // What this writes decides the next check, which the memo would answer from the old state.
    IS_TRUSTED.lock().unwrap().remove(&canonicalized_path);
    if !record_points_at(&hashed_path, &canonicalized_path) {
        file::create_dir_all(hashed_path.parent().unwrap())?;
        file::make_symlink_or_file(&canonicalized_path, &hashed_path)?;
    }
    // The hash is only read for content-bound trust. Record it for an env file too, which is
    // bound to its content when only the `env_files` category is on. A trust root that is a
    // directory is a config trusted the normal way and has no content to hash.
    let paranoid = &Settings::get().paranoid;
    let trust_hash_path = with_appended_extension(&hashed_path, "hash");
    let env_file_path = with_appended_extension(&hashed_path, ENV_FILE_APPROVAL);
    let env_file = kind == TrustAs::EnvFile
        || (paranoid.env_files()
            && !paranoid.config()
            && path.is_file()
            && !looks_like_config_file(path));
    if env_file || paranoid.config() || (paranoid.env_files() && path.is_file()) {
        let hash = hash::file_hash_sha256(path, None)?;
        file::write(trust_hash_path, hash)?;
    } else if trust_hash_path.exists() {
        // This is trust by path. A hash left by an approval of a file that was at this path
        // would keep the record bound to that file's content.
        file::remove_file(&trust_hash_path)?;
    }
    // An env file approval, which `is_trusted` never takes as trust in the path as config.
    if env_file {
        file::write(env_file_path, "")?;
    } else if env_file_path.exists() {
        file::remove_file(&env_file_path)?;
    }
    Ok(())
}

/// The sidecar of a trust record that approves an env file and not a config.
pub const ENV_FILE_APPROVAL: &str = "env-file";

/// Whether the trust record of `path` approves it as an env file only.
#[cfg(test)]
fn is_env_file_approval(path: &Path) -> bool {
    TrustRecord::find(&record_identity(path)).is_env_file_approval()
}

/// Whether the trust record of `path` is bound to the content of a file.
fn has_trust_hash(path: &Path) -> bool {
    TrustRecord::find(&record_identity(path)).has_hash()
}

/// Records trust for the root of a config file mise has just written for the user, as
/// `mise use` and `mise set` do. Safe mode loads config without a trust decision, so a write
/// there has none to record and leaves trust as it was.
pub fn trust_after_write(path: &Path) -> Result<()> {
    if Settings::safe_mode() {
        return Ok(());
    }
    trust(&config_trust_root(path))
}

/// Asks for the trust that [`trust_after_write`] is about to record, before the write.
///
/// That record is for the whole config root, so it also covers the other config files there.
/// A command that loads the full config (`mise use`) has checked them already, but `mise set`,
/// `mise unset` and `mise tasks add` read only the file they edit: a sibling mise refused to
/// load (`mise.local.toml`, `.config/mise/conf.d/*.toml`, a profile of another `MISE_ENV`)
/// would start to load without anyone having decided to trust it. Each one that needs trust
/// goes through [`trust_check`], which prompts, or fails without a terminal, like any other
/// untrusted config. So does the file itself, for a writer that edits it as text.
///
/// Global config is trusted without a record, so writing it decides nothing about a project.
pub fn trust_check_before_write(path: &Path) -> Result<()> {
    if Settings::safe_mode() || config::is_global_config(path) {
        return Ok(());
    }
    let config_root = config_trust_root(path);
    let mut files = if config_root.as_path() == path {
        IndexSet::new()
    } else {
        config::config_files_in_dir_for_any_env(&config_root)
    };
    files.insert(path.to_path_buf());
    for file in files {
        if file.is_file()
            && !is_config_ignored_via_setting(&file)
            && !is_path_trusted(&file)
            && config_requires_trust(&file)
        {
            trust_check(&file)?;
        }
    }
    Ok(())
}

/// Whether a config file, as it is on disk, loads only once trusted.
fn config_requires_trust(path: &Path) -> bool {
    if config::is_tool_versions_file(path)
        || matches!(
            detect_config_file_type_by_filename(path),
            Some(ConfigFileType::ToolVersions)
        )
    {
        ToolVersions::path_requires_trust(path)
    } else {
        !MiseToml::path_is_trust_exempt(path)
    }
}

/// Synchronizes the derived monorepo marker used to trust descendant configs.
///
/// The marker outlives the run, so it is only written for a root that is
/// trusted: a root that merely loaded is not, since safe mode loads config
/// nobody trusted. CI trusts everything and records nothing, so it leaves the
/// marker alone.
pub(crate) fn set_monorepo_root_marker(path: &Path, enabled: bool) -> Result<()> {
    if *mise_util::env::IS_CI {
        return Ok(());
    }
    let config_root = config_trust_root(path);
    let enabled = enabled && is_trusted(&config_root);
    // The walk in `trusted_monorepo_ancestor` reads the marker under the current name only, also
    // for a root whose record has an older one.
    let hashed_path = dirs::TRUSTED_CONFIGS.join(hashed_path_filename(&config_root));
    let monorepo_marker = with_appended_extension(&hashed_path, "monorepo");
    if enabled && !monorepo_marker.exists() {
        file::create_dir_all(monorepo_marker.parent().unwrap())?;
        file::write(&monorepo_marker, "")?;
    } else if !enabled && monorepo_marker.exists() {
        file::remove_file(&monorepo_marker)?;
        if let Ok(config_root) = config_root.canonicalize() {
            IS_TRUSTED
                .lock()
                .unwrap()
                .retain(|path| path == &config_root || !path.starts_with(&config_root));
        }
    }
    Ok(())
}

pub fn untrust(path: &Path) -> eyre::Result<()> {
    rm_ignored(path.to_path_buf())?;
    // The current name also covers sidecars left without a record, such as the monorepo marker
    // of a root trusted through `trusted_config_paths`.
    let current = dirs::TRUSTED_CONFIGS.join(hashed_path_filename(path));
    let mut records = records_for(&dirs::TRUSTED_CONFIGS, path);
    if !records.contains(&current) {
        records.push(current);
    }
    let identity = record_identity(path);
    for hashed_path in records {
        if record_points_at(&hashed_path, &identity) {
            file::remove_file(&hashed_path)?;
        }
        for ext in ["hash", "monorepo", ENV_FILE_APPROVAL] {
            let sidecar = with_appended_extension(&hashed_path, ext);
            if sidecar.exists() {
                file::remove_file(&sidecar)?;
            }
        }
    }
    Ok(())
}

/// The trust record for `path`, e.g. `~/.local/state/mise/trusted-configs/dir-file-<sha256>`.
fn trust_path(path: &Path) -> PathBuf {
    record_path(&dirs::TRUSTED_CONFIGS, path)
}

fn ignore_path(path: &Path) -> PathBuf {
    record_path(&dirs::IGNORED_CONFIGS, path)
}

/// Whether `path` has a trust record of its own: one whose stored target is `path`. A record
/// name is derived from the path, so a file of that name proves nothing until its target is
/// compared.
#[cfg(test)]
fn has_trust_record(path: &Path) -> bool {
    TrustRecord::find(&record_identity(path)).owned
}

/// The file in `dir` that holds the record for `path`: the current name, or an older name whose
/// record points at `path`. `.hash` and `.monorepo` sidecars sit beside the record it returns.
fn record_path(dir: &Path, path: &Path) -> PathBuf {
    RecordLookup::in_dir(dir, &record_identity(path)).path
}

/// Where the record for a path is and whether it is that path's own, looked up once.
struct RecordLookup {
    path: PathBuf,
    /// Whether the record stores the path it is looked up for as its target. A record name is
    /// derived from the path, so a file of that name proves nothing until its target is compared.
    owned: bool,
}

impl RecordLookup {
    /// The record in `dir` for `identity`, a path already resolved by [`record_identity`].
    fn in_dir(dir: &Path, identity: &Path) -> Self {
        let current = dir.join(record_name(identity));
        if let Some(target) = record_target(&current) {
            let owned = target == identity;
            return Self {
                path: current,
                owned,
            };
        }
        match legacy_records(dir, identity).into_iter().next() {
            Some(path) => Self { path, owned: true },
            None => Self {
                path: current,
                owned: false,
            },
        }
    }
}

/// A path's trust record, with the sidecars a trust check reads.
type TrustRecord = RecordLookup;

impl TrustRecord {
    /// The trust record of `identity`, a path already resolved by [`record_identity`].
    fn find(identity: &Path) -> Self {
        Self::in_dir(&dirs::TRUSTED_CONFIGS, identity)
    }

    fn sidecar(&self, ext: &str) -> PathBuf {
        with_appended_extension(&self.path, ext)
    }

    /// Whether the record is bound to the content of a file.
    fn has_hash(&self) -> bool {
        self.owned && self.sidecar("hash").exists()
    }

    /// Whether the record approves its file as an env file only. [`trust`] writes that marker
    /// only together with a hash, so a record without one is not looked at further.
    fn is_env_file_approval(&self) -> bool {
        self.has_hash() && self.sidecar(ENV_FILE_APPROVAL).exists()
    }

    /// Whether the record has a hash and `path` still has the content it approved.
    fn matches_content(&self, path: &Path) -> eyre::Result<bool> {
        if !self.owned {
            return Ok(false);
        }
        let hash = match std::fs::read_to_string(self.sidecar("hash")) {
            Ok(hash) => hash,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => Err(err)?,
        };
        Ok(hash == hash::file_hash_sha256(path, None)?)
    }
}

/// Every record in `dir` that points at `path`, under its current name and under the older
/// names.
fn records_for(dir: &Path, path: &Path) -> Vec<PathBuf> {
    let identity = record_identity(path);
    let current = dir.join(hashed_path_filename(path));
    let mut records = vec![];
    if record_points_at(&current, &identity) {
        records.push(current);
    }
    records.extend(legacy_records(dir, &identity));
    records
}

remove_by!("2027.11.0", "legacy-trust-record-names");

/// Records under the names that mise 2026.11.13 and earlier used, which hash the path with
/// `Hash`. That hash skips separators, so different paths can share a name, and a record found
/// under one counts only when it points at `identity`.
fn legacy_records(dir: &Path, identity: &Path) -> Vec<PathBuf> {
    let hash = hash_to_str(&identity);
    [hash_to_str(&hash), prefixed_record_name(identity, hash)]
        .into_iter()
        .map(|name| dir.join(name))
        .filter(|record| record_points_at(record, identity))
        .collect()
}

/// Whether the record file `record` stores `identity` as its target.
fn record_points_at(record: &Path, identity: &Path) -> bool {
    record_target(record).is_some_and(|target| target == identity)
}

/// The path the record file `record` stores, or `None` when there is no such file. Records are
/// symlinks, read with one call; only something else at that name, such as the plain file holding
/// the path that Windows gets, is read the slower way.
fn record_target(record: &Path) -> Option<PathBuf> {
    match std::fs::read_link(record) {
        Ok(target) => Some(target),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => file::resolve_symlink(record).ok().flatten(),
    }
}

/// The path a record is named for and points at.
fn record_identity(path: &Path) -> PathBuf {
    // canonicalize needs access to every ancestor, which a sandbox can deny (e.g. Codex
    // "workspace write" on Windows), so fall back to the absolute path instead of panicking.
    path.canonicalize()
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Appends an extension to a path without replacing existing dots in the filename.
/// Unlike `Path::with_extension`, this preserves the full filename.
/// e.g. "foo-bar.toml-abc123" + "hash" → "foo-bar.toml-abc123.hash"
///
/// NOTE: This changes the filename convention for .hash and .monorepo files.
/// Existing files from prior versions will not be found, requiring a one-time
/// re-trust of previously trusted configs after upgrade.
pub fn with_appended_extension(path: &Path, ext: &str) -> PathBuf {
    let mut os_string = path.as_os_str().to_owned();
    os_string.push(".");
    os_string.push(ext);
    PathBuf::from(os_string)
}

/// The file name of the trust and ignore records for `path`: the parent and file names, for
/// people reading the directory, then the SHA-256 of the whole path's bytes, which tells apart
/// paths that the readable part does not.
fn hashed_path_filename(path: &Path) -> String {
    record_name(&record_identity(path))
}

/// [`hashed_path_filename`] for a path already resolved by [`record_identity`], which needs no
/// filesystem call.
fn record_name(identity: &Path) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(identity.as_os_str().as_encoded_bytes());
    let hash = digest.iter().map(|b| format!("{b:02x}")).collect();
    prefixed_record_name(identity, hash)
}

fn prefixed_record_name(identity: &Path, hash: String) -> String {
    let trunc_str = |s: &OsStr| s.to_string_lossy().chars().take(20).collect::<String>();
    let parent = identity.parent().and_then(|p| p.file_name()).map(trunc_str);
    let filename = identity.file_name().map(trunc_str);
    [parent, filename, Some(hash)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("-")
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

/// Whether the filename alone marks the file as mise TOML configuration,
/// including names chosen with `MISE_OVERRIDE_CONFIG_FILENAMES`.
pub fn is_mise_toml_filename(path: &Path) -> bool {
    matches!(
        detect_config_file_type_by_filename(path),
        Some(ConfigFileType::MiseToml)
    )
}

/// Whether config loading skips `path` because it is a version file linked to
/// a file of another name, see [`diagnostic::links_to_another_name`]. A
/// version file is a file found under one of `idiomatic_filenames`, or a
/// tool-versions file by name, as loading decides it. Discovery finds everything
/// else under a mise TOML filename, which that rule does not cover.
pub(crate) fn is_skipped_version_file_link(path: &Path, idiomatic_filenames: &[&str]) -> bool {
    // Idiomatic filenames first: `parse_config_file` hands a path that matches
    // one to the idiomatic parser even when a mise TOML filename matches too.
    let is_version_file = idiomatic_filenames.iter().any(|f| path.ends_with(f))
        || matches!(
            detect_config_file_type_by_filename(path),
            Some(ConfigFileType::ToolVersions)
        );
    is_version_file && diagnostic::links_to_another_name(path)
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

pub use mise_config::task::TaskConfig;

pub use mise_config::tool_config::ToolConfig;

/// Turn the deprecated spellings mise-config's parsers accept into mise's warnings, with the
/// ids, versions and messages mise has always used.
pub(crate) fn register_deprecation_handler() {
    mise_config::deprecation::set_handler(|deprecation| {
        use mise_config::deprecation::Deprecation;
        match deprecation {
            Deprecation::EnvMise => deprecated_at!(
                "2026.7.0",
                "2026.12.0",
                "config.env.mise",
                "`env.mise` is deprecated. Use `env._` instead."
            ),
            Deprecation::DirectiveValue => deprecated_at!(
                "2026.7.0",
                "2026.12.0",
                "config.directive.value",
                "`value` in built-in `file`, `path`, and `source` directive objects is deprecated. Use `path` instead."
            ),
            Deprecation::DirectiveValues => deprecated_at!(
                "2026.7.0",
                "2026.12.0",
                "config.directive.values",
                "`values` in built-in `file`, `path`, and `source` directive objects is deprecated. Use `path` instead."
            ),
            Deprecation::ToolVersionAliases => deprecated!(
                "TOOL_VERSION_ALIASES",
                "tool version aliases should be `alias.<TOOL>.versions.<FROM> = <TO>`, not `alias.<TOOL>.<FROM> = <TO>`"
            ),
        }
    });
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
            .into_tools()
            .collect::<Vec<_>>();
        let (_, versions, _) = tools
            .iter()
            .find(|(backend, _, _)| backend.short == "goreleaser")
            .expect("goreleaser should be parsed from its nested idiomatic path");

        assert_eq!(versions[0].version(), "2");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn skipped_version_file_link_is_told_apart_from_mise_toml() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let tool_versions = dir.join(env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
        std::fs::write(dir.join(".foo-version"), "1\n").unwrap();
        std::fs::write(dir.join("mise.toml"), "").unwrap();
        // A plugin may declare an idiomatic filename that ends in `.toml`.
        symlink(dir.join(".foo-version"), dir.join("foo.toml")).unwrap();
        symlink(dir.join(".foo-version"), &tool_versions).unwrap();
        symlink(dir.join("mise.toml"), dir.join(".mise.toml")).unwrap();

        assert!(is_skipped_version_file_link(
            &dir.join("foo.toml"),
            &["foo.toml"]
        ));
        assert!(!is_skipped_version_file_link(&dir.join("foo.toml"), &[]));
        assert!(is_skipped_version_file_link(&tool_versions, &[]));
        assert!(!is_skipped_version_file_link(
            &dir.join(".mise.toml"),
            &["foo.toml"]
        ));
        assert!(!is_skipped_version_file_link(
            &dir.join(".foo-version"),
            &[".foo-version"]
        ));
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

#[cfg(test)]
mod env_file_trust_tests {
    use super::*;

    fn paranoid_on(_held: &crate::test::SettingsGuard) {
        use confique::Layer;
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.paranoid = Some(mise_settings::Paranoid::all());
        Settings::reset(Some(settings));
    }

    fn is_untrusted(path: &Path) -> bool {
        matches!(
            env_file_trust_check(path)
                .unwrap_err()
                .downcast_ref::<crate::errors::Error>(),
            Some(UntrustedEnvFile(_))
        )
    }

    /// The first passing check memoizes the path in `IS_TRUSTED`. A long-lived process that reloads
    /// after the file was edited must still compare the content hash, not the memo.
    #[test]
    fn an_edited_file_needs_new_trust_even_after_a_passing_check() {
        let held = crate::test::SettingsGuard::lock();
        paranoid_on(&held);
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".env");
        std::fs::write(&path, "A=1\n").unwrap();

        assert!(is_untrusted(&path));
        trust(&path).unwrap();
        assert!(env_file_trust_check(&path).unwrap());
        assert!(env_file_trust_check(&path).unwrap());

        std::fs::write(&path, "A=2\n").unwrap();
        assert!(is_untrusted(&path));

        trust(&path).unwrap();
        assert!(env_file_trust_check(&path).unwrap());
    }

    #[test]
    fn a_missing_file_is_not_loaded() {
        let held = crate::test::SettingsGuard::lock();
        paranoid_on(&held);
        let tmp = tempfile::tempdir().unwrap();
        assert!(!env_file_trust_check(&tmp.path().join(".env")).unwrap());
    }

    /// A path that cannot be resolved for a reason other than being absent is not shown to be
    /// absent, so it fails instead of loading unchecked.
    #[cfg(unix)]
    #[test]
    fn a_path_that_cannot_be_resolved_fails_closed() {
        use std::os::unix::ffi::OsStrExt;
        let held = crate::test::SettingsGuard::lock();
        paranoid_on(&held);
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/nul\0byte/.env"));
        let err = env_file_trust_check(path).unwrap_err();
        assert!(err.to_string().contains("cannot check paranoid-mode trust"));
    }

    /// Only `env_files` makes env file trust matter, and with it on alone the trust is still bound
    /// to content.
    #[test]
    fn only_the_env_files_category_gates_env_files() {
        use confique::Layer;
        use mise_settings::{Paranoid, ParanoidCategory};
        let _held = crate::test::SettingsGuard::lock();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".env");
        std::fs::write(&path, "A=1\n").unwrap();
        let select = |paranoid: Paranoid| {
            let mut settings = crate::config::settings::SettingsPartial::empty();
            settings.paranoid = Some(paranoid);
            Settings::reset(Some(settings));
        };
        for category in [
            ParanoidCategory::Config,
            ParanoidCategory::Plugins,
            ParanoidCategory::Provenance,
        ] {
            select(Paranoid::only([category]));
            assert!(env_file_trust_check(&path).unwrap(), "{category}");
        }
        select(Paranoid::only([ParanoidCategory::EnvFiles]));
        assert!(is_untrusted(&path));
        trust(&path).unwrap();
        assert!(env_file_trust_check(&path).unwrap());
        std::fs::write(&path, "A=2\n").unwrap();
        assert!(is_untrusted(&path));
    }

    /// With only `env_files` on, approving an env file is not trust in the same path as config,
    /// in this run or a later one, and trusting that path the normal way replaces the approval.
    #[test]
    fn an_approved_env_file_is_not_config_trust() {
        use confique::Layer;
        use mise_settings::{Paranoid, ParanoidCategory};
        let _held = crate::test::SettingsGuard::lock();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.paranoid = Some(Paranoid::only([ParanoidCategory::EnvFiles]));
        Settings::reset(Some(settings));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".env");
        std::fs::write(&path, "A=1\n").unwrap();

        trust(&path).unwrap();
        assert!(env_file_trust_check(&path).unwrap());
        assert!(!is_trusted(&path));
        Settings::reset(None);
        assert!(!is_trusted(&path));

        // A config named by its file name is approved as config.
        let config = tmp.path().join("mise.toml");
        std::fs::write(&config, "").unwrap();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.paranoid = Some(Paranoid::only([ParanoidCategory::EnvFiles]));
        Settings::reset(Some(settings));
        trust(&config).unwrap();
        assert!(!is_env_file_approval(&config));

        // A directory that replaced the file is trusted by path once the user trusts it.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        trust(&path).unwrap();
        assert!(!is_env_file_approval(&path));
        assert!(!has_trust_hash(&path));
        untrust(&path).unwrap();
        untrust(&config).unwrap();
    }

    /// With both categories on, an env file `mise trust` approves for an env directive is still
    /// not trust in the same path as config.
    #[test]
    fn an_env_file_approval_is_not_config_trust_in_full_paranoid_mode() {
        use confique::Layer;
        use mise_settings::Paranoid;
        let _held = crate::test::SettingsGuard::lock();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.paranoid = Some(Paranoid::all());
        Settings::reset(Some(settings));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("env.toml");
        std::fs::write(&path, "A = 1\n").unwrap();

        trust_env_file(&path).unwrap();
        assert!(env_file_trust_check(&path).unwrap());
        assert!(!is_trusted(&path));

        // Trusting the path as config replaces the approval.
        trust(&path).unwrap();
        assert!(is_trusted(&path));
        untrust(&path).unwrap();
        Settings::reset(None);
    }

    #[test]
    fn outside_paranoid_mode_every_file_loads() {
        let _held = crate::test::SettingsGuard::lock();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".env");
        std::fs::write(&path, "A=1\n").unwrap();
        assert!(env_file_trust_check(&path).unwrap());
    }
}

#[cfg(test)]
mod hashed_path_filename_tests {
    use super::*;

    #[test]
    fn survives_uncanonicalizable_path() {
        // canonicalize fails for a missing path, as it does when a sandbox denies access
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope").join("mise.toml");
        assert!(missing.canonicalize().is_err());
        let name = hashed_path_filename(&missing);
        assert!(name.starts_with("nope-mise.toml-"), "{name}");
        assert_eq!(name, hashed_path_filename(&missing));

        // a relative path must hash like its absolute form, not as written
        let relative = Path::new("mise-nonexistent-dir").join("mise.toml");
        assert!(relative.canonicalize().is_err());
        let absolute = std::path::absolute(&relative).unwrap();
        assert_eq!(
            hashed_path_filename(&relative),
            hashed_path_filename(&absolute)
        );
    }

    /// Two env files whose paths differ only in where the separators are.
    fn files_that_differ_in_separators() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("platform/deploy/staging/api/.env");
        let b = tmp.path().join("plat/formdeploys/taging/api/.env");
        for path in [&a, &b] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "A=1\n").unwrap();
        }
        // `Path`'s `Hash` skips separators, so these share the name older versions gave a record.
        let legacy_name = |p: &Path| {
            let identity = record_identity(p);
            prefixed_record_name(&identity, hash_to_str(&identity))
        };
        assert_eq!(legacy_name(&a), legacy_name(&b));
        (tmp, a, b)
    }

    #[test]
    fn a_record_of_another_path_is_not_trust() {
        let (_tmp, a, b) = files_that_differ_in_separators();
        assert_ne!(hashed_path_filename(&a), hashed_path_filename(&b));
        trust(&a).unwrap();
        assert!(has_trust_record(&a));
        assert!(!has_trust_record(&b));
        untrust(&a).unwrap();
        assert!(!has_trust_record(&a));
    }

    #[test]
    fn a_record_under_the_old_name_counts_only_for_its_target() {
        let (_tmp, a, b) = files_that_differ_in_separators();
        let identity = record_identity(&a);
        let legacy =
            dirs::TRUSTED_CONFIGS.join(prefixed_record_name(&identity, hash_to_str(&identity)));
        file::create_dir_all(&*dirs::TRUSTED_CONFIGS).unwrap();
        file::make_symlink_or_file(&identity, &legacy).unwrap();
        assert!(has_trust_record(&a));
        assert!(!has_trust_record(&b));
        // Re-trusting the other path leaves the old record alone and writes its own.
        trust(&b).unwrap();
        assert!(has_trust_record(&a));
        assert!(has_trust_record(&b));
        untrust(&a).unwrap();
        assert!(!legacy.exists());
        assert!(!has_trust_record(&a));
        assert!(has_trust_record(&b));
        untrust(&b).unwrap();
    }

    #[test]
    fn trusting_a_path_keeps_the_ignore_of_another() {
        let (_tmp, a, b) = files_that_differ_in_separators();
        let identity = record_identity(&b);
        let legacy =
            dirs::IGNORED_CONFIGS.join(prefixed_record_name(&identity, hash_to_str(&identity)));
        file::create_dir_all(&*dirs::IGNORED_CONFIGS).unwrap();
        file::make_symlink_or_file(&identity, &legacy).unwrap();
        trust(&a).unwrap();
        assert_eq!(file::resolve_symlink(&legacy).unwrap(), Some(identity));
        rm_ignored(b.clone()).unwrap();
        assert!(!legacy.exists());
        untrust(&a).unwrap();
    }

    #[test]
    fn an_ignored_subproject_stays_ignored_under_a_trusted_root() {
        let tmp = tempfile::tempdir().unwrap();
        let mono = tmp.path().canonicalize().unwrap().join("mono");
        let sub = mono.join("apps/sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(mono.join("mise.toml"), "monorepo_root = true\n").unwrap();
        std::fs::write(sub.join("mise.toml"), "").unwrap();
        let config = sub.join("mise.toml");
        trust(&mono).unwrap();
        file::write(with_appended_extension(&trust_path(&mono), "monorepo"), "").unwrap();
        add_ignored(sub.clone()).unwrap();

        // The ignore of the subproject's root covers its config file, ahead of the trusted
        // monorepo root above it.
        assert!(!is_trusted_with(&config, false));
        assert!(!is_path_trusted(&config));
        // A command that implies trust records none for it, so the ignore is not removed.
        assert!(!implicit_trust_may_record(
            &config,
            &config_trust_root(&config)
        ));

        rm_ignored(sub).unwrap();
        untrust(&mono).unwrap();
    }
}
