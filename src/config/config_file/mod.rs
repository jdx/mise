use std::ffi::OsStr;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use eyre::{Result, eyre};
use idiomatic_version::IdiomaticVersionFile;
use path_absolutize::Absolutize;
use serde_derive::Deserialize;
use std::sync::LazyLock as Lazy;
use tool_versions::ToolVersions;
use versions::Versioning;
use xx::regex;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::env_directive::EnvDirective;
use crate::config::{AliasMap, Settings, is_global_config, settings};
use crate::errors::Error::UntrustedConfig;
use crate::file::display_path;
use crate::hash::hash_to_str;
use crate::hooks::Hook;
use crate::redactions::Redactions;
use crate::task::Task;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource, ToolVersionList, Toolset};
use crate::ui::{prompt, style};
use crate::watch_files::WatchFile;
use crate::{backend, config, dirs, env, file, hash};

use super::Config;

pub mod idiomatic_version;
pub mod mise_toml;
pub mod toml;
pub mod tool_versions;

#[derive(Debug, PartialEq)]
pub enum ConfigFileType {
    MiseToml,
    ToolVersions,
    IdiomaticVersion,
}

pub trait ConfigFile: Debug + Send + Sync {
    fn get_path(&self) -> &Path;
    fn min_version(&self) -> &Option<Versioning> {
        &None
    }
    /// gets the project directory for the config
    /// if it's a global/system config, returns None
    /// files like ~/src/foo/.mise/config.toml will return ~/src/foo
    /// and ~/src/foo/.mise.config.toml will return None
    fn project_root(&self) -> Option<&Path> {
        let p = self.get_path();
        if config::is_global_config(p) {
            return None;
        }
        match p.parent() {
            Some(dir) => match dir {
                dir if dir.starts_with(*dirs::CONFIG) => None,
                dir if dir.starts_with(*dirs::SYSTEM) => None,
                dir if dir == *dirs::HOME => None,
                dir => Some(dir),
            },
            None => None,
        }
    }
    fn config_type(&self) -> ConfigFileType;
    fn config_root(&self) -> PathBuf {
        config_root(self.get_path())
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

    fn task_config(&self) -> &TaskConfig {
        static DEFAULT_TASK_CONFIG: Lazy<TaskConfig> = Lazy::new(TaskConfig::default);
        &DEFAULT_TASK_CONFIG
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

fn init(path: &Path) -> Arc<dyn ConfigFile> {
    match detect_config_file_type(path) {
        Some(ConfigFileType::MiseToml) => Arc::new(MiseToml::init(path)),
        Some(ConfigFileType::ToolVersions) => Arc::new(ToolVersions::init(path)),
        Some(ConfigFileType::IdiomaticVersion) => {
            Arc::new(IdiomaticVersionFile::init(path.to_path_buf()))
        }
        _ => panic!("Unknown config file type: {}", path.display()),
    }
}

pub fn parse_or_init(path: &Path) -> eyre::Result<Arc<dyn ConfigFile>> {
    let path = if path.is_dir() {
        path.join("mise.toml")
    } else {
        path.into()
    };
    let cf = match path.exists() {
        true => parse(&path)?,
        false => init(&path),
    };
    Ok(cf)
}

pub fn parse(path: &Path) -> Result<Arc<dyn ConfigFile>> {
    if let Ok(settings) = Settings::try_get() {
        if settings.paranoid {
            trust_check(path)?;
        }
    }
    match detect_config_file_type(path) {
        Some(ConfigFileType::MiseToml) => Ok(Arc::new(MiseToml::from_file(path)?)),
        Some(ConfigFileType::ToolVersions) => Ok(Arc::new(ToolVersions::from_file(path)?)),
        Some(ConfigFileType::IdiomaticVersion) => {
            Ok(Arc::new(IdiomaticVersionFile::from_file(path)?))
        }
        #[allow(clippy::box_default)]
        _ => Ok(Arc::new(MiseToml::default())),
    }
}

pub fn config_root(path: &Path) -> PathBuf {
    if is_global_config(path) {
        return env::MISE_GLOBAL_CONFIG_ROOT.to_path_buf();
    }
    let path = path
        .absolutize()
        .map(|p| p.to_path_buf())
        .unwrap_or(path.to_path_buf());
    let parts = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    const EMPTY: &str = "";
    let filename = parts.last().map(|p| p.as_str()).unwrap_or(EMPTY);
    let parent = parts
        .iter()
        .nth_back(1)
        .map(|p| p.as_str())
        .unwrap_or(EMPTY);
    let grandparent = parts
        .iter()
        .nth_back(2)
        .map(|p| p.as_str())
        .unwrap_or(EMPTY);
    let great_grandparent = parts
        .iter()
        .nth_back(3)
        .map(|p| p.as_str())
        .unwrap_or(EMPTY);
    let parent_path = || path.parent().unwrap().to_path_buf();
    let grandparent_path = || parent_path().parent().unwrap().to_path_buf();
    let great_grandparent_path = || grandparent_path().parent().unwrap().to_path_buf();
    let great_great_grandparent_path = || great_grandparent_path().parent().unwrap().to_path_buf();
    let is_mise_dir = |d: &str| d == "mise" || d == ".mise";
    let is_config_filename = |f: &str| {
        f == "config.toml" || f == "config.local.toml" || regex!(r"config\..+\.toml").is_match(f)
    };
    if parent == "conf.d" && is_mise_dir(grandparent) {
        if great_grandparent == ".config" {
            great_great_grandparent_path()
        } else {
            great_grandparent_path()
        }
    } else if is_mise_dir(parent) && is_config_filename(filename) {
        if grandparent == ".config" {
            great_grandparent_path()
        } else {
            grandparent_path()
        }
    } else if parent == ".config" {
        grandparent_path()
    } else {
        parent_path()
    }
}

pub fn config_trust_root(path: &Path) -> PathBuf {
    if settings::is_loaded() && Settings::get().paranoid {
        path.to_path_buf()
    } else {
        config_root(path)
    }
}

pub fn trust_check(path: &Path) -> eyre::Result<()> {
    static MUTEX: Mutex<()> = Mutex::new(());
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple checks at once so we don't prompt multiple times for the same path
    let config_root = config_trust_root(path);
    let default_cmd = String::new();
    let args = env::ARGS.read().unwrap();
    let cmd = args.get(1).unwrap_or(&default_cmd).as_str();
    if is_trusted(&config_root) || is_trusted(path) || cmd == "trust" || cfg!(test) {
        return Ok(());
    }
    if cmd != "hook-env" && !is_ignored(&config_root) && !is_ignored(path) {
        let ans = prompt::confirm_with_all(format!(
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
    if is_ignored(canonicalized_path.as_path()) {
        return false;
    }
    if IS_TRUSTED
        .lock()
        .unwrap()
        .contains(canonicalized_path.as_path())
    {
        return true;
    }
    if config::is_global_config(path) {
        add_trusted(canonicalized_path.to_path_buf());
        return true;
    }
    let settings = Settings::get();
    for p in settings.trusted_config_paths() {
        if canonicalized_path.starts_with(p) {
            add_trusted(canonicalized_path.to_path_buf());
            return true;
        }
    }
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
        // the file isn't trusted, and we're not on a CI system where we generally assume we can
        // trust config files
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
pub fn is_ignored(path: &Path) -> bool {
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
    if let Ok(path) = path.canonicalize() {
        env::MISE_IGNORED_CONFIG_PATHS
            .iter()
            .any(|p| path.starts_with(p))
            || IS_IGNORED.lock().unwrap().contains(&path)
    } else {
        debug!("is_ignored: path canonicalize failed");
        true
    }
}

pub fn trust(path: &Path) -> Result<()> {
    rm_ignored(path.to_path_buf())?;
    let hashed_path = trust_path(path);
    if !hashed_path.exists() {
        file::create_dir_all(hashed_path.parent().unwrap())?;
        file::make_symlink_or_file(path.canonicalize()?.as_path(), &hashed_path)?;
    }
    if Settings::get().paranoid {
        let trust_hash_path = hashed_path.with_extension("hash");
        let hash = hash::file_hash_sha256(path, None)?;
        file::write(trust_hash_path, hash)?;
    }
    Ok(())
}

pub fn untrust(path: &Path) -> eyre::Result<()> {
    rm_ignored(path.to_path_buf())?;
    let hashed_path = trust_path(path);
    if hashed_path.exists() {
        file::remove_file(hashed_path)?;
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
    let trust_hash_path = trust_path.with_extension("hash");
    if !trust_hash_path.exists() {
        return Ok(false);
    }
    let hash = file::read_to_string(&trust_hash_path)?;
    let actual = hash::file_hash_sha256(path, None)?;
    Ok(hash == actual)
}

fn detect_config_file_type(path: &Path) -> Option<ConfigFileType> {
    match path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("mise.toml")
    {
        f if backend::list()
            .iter()
            .any(|b| match b.idiomatic_filenames() {
                Ok(filenames) => filenames.contains(&f.to_string()),
                Err(e) => {
                    debug!("idiomatic_filenames failed for {}: {:?}", b, e);
                    false
                }
            }) =>
        {
            Some(ConfigFileType::IdiomaticVersion)
        }
        f if env::MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES
            .as_ref()
            .is_some_and(|o| o.contains(f)) =>
        {
            Some(ConfigFileType::ToolVersions)
        }
        f if env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str() == f => {
            Some(ConfigFileType::ToolVersions)
        }
        f if f.ends_with(".toml") => Some(ConfigFileType::MiseToml),
        f if env::MISE_OVERRIDE_CONFIG_FILENAMES.contains(f) => Some(ConfigFileType::MiseToml),
        f if env::MISE_DEFAULT_CONFIG_FILENAME.as_str() == f => Some(ConfigFileType::MiseToml),
        _ => None,
    }
}

impl Display for dyn ConfigFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let toolset = self.to_toolset().unwrap().to_string();
        write!(f, "{}: {toolset}", &display_path(self.get_path()))
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
pub struct TaskConfig {
    pub includes: Option<Vec<PathBuf>>,
    pub dir: Option<String>,
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_detect_config_file_type() {
        env::set_var("MISE_EXPERIMENTAL", "true");
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.nvmrc")),
            Some(ConfigFileType::IdiomaticVersion)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.ruby-version")),
            Some(ConfigFileType::IdiomaticVersion)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.test-tool-versions")),
            Some(ConfigFileType::ToolVersions)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/mise.toml")),
            Some(ConfigFileType::MiseToml)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/rust-toolchain.toml")),
            Some(ConfigFileType::IdiomaticVersion)
        );
    }

    #[test]
    fn test_config_root() {
        for p in &[
            "/foo/bar/.config/mise/conf.d/config.toml",
            "/foo/bar/.config/mise/conf.d/foo.toml",
            "/foo/bar/.config/mise/config.local.toml",
            "/foo/bar/.config/mise/config.toml",
            "/foo/bar/.config/mise.local.toml",
            "/foo/bar/.config/mise.toml",
            "/foo/bar/.mise.env.toml",
            "/foo/bar/.mise.local.toml",
            "/foo/bar/.mise.toml",
            "/foo/bar/.mise/conf.d/config.toml",
            "/foo/bar/.mise/config.local.toml",
            "/foo/bar/.mise/config.toml",
            "/foo/bar/.tool-versions",
            "/foo/bar/mise.env.toml",
            "/foo/bar/mise.local.toml",
            "/foo/bar/mise.toml",
            "/foo/bar/mise/config.local.toml",
            "/foo/bar/mise/config.toml",
            "/foo/bar/.config/mise/config.env.toml",
            "/foo/bar/.config/mise.env.toml",
            "/foo/bar/.mise/config.env.toml",
            "/foo/bar/.mise.env.toml",
        ] {
            println!("{p}");
            assert_eq!(config_root(Path::new(p)), PathBuf::from("/foo/bar"));
        }
    }
}
