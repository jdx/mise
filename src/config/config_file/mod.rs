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
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use idiomatic_version::IdiomaticVersionFile;
use indexmap::{IndexMap, IndexSet};
use path_absolutize::Absolutize;
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

pub use typed_consent::{CommandOrigin, set_command_origin};

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

/// Starts a new, empty config for `path` without reading it. Nothing is written until the
/// config is saved, so a preview of a new config uses this even where `path` is a dangling
/// link that [`parse_or_init`] refuses.
pub async fn init(path: &Path) -> Result<Arc<dyn ConfigFile>> {
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
    // A dangling link is not a missing config: starting a new one would create the file the
    // link names, wherever that is. The write checks where a link leads before it follows one.
    if !path.exists() && path.is_symlink() {
        symlink_check_before_write(&path)?;
    }
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
///
/// A `ref:` or `rev:` version (or `ref-`/`rev-`, which `ToolRequest` reads the same way) names
/// any commit the tool's repository serves, and forges serve commits nobody published there:
/// a pull request's head (`refs/pull/1/head`) or, on GitHub, a fork's commit by its id. A
/// shim or the not-found handler builds such a version from source when another version of
/// the tool is installed. `tag:` and `branch:` only name what the repository's maintainers
/// pushed, as a plain version names what they released.
pub fn version_requires_trust(version: &str) -> bool {
    is_path_version(version) || matches!(version.split_once([':', '-']), Some(("ref" | "rev", _)))
}

/// Whether a tool version is a `path:` version, which safe mode ignores in project config.
pub(crate) fn is_path_version(version: &str) -> bool {
    version.starts_with("path:")
}

/// Whether a `[tools]` key only takes effect once the config is trusted.
///
/// Inline options (`tool[postinstall=...]`) can run code or redirect a backend.
/// An `spm:` package named by the URL of a self-hosted forge makes mise derive
/// that host's API from the name and send it the user's forge token, as an
/// `api_url` option would.
///
/// The key is checked as `BackendArg` reads it, after `unalias_backend` drops
/// leading `core:` prefixes, so `core:spm:<url>` is the same `spm:` key.
pub fn tool_key_requires_trust(tool: &str) -> bool {
    let tool = crate::backend::unalias_backend(tool.trim());
    tool.contains('[') || crate::backend::spm::tool_key_selects_api_host(&tool)
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

/// The project directory `path` belongs to, whatever paranoid mode makes its trust root: the
/// project a link inside it leads from, or that holds the nested root it is in, else its config
/// root.
pub fn config_project(path: &Path) -> PathBuf {
    config_root::linked_project(path)
        .or_else(|| config_root::nested_root_project(path))
        .unwrap_or_else(|| config_root::config_root(path))
}

/// The paths of the project at `project` with a trust record that approves them as config,
/// whether or not they are still on disk. In paranoid mode each config is its own trust root, so
/// these are the files the user approved there, a config that another branch brings back
/// included. Env file approvals are not trust in a config and are left out.
pub fn trusted_configs_of_project(project: &Path) -> Vec<PathBuf> {
    let project = record_identity(project);
    trusted_paths_below(&project)
        .into_iter()
        .filter(|path| {
            config_project(path) == project && !TrustRecord::find(path).is_env_file_approval()
        })
        .collect()
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
///
/// A nested root of an ignored project is ignored with it, as config discovery leaves it out
/// ([`project_is_ignored`]), so the trust its own directory has, from a trusted monorepo root
/// above it or from CI, does not count. `trusted_config_paths` still does.
pub fn is_path_trusted(path: &Path) -> bool {
    if let Some(project) = config_root::linked_project(path) {
        return is_trusted(&project);
    }
    if project_is_ignored(path) && !is_trusted_via_config_paths(path) {
        return false;
    }
    is_trusted(&config_trust_root(path)) || is_trusted(path)
}

/// Whether mise may put a directory that lives in the project at `dir` first on PATH, such as
/// the `bin` of a virtualenv there. Its files then run in place of system commands, which is
/// what `_.python.venv` and `_.path` in that project's own config need trust for.
///
/// True when a loaded config file rooted at `dir` is trusted, or, outside paranoid mode, when
/// `dir` itself is (`mise trust <dir>`). A trust record of a parent directory does not count: a
/// clone below a trusted project is not that project. What already trusts every config file
/// below a directory, a `trusted_config_paths` entry or a trusted monorepo root, does count,
/// through [`is_trusted`]: the project's own config could then put its files on PATH anyway.
/// A project file linked to global config does not lend it global config's trust. Safe mode
/// trusts no project, so this is false there.
pub fn is_project_dir_trusted(config: &Config, dir: &Path) -> bool {
    if Settings::safe_mode() {
        return false;
    }
    let trusted_config_here = config.config_files.iter().any(|(path, cf)| {
        cf.project_root().as_deref() == Some(dir)
            && !config::is_project_link_to_global_config(path)
            && is_path_trusted(path)
    });
    trusted_config_here || (!Settings::get().paranoid.config() && is_trusted(dir))
}

/// What to run so that [`is_project_dir_trusted`] accepts `dir`, for a warning that names it.
pub fn project_dir_trust_hint(dir: &Path) -> String {
    let dir = display_path(dir);
    if Settings::get().paranoid.config() {
        format!("trust a mise config file in {dir} with `mise trust <file>`")
    } else {
        format!("run `mise trust {dir}`")
    }
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
            granted_by_this_command(&config_root);
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
    let was_trusted = is_trusted(config_root);
    trust(config_root)?;
    if !was_trusted {
        granted_by_this_command(config_root);
    }
    if !was_trusted && settings::is_loaded() {
        Settings::reload()?;
    }
    Ok(())
}

/// The config roots this process trusted that were not trusted when it started, resolved.
static GRANTED_BY_THIS_COMMAND: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

fn resolved_root(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Notes that this process trusted `config_root`. Every way a command grants trust on its own
/// comes through here: a command that implies trust, one typed on a terminal, a yes to the
/// prompt, and the `yes` setting.
fn granted_by_this_command(config_root: &Path) {
    GRANTED_BY_THIS_COMMAND
        .lock()
        .unwrap()
        .get_or_insert_with(HashSet::new)
        .insert(resolved_root(config_root));
}

/// Whether this process trusted `config_root`, rather than finding it trusted when it started.
/// A root below a monorepo root that this process trusted counts too: the monorepo root's
/// marker trusts it, so it needs no record of its own, and its trust came from this command
/// just the same. Such a root counts even when it was also trusted on its own before, as the
/// monorepo root's trust would hide that.
fn trusted_by_this_command(config_root: &Path) -> bool {
    let granted = GRANTED_BY_THIS_COMMAND.lock().unwrap();
    let Some(roots) = granted.as_ref() else {
        return false;
    };
    let root = resolved_root(config_root);
    roots.contains(&root)
        || root
            .ancestors()
            .skip(1)
            .any(|dir| roots.contains(dir) && has_monorepo_marker(dir))
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
pub fn project_is_ignored(path: &Path) -> bool {
    config_root::nested_root_project(path).is_some_and(|project| is_ignored(&project))
}

/// [`trust_and_reload_settings`] that takes the record back when the reload fails on the
/// project's own `[settings]`. Every later command would fail on that setting at startup,
/// `mise untrust` too, so a record left behind could not be removed with mise.
fn trust_and_reload_settings_or_roll_back(config_root: &Path) -> eyre::Result<()> {
    let was_trusted = is_trusted(config_root);
    trust(config_root)?;
    if !was_trusted {
        granted_by_this_command(config_root);
    }
    if !was_trusted
        && settings::is_loaded()
        && let Err(err) = Settings::reload()
    {
        untrust(config_root)?;
        if let Ok(canonical) = config_root.canonicalize() {
            IS_TRUSTED.lock().unwrap().remove(&canonical);
        }
        return Err(err);
    }
    Ok(())
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
    // Global config is trusted without a record; its root is the home directory,
    // so a record would trust a project in `~` and clear an ignore for it.
    if implicitly_trusts_active_config()
        && !config::is_global_config(path)
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
    // The prompt hook neither asks nor trusts. The parsed command catches `mise -q hook-env`;
    // argv[1] covers config loaded before the command line is parsed.
    let prompt_hook = cmd == "hook-env" || typed_consent::is_prompt_hook();
    if !prompt_hook && !strict && !is_ignored(&config_root) && !is_ignored(path) {
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
        debug!(
            "safe mode: not loading env file {}",
            style::display_path_escaped(path)
        );
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
    // Trust that depends on the file's content is checked again next time: the file can change
    // within one process, and the memo would keep accepting it.
    if trusted
        && kind == TrustAs::Config
        && !env_file_approval
        && !content_bound
        && !record.has_hash()
    {
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
///
/// The search starts above the root `canonical` belongs to: its parent when it is a directory, a
/// project trusted as one, and its grandparent when it is a file, which lies in its root.
fn trusted_monorepo_ancestor(canonical: &Path) -> bool {
    let own_levels = if canonical.is_dir() { 1 } else { 2 };
    canonical
        .ancestors()
        .skip(own_levels)
        .any(|dir| has_monorepo_marker(dir) && is_trusted_with(dir, false))
}

/// Whether `dir`, resolved, is marked as a monorepo root, under the current record name or an
/// older one.
fn has_monorepo_marker(dir: &Path) -> bool {
    let record = dirs::TRUSTED_CONFIGS.join(record_name(dir));
    with_appended_extension(&record, "monorepo").exists() || has_legacy_monorepo_marker(dir)
}

remove_by!("2027.11.0", "legacy-trust-record-names");

/// Whether `dir` has a monorepo marker beside a record under a name that mise 2026.11.14 and
/// earlier used. Loading the root's config writes the marker under the current name, but settings
/// are read before that, so the first run after an upgrade would otherwise leave out the
/// `[settings]` of a subproject trusted through the root.
fn has_legacy_monorepo_marker(dir: &Path) -> bool {
    legacy_records(&dirs::TRUSTED_CONFIGS, dir)
        .iter()
        .any(|record| with_appended_extension(record, "monorepo").exists())
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
    // A path that is gone, such as a nested root `mise untrust` found by its record, still has
    // its records removed: they hold it as it was resolved.
    let path = record_identity(&path);
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

/// The longest glob [`compile_path_glob`] accepts, in bytes: what a path can be
/// on Linux. The regex globset generates takes memory in proportion to the
/// pattern before the size limit of the compiled regex can reject it.
const GLOB_MAX_LEN: usize = 4096;
/// The most `{` it accepts. globset recurses once per nested `{…}` while it
/// builds a glob, so a deeply nested pattern overflows the stack. Every `{` is
/// counted rather than the nesting itself: a `}` that is escaped or inside a
/// character class closes nothing, and a depth counter that missed one would
/// let the nesting through.
const GLOB_MAX_BRACES: usize = 64;

/// Compiles a path glob that can come from a file read without trust: a
/// project `.miserc.toml` sets `ignored_config_paths` and the override
/// filenames, which become the config filename patterns.
///
/// `Glob::compile_matcher` panics when the regex it generates is rejected, for
/// nesting too deep or growing too large. `GlobSetBuilder::build` returns that
/// as an error instead.
pub(crate) fn compile_path_glob(pattern: &str) -> Result<GlobSet> {
    if pattern.len() > GLOB_MAX_LEN {
        return Err(eyre!("the pattern is longer than {GLOB_MAX_LEN} bytes"));
    }
    if pattern.matches('{').count() > GLOB_MAX_BRACES {
        return Err(eyre!("more than {GLOB_MAX_BRACES} `{{` in the pattern"));
    }
    let glob = GlobBuilder::new(pattern).literal_separator(true).build()?;
    Ok(GlobSetBuilder::new().add(glob).build()?)
}

#[derive(Default)]
struct IgnoredConfigPathMatcher {
    literals: Vec<PathBuf>,
    globs: Vec<GlobSet>,
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
            match compile_path_glob(&pattern) {
                Ok(glob) => matcher.globs.push(glob),
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
    let canonicalized_path = path.canonicalize()?;
    // Written under the current name: a record under an older name does not say what it approves
    // (see `TrustRecord::is_env_file_approval`), so it is replaced below.
    let hashed_path = dirs::TRUSTED_CONFIGS.join(record_name(&canonicalized_path));
    // What this writes decides the next check, which the memo would answer from the old state.
    IS_TRUSTED.lock().unwrap().remove(&canonicalized_path);
    file::create_dir_all(hashed_path.parent().unwrap())?;
    // The record is written last, after the sidecars that say what it approves: a write that
    // fails part way leaves no new record, rather than one without its env file marker that
    // would count as trust in the path as config.
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
    if !record_points_at(&hashed_path, &canonicalized_path) {
        file::make_symlink_or_file(&canonicalized_path, &hashed_path)?;
    }
    remove_by!("2027.11.0", "legacy-trust-record-names");
    for legacy in legacy_records(&dirs::TRUSTED_CONFIGS, &canonicalized_path) {
        // The monorepo marker moves to the new record: settings are read before the root's
        // config loads and writes it again.
        let marker = with_appended_extension(&legacy, "monorepo");
        if marker.exists() {
            file::write(with_appended_extension(&hashed_path, "monorepo"), "")?;
        }
        for ext in ["hash", "monorepo", ENV_FILE_APPROVAL] {
            let sidecar = with_appended_extension(&legacy, ext);
            if sidecar.exists() {
                file::remove_file(&sidecar)?;
            }
        }
        file::remove_file(&legacy)?;
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
/// there has none to record and leaves trust as it was. Global config is trusted without a
/// record, and its trust root is the home directory: a record for it would trust a project
/// in `~`, and clear a decision to ignore that project.
///
/// `trusted_before` is [`trusted_before_write`], taken before the write changed the file. Trust
/// the record adds is this command's, so a later write in the same root does not count it as
/// trust from before the command and follow a link out of the project. A record that only renews
/// trust from before, as paranoid mode's content-bound one does, is not.
pub fn trust_after_write(path: &Path, trusted_before: bool) -> Result<()> {
    if Settings::safe_mode() || config::is_global_config_write(path) {
        return Ok(());
    }
    let config_root = config_trust_root(path);
    trust(&config_root)?;
    if !trusted_before {
        granted_by_this_command(&config_root);
    }
    Ok(())
}

/// Whether the trust [`trust_after_write`] records for `path` is already there, asked before the
/// write. It has to be asked then: in paranoid mode the record is the file's own and bound to its
/// content, so once the write changes the file it no longer counts. The file's trust in paranoid
/// mode is also the trust loading goes by, a linked project's `trusted_config_paths` included.
/// Otherwise the record is the config root's, so the root's own trust has to be there already
/// too: a file record does not make the root's new record trust from before. Either way the file
/// has to load as trusted, so a nested root of an ignored project, which its directory's trust
/// does not cover, counts as newly trusted.
pub fn trusted_before_write(path: &Path) -> bool {
    let config_root = config_trust_root(path);
    is_path_trusted(path) && (config_root == path || is_trusted(&config_root))
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
    if Settings::safe_mode() || config::is_global_config_write(path) {
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

/// Refuses to write config file `path` through a symlink to a file the user did not choose.
///
/// The symlink is `path` itself, or a directory between the project and `path`, such as a
/// linked `.config` or `.mise`: the lockfile writer checks the same directories.
///
/// A project's links come with its working tree, so in a checkout nobody reviewed their author
/// would choose which file `mise use` or `mise set` replaces: a file elsewhere in the user's
/// home, or a trust record in mise's state directory. A link that stays in the config root is
/// followed. One that leaves it is followed only for global or system config, which the user
/// links into place themselves (a dotfiles checkout), or for a project outside safe mode that
/// was trusted before this command started. A project's link to global config, a link into the
/// state directory, and a link out of the root whose target does not exist are never followed,
/// so a write never creates a file outside the project.
///
/// A writer that asks a question before it writes, such as a trust prompt, checks again with
/// [`checked_write_destination`] after the answer, as the links can change while it waits.
pub fn symlink_check_before_write(path: &Path) -> Result<()> {
    checked_write_destination(path).map(drop)
}

/// Whether `destination` is in mise's state directory, where trust records live. Neither a config
/// write nor a task change goes there, whatever links lead to it.
fn in_state_dir(destination: &Path) -> bool {
    destination.starts_with(file::desymlink_path(&dirs::STATE))
}

/// [`symlink_check_before_write`], returning the file the check approved: `path` in its
/// directory resolved, or the file its link leads to. Writing there rather than to `path` goes to
/// the destination this check saw even if a link changes afterwards. That narrows the window to
/// the write itself but does not close it: mise opens the file by its path, not through a
/// directory it holds open.
pub fn checked_write_destination(path: &Path) -> Result<PathBuf> {
    // The directory is resolved from the absolute path: a bare file name such as `mise.toml`
    // has an empty parent, which does not resolve. A directory the write is about to create is
    // resolved through the part of it that exists, so a link above it is judged where it leads.
    let abs = path.absolutize()?;
    let dir = abs.parent().map(file::desymlink_path);
    let dir_leads_out = config_root::linked_project(path).is_some_and(|project| {
        let project = project.canonicalize().ok();
        !matches!((&dir, project), (Some(dir), Some(project)) if dir.starts_with(&project))
    });
    // A directory link that stays in the project can still lead to global config, as a home
    // directory's `.mise` linked to `.config/mise` does, so that case is never waved through,
    // and neither is one that would create a global config file that does not exist yet.
    let to_global = || config::write_reaches_global_config_through_link(&abs);
    // When the state directory is inside the project, a directory link that stays in the project
    // can still lead into it, so no destination there is accepted.
    if !path.is_symlink() && !dir_leads_out && !to_global() {
        let destination = match (dir, abs.file_name()) {
            (Some(dir), Some(name)) => dir.join(name),
            _ => abs.to_path_buf(),
        };
        if in_state_dir(&destination) {
            return Err(eyre!(
                "refusing to write {} to {}: it is in mise's state directory. Write the config somewhere else, or replace the linked directory with a regular one",
                style::display_path_escaped(path),
                style::display_path_escaped(&destination)
            ));
        }
        return Ok(destination);
    }
    let target = file::atomic_write_target(path)?;
    let through = if path.is_symlink() {
        "its symlink"
    } else {
        "a linked directory"
    };
    let refuse = |why: &str| -> Result<PathBuf> {
        Err(eyre!(
            "refusing to write {} through {through} to {}: {why}. Write the file it links to directly, or replace the link with a regular file",
            style::display_path_escaped(path),
            style::display_path_escaped(&target)
        ))
    };
    let unresolved = target
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    if unresolved || in_state_dir(&target) {
        return refuse("the target is in mise's state directory or cannot be resolved");
    }
    // Global config's root is the home directory, which holds the project too, so a project's
    // link to global config would pass every check below, and the trust it would need is the
    // home directory's. The project's author chose that link, and `-g` writes the same file.
    if to_global() {
        return refuse(
            "a project's link to global config is not followed. Write global config with `-g`",
        );
    }
    // Below a directory link that leaves the project, the config root and the link's own
    // directory resolve to wherever that link leads, so neither says the target stays.
    let root = config_root::config_root(path).canonicalize().ok();
    let in_root = !dir_leads_out
        && (root.is_some_and(|root| target.starts_with(root))
            || dir.is_some_and(|dir| target.starts_with(dir)));
    if in_root || config::is_global_config_write(path) {
        return Ok(target);
    }
    if !target.exists() {
        return refuse("the link leaves the config's directory and its target does not exist");
    }
    if Settings::safe_mode() {
        return refuse(
            "a link out of the config's directory is only followed for global config, or for a trusted project outside safe mode",
        );
    }
    // Only trust from before the command started covers a link out of the project. Trust the
    // command granted itself, however it was granted, came before anyone looked at where the
    // project's links lead: the prompt names the directory, not the files its links reach.
    let project = config_trust_root(path);
    if is_path_trusted(path) && !trusted_by_this_command(&project) {
        return Ok(target);
    }
    Err(eyre!(
        "refusing to write {} through {through} to {}: the link leaves the project, and mise follows such a link only for a project that was trusted before this command. Check where the link leads, run `mise trust` in {} and run the command again, or replace the link with a regular file",
        style::display_path_escaped(path),
        style::display_path_escaped(&target),
        style::display_path_escaped(&project)
    ))
}

/// Refuses to write task script `script`, found in a task directory of config root `root`,
/// through a link that leaves the root: the script itself, or a directory between the root and
/// the script. See [`script_check_before_remove`] for the rule. Returns the path to write: the
/// script with the links this check approved resolved, so the write goes where it passed.
pub fn script_check_before_write(script: &Path, root: &Path) -> Result<PathBuf> {
    script_link_check(script, root, true)
}

/// Refuses to remove task script `script`, found in a task directory of config root `root`,
/// through a directory link that leaves the root. Removing a script deletes its own link, not
/// what the link leads to, so only the directories are checked. Returns the path to remove: the
/// script in its directory resolved, so the removal goes to the directory this check approved.
///
/// The rule is [`symlink_check_before_write`]'s: a project's links come with its working tree, so
/// a linked `mise-tasks` would let its author choose which file `mise tasks rm` deletes or
/// `mise tasks add --file` writes. A link that stays in the root is followed. One that leaves it
/// is followed only for global or system config's own tasks, which the user links into place
/// themselves, or for a project outside safe mode that was trusted before this command started,
/// and never to create a file. A script outside the root, which an include named, is the
/// config's choice and is left alone.
pub fn script_check_before_remove(script: &Path, root: &Path) -> Result<PathBuf> {
    script_link_check(script, root, false)
}

/// [`script_check_before_write`] when `follow_own_link` is set, else
/// [`script_check_before_remove`]. Returns the path the change goes to.
fn script_link_check(script: &Path, root: &Path, follow_own_link: bool) -> Result<PathBuf> {
    let script = script.absolutize()?;
    let mut root = root.absolutize()?.into_owned();
    // A conf.d folder, and `.config/mise` with a `mise.toml` of its own, are config roots of
    // their own, but they sit in a project's directory, and a link there is the project's. When
    // the root or a directory above it, up to the project, is a link, the project is the
    // boundary, as it is for the root's trust and lockfile and for config writes.
    if let Some(project) = config_root::linked_project(&root.join("mise.toml")) {
        root = project;
    }
    let (Some(dir), Some(name)) = (script.parent(), script.file_name()) else {
        return Ok(script.into_owned());
    };
    let target = if follow_own_link {
        file::atomic_write_target(&script)?
    } else {
        dir.canonicalize()?.join(name)
    };
    // As for config writes: when the state directory is inside the project, a link that stays in
    // the project can still lead to a trust record.
    if in_state_dir(&target) {
        let verb = if follow_own_link { "write" } else { "remove" };
        return Err(eyre!(
            "refusing to {verb} task {} at {}: it is in mise's state directory. Replace the linked directory with a regular one",
            style::display_path_escaped(&script),
            style::display_path_escaped(&target)
        ));
    }
    let Ok(below_root) = script.strip_prefix(&root) else {
        return Ok(target);
    };
    if [&*dirs::CONFIG, &*dirs::SYSTEM_CONFIG]
        .into_iter()
        .any(|config_dir| config_dir.absolutize().is_ok_and(|d| script.starts_with(d)))
    {
        return Ok(target);
    }
    let own_link = follow_own_link && script.is_symlink();
    let dir_link = below_root
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .filter(|relative| !relative.as_os_str().is_empty())
        .any(|relative| config_root::is_link(&root.join(relative)));
    if !own_link && !dir_link {
        return Ok(target);
    }
    let (verb, through) = match (follow_own_link, own_link) {
        (false, _) => ("remove", "a linked directory"),
        (true, true) => ("write", "its symlink"),
        (true, false) => ("write", "a linked directory"),
    };
    let refuse = |why: &str| -> Result<PathBuf> {
        Err(eyre!(
            "refusing to {verb} task {} through {through} to {}: {why}",
            style::display_path_escaped(&script),
            style::display_path_escaped(&target)
        ))
    };
    if root
        .canonicalize()
        .is_ok_and(|root| target.starts_with(root))
    {
        return Ok(target);
    }
    if target.symlink_metadata().is_err() {
        return refuse(
            "the link leaves the project and its target does not exist. Replace the link with a regular directory",
        );
    }
    if Settings::safe_mode() {
        return refuse(
            "a link out of the project is only followed for global config, or for a trusted project outside safe mode",
        );
    }
    if is_trusted(&root) && !trusted_by_this_command(&root) {
        return Ok(target);
    }
    refuse(&format!(
        "the link leaves the project, and mise follows such a link only for a project that was trusted before this command. Check where the link leads, run `mise trust` in {} and run the command again, or replace the link",
        style::display_path_escaped(&root)
    ))
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
    // The marker is written under the current name, also for a root whose record has an older
    // one; `trusted_monorepo_ancestor` reads an older name's marker only until this runs.
    let hashed_path = dirs::TRUSTED_CONFIGS.join(hashed_path_filename(&config_root));
    let monorepo_marker = with_appended_extension(&hashed_path, "monorepo");
    if enabled && !monorepo_marker.exists() {
        file::create_dir_all(monorepo_marker.parent().unwrap())?;
        file::write(&monorepo_marker, "")?;
    } else if !enabled
        && remove_monorepo_markers(&config_root, &monorepo_marker)?
        && let Ok(config_root) = config_root.canonicalize()
    {
        IS_TRUSTED
            .lock()
            .unwrap()
            .retain(|path| path == &config_root || !path.starts_with(&config_root));
    }
    Ok(())
}

/// Removes `marker`, the current-name monorepo marker of `config_root`, and any marker beside one
/// of its records under an older name, which [`trusted_monorepo_ancestor`] still reads. Returns
/// whether one was removed.
fn remove_monorepo_markers(config_root: &Path, marker: &Path) -> Result<bool> {
    remove_by!("2027.11.0", "legacy-trust-record-names");
    let legacy = legacy_records(&dirs::TRUSTED_CONFIGS, &record_identity(config_root))
        .into_iter()
        .map(|record| with_appended_extension(&record, "monorepo"));
    let mut removed = false;
    for marker in std::iter::once(marker.to_path_buf()).chain(legacy) {
        if marker.exists() {
            file::remove_file(&marker)?;
            removed = true;
        }
    }
    Ok(removed)
}

/// The paths strictly below `dir` that have a trust record, whether or not they are still on
/// disk: a config root that the current checkout lacks keeps its record, and gets it back when
/// another branch brings its config back.
pub fn trusted_paths_below(dir: &Path) -> Vec<PathBuf> {
    let dir = record_identity(dir);
    file::ls(&dirs::TRUSTED_CONFIGS)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|record| record_target(&record))
        .filter(|target| target != &dir && target.starts_with(&dir))
        .collect()
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
#[cfg(test)]
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
    /// Whether the record has a name that mise 2026.11.14 and earlier used.
    legacy: bool,
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
                legacy: false,
            };
        }
        match legacy_records(dir, identity).into_iter().next() {
            Some(path) => Self {
                path,
                owned: true,
                legacy: true,
            },
            None => Self {
                path: current,
                owned: false,
                legacy: false,
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
    ///
    /// mise 2026.11.14 and earlier wrote no marker, and recorded an env file approval exactly as
    /// it recorded paranoid trust in a config file: a record with a hash. Such a record under an
    /// old name does not say which one the user approved, so it counts only as the narrower one,
    /// an env file approval. A config trusted that way is asked about again once, and trusting
    /// it moves the record to the current name.
    fn is_env_file_approval(&self) -> bool {
        remove_by!("2027.11.0", "legacy-trust-record-names");
        self.has_hash() && (self.legacy || self.sidecar(ENV_FILE_APPROVAL).exists())
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

/// Records under the names that mise 2026.11.14 and earlier used, which hash the path with
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
    fn brace_glob_matches_each_alternative() {
        let root = tempfile::tempdir().unwrap();
        let pattern = root.path().join("{vendor,third_party}/**/mise.toml");
        let matcher = IgnoredConfigPathMatcher::new(&[pattern]);

        assert!(matcher.is_match(&root.path().join("vendor/jj/mise.toml")));
        assert!(matcher.is_match(&root.path().join("third_party/jj/mise.toml")));
        assert!(!matcher.is_match(&root.path().join("src/jj/mise.toml")));
    }

    /// A project `.miserc.toml` supplies these patterns without trust, so one
    /// that globset cannot compile is kept as a literal path instead of taking
    /// the process down.
    #[test]
    fn glob_that_cannot_be_compiled_is_kept_as_a_literal() {
        let nested = |depth: usize| format!("/x/{}b{}", "{a,".repeat(depth), "}".repeat(depth));
        for pattern in [
            // past the regex parser's nesting limit
            nested(125),
            // deep enough to overflow the stack in globset's glob-to-regex pass
            nested(100_000),
            // 1000 levels again, each `{` followed by a `}` that closes nothing
            format!("/x/{}b{}", r"{\}".repeat(1000), "}".repeat(1000)),
            format!("/x/{}b{}", "{[}]".repeat(1000), "}".repeat(1000)),
            // past the size limit of the compiled regex
            format!("/x/{}", "?".repeat(2_000_000)),
        ] {
            let path = PathBuf::from(pattern);
            let matcher = IgnoredConfigPathMatcher::new(std::slice::from_ref(&path));
            assert!(matcher.globs.is_empty());
            assert_eq!(matcher.literals, [path]);
        }
    }

    #[test]
    fn path_glob_at_the_limits_still_compiles() {
        let nested = |depth: usize| format!("{}b{}", "{a,".repeat(depth), "}".repeat(depth));
        assert!(compile_path_glob(&nested(GLOB_MAX_BRACES)).is_ok());
        assert!(compile_path_glob(&nested(GLOB_MAX_BRACES + 1)).is_err());
        assert!(compile_path_glob(&"?".repeat(GLOB_MAX_LEN)).is_ok());
        assert!(compile_path_glob(&"?".repeat(GLOB_MAX_LEN + 1)).is_err());
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
    fn a_root_trusted_through_a_monorepo_root_this_command_trusted_is_trusted_by_it() {
        let dir = tempfile::tempdir().unwrap();
        let mono = dir.path().canonicalize().unwrap().join("mono");
        let child = mono.join("packages/app");
        std::fs::create_dir_all(&child).unwrap();
        granted_by_this_command(&mono);
        assert!(trusted_by_this_command(&mono));
        // Without the marker the monorepo root does not trust the child, so neither does this
        // command.
        assert!(!trusted_by_this_command(&child));
        let record = dirs::TRUSTED_CONFIGS.join(record_name(&mono));
        let marker = with_appended_extension(&record, "monorepo");
        file::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();
        assert!(trusted_by_this_command(&child));
        std::fs::remove_file(&marker).unwrap();
    }

    /// Tests trust every path unless trust is bound to content, so paranoid mode is what leaves
    /// the config untrusted before the write. Its trust root is then the file itself.
    #[test]
    fn trust_recorded_after_a_write_is_this_commands() {
        use confique::Layer;
        let _held = crate::test::SettingsGuard::lock();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.paranoid = Some(mise_settings::Paranoid::all());
        Settings::reset(Some(settings));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("mise.toml");
        std::fs::write(&path, "").unwrap();
        assert!(!is_trusted(&path));
        let root = config_trust_root(&path);
        assert_eq!(root, path);
        let trusted_before = trusted_before_write(&path);
        assert!(!trusted_before);
        std::fs::write(&path, "[env]\n").unwrap();
        trust_after_write(&path, trusted_before).unwrap();
        assert!(is_trusted(&root));
        assert!(trusted_by_this_command(&root));
        untrust(&root).unwrap();

        // A file trusted before the write keeps trust from before the command once the write
        // has changed it and renewed its record, so a second write may follow its links too.
        let renewed = dir.path().canonicalize().unwrap().join("renewed.toml");
        std::fs::write(&renewed, "").unwrap();
        trust(&renewed).unwrap();
        let trusted_before = trusted_before_write(&renewed);
        assert!(trusted_before);
        std::fs::write(&renewed, "[env]\n").unwrap();
        assert!(!is_trusted(&renewed));
        trust_after_write(&renewed, trusted_before).unwrap();
        assert!(is_trusted(&renewed));
        assert!(!trusted_by_this_command(&renewed));
        untrust(&renewed).unwrap();
        Settings::reset(None);
    }

    #[cfg(unix)]
    #[test]
    fn a_write_goes_to_the_destination_its_check_approved() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().canonicalize().unwrap().join("proj");
        std::fs::create_dir_all(project.join("config")).unwrap();
        std::fs::write(project.join("config/real.toml"), "").unwrap();
        symlink("config/real.toml", project.join("mise.toml")).unwrap();
        // A link that stays in the project: the write goes to the file it leads to, so a link
        // swapped in afterwards is not what it writes through.
        assert_eq!(
            checked_write_destination(&project.join("mise.toml")).unwrap(),
            project.join("config/real.toml")
        );
        // A regular file: its own path, in its directory resolved.
        std::fs::write(project.join("mise.local.toml"), "").unwrap();
        symlink(&project, dir.path().join("alias")).unwrap();
        assert_eq!(
            checked_write_destination(&dir.path().join("alias/mise.local.toml")).unwrap(),
            project.join("mise.local.toml")
        );
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

    /// Config trust bound to the content is not memoized either: the check after an edit
    /// compares the hash again.
    #[test]
    fn an_edited_config_needs_new_trust_even_after_a_passing_check() {
        let held = crate::test::SettingsGuard::lock();
        paranoid_on(&held);
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mise.toml");
        std::fs::write(&path, "[env]\nA = '1'\n").unwrap();

        trust(&path).unwrap();
        assert!(is_trusted(&path));
        assert!(is_trusted(&path));

        std::fs::write(&path, "[env]\nA = '2'\n").unwrap();
        assert!(!is_trusted(&path));
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

    /// mise 2026.11.14 and earlier wrote an env file approval as a record with a hash and no
    /// marker, just as it wrote paranoid trust in a config. Such a record approves an env file,
    /// never config, until the user trusts the path again.
    #[test]
    fn a_hashed_record_under_the_old_name_is_not_config_trust() {
        use confique::Layer;
        use mise_settings::{Paranoid, ParanoidCategory};
        let _held = crate::test::SettingsGuard::lock();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mise.toml");
        std::fs::write(&path, "[env]\nA = \"1\"\n").unwrap();
        let identity = record_identity(&path);
        let legacy =
            dirs::TRUSTED_CONFIGS.join(prefixed_record_name(&identity, hash_to_str(&identity)));
        file::create_dir_all(&*dirs::TRUSTED_CONFIGS).unwrap();
        file::make_symlink_or_file(&identity, &legacy).unwrap();

        // Without a hash the old record is trust by path, as before.
        assert!(!is_env_file_approval(&path));

        file::write(
            with_appended_extension(&legacy, "hash"),
            hash::file_hash_sha256(&path, None).unwrap(),
        )
        .unwrap();
        assert!(is_env_file_approval(&path));
        // Tests trust every config that has no env file approval, so this fails only on it.
        assert!(!is_trusted(&path));
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.paranoid = Some(Paranoid::only([ParanoidCategory::EnvFiles]));
        Settings::reset(Some(settings));
        assert!(env_file_trust_check(&path).unwrap());
        Settings::reset(None);

        // Trusting the path again replaces the old record with one under the current name.
        trust(&path).unwrap();
        assert!(!legacy.exists());
        assert!(!with_appended_extension(&legacy, "hash").exists());
        assert!(
            dirs::TRUSTED_CONFIGS
                .join(hashed_path_filename(&path))
                .exists()
        );
        assert!(!is_env_file_approval(&path));
        assert!(is_trusted(&path));
        untrust(&path).unwrap();
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

    /// A nested root of an ignored project, also one that arrives after the ignore, is not
    /// trusted through its own directory, which a trusted monorepo root above it (or, here, the
    /// test run) would trust.
    #[test]
    fn a_nested_root_of_an_ignored_project_is_not_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        let mono = tmp.path().canonicalize().unwrap().join("mono");
        let sub = mono.join("apps/sub");
        let late = sub.join(".mise/conf.d/late");
        std::fs::create_dir_all(&late).unwrap();
        std::fs::write(mono.join("mise.toml"), "monorepo_root = true\n").unwrap();
        std::fs::write(sub.join("mise.toml"), "").unwrap();
        trust(&mono).unwrap();
        file::write(with_appended_extension(&trust_path(&mono), "monorepo"), "").unwrap();
        add_ignored(sub.clone()).unwrap();
        let config = late.join("mise.toml");
        std::fs::write(&config, "").unwrap();

        assert!(is_trusted(&config_trust_root(&config)));
        assert!(!is_path_trusted(&config));

        rm_ignored(sub).unwrap();
        assert!(is_path_trusted(&config));
        untrust(&mono).unwrap();
    }

    #[test]
    fn a_monorepo_marker_under_the_old_name_trusts_the_subprojects() {
        let tmp = tempfile::tempdir().unwrap();
        let mono = tmp.path().canonicalize().unwrap().join("mono");
        let sub = mono.join("apps/sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(mono.join("mise.toml"), "monorepo_root = true\n").unwrap();
        std::fs::write(sub.join("mise.toml"), "[settings]\njobs = 3\n").unwrap();
        let config = sub.join("mise.toml");

        // What mise 2026.11.14 left behind: the root's record and marker under the old name,
        // and nothing under the current one. Settings are read before the root's config loads
        // and rewrites the marker, so the old marker has to count on its own.
        let identity = record_identity(&mono);
        let legacy =
            dirs::TRUSTED_CONFIGS.join(prefixed_record_name(&identity, hash_to_str(&identity)));
        file::create_dir_all(&*dirs::TRUSTED_CONFIGS).unwrap();
        file::make_symlink_or_file(&identity, &legacy).unwrap();
        file::write(with_appended_extension(&legacy, "monorepo"), "").unwrap();
        let current = with_appended_extension(
            &dirs::TRUSTED_CONFIGS.join(hashed_path_filename(&mono)),
            "monorepo",
        );
        assert!(!current.exists());

        // Tests trust every config without a marker, so the lookup is asserted directly.
        let canonical = config.canonicalize().unwrap();
        assert!(trusted_monorepo_ancestor(&canonical));
        assert!(is_trusted_with(&config, false));
        assert!(is_path_trusted(&config));

        // A root that stops being a monorepo loses the old-name marker too, so it no longer
        // trusts the subprojects.
        assert!(remove_monorepo_markers(&mono, &current).unwrap());
        assert!(!with_appended_extension(&legacy, "monorepo").exists());
        assert!(!trusted_monorepo_ancestor(&canonical));
        assert!(!remove_monorepo_markers(&mono, &current).unwrap());

        untrust(&mono).unwrap();
    }

    /// A project directory trusted as a whole, as a config linked into it is, looks for the
    /// monorepo root from its parent: a project right below the root is still in it.
    #[test]
    fn a_project_directory_right_below_a_monorepo_root_is_in_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mono = tmp.path().canonicalize().unwrap().join("mono");
        let app = mono.join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(mono.join("mise.toml"), "monorepo_root = true\n").unwrap();
        trust(&mono).unwrap();
        let marker =
            with_appended_extension(&dirs::TRUSTED_CONFIGS.join(record_name(&mono)), "monorepo");
        file::write(&marker, "").unwrap();

        assert!(trusted_monorepo_ancestor(&app));
        assert!(!trusted_monorepo_ancestor(&mono));
        // A file is looked up from above its own directory, the root it lies in.
        std::fs::write(mono.join("mise.local.toml"), "").unwrap();
        assert!(!trusted_monorepo_ancestor(&mono.join("mise.local.toml")));
        assert!(trusted_monorepo_ancestor(&app.join("mise.toml")));

        file::remove_file(&marker).unwrap();
        untrust(&mono).unwrap();
    }

    /// A task script is written where the check followed its links to, so a link that changes
    /// after the check cannot send the write elsewhere.
    #[cfg(unix)]
    #[test]
    fn a_task_script_write_goes_to_the_destination_the_check_approved() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().canonicalize().unwrap().join("project");
        std::fs::create_dir_all(project.join("real")).unwrap();
        std::os::unix::fs::symlink("real", project.join("mise-tasks")).unwrap();
        let destination =
            script_check_before_write(&project.join("mise-tasks/new"), &project).unwrap();
        assert_eq!(destination, project.join("real/new"));
    }

    /// `.config/mise` holding a `mise.toml` is a config root of its own, but a link at it is the
    /// project's, so it does not take the root, and a task written there, out of the project.
    #[cfg(unix)]
    #[test]
    fn a_task_root_that_is_itself_a_link_out_of_the_project_is_not_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let project = base.join("project");
        let outside = base.join("outside/mise");
        std::fs::create_dir_all(outside.join("tasks")).unwrap();
        std::fs::write(outside.join("mise.toml"), "").unwrap();
        std::fs::create_dir_all(project.join(".config")).unwrap();
        std::os::unix::fs::symlink(&outside, project.join(".config/mise")).unwrap();
        let root = project.join(".config/mise");
        assert_eq!(config_root::config_root(&root.join("mise.toml")), root);
        let err = script_check_before_write(&root.join("tasks/new"), &root).unwrap_err();
        assert!(
            format!("{err:#}").contains("through a linked directory"),
            "{err:#}"
        );
        assert!(script_check_before_remove(&root.join("tasks/new"), &root).is_err());
    }
}
