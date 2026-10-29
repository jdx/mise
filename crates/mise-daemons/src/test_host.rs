//! The unit tests below mise have no config system, so config files are parsed straight from
//! their `[daemons]`, `[daemons_settings]`, `[daemon_groups]`, and `[daemon_providers]` tables.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use eyre::Result;
use indexmap::IndexMap;
use serde::Deserialize;

use crate::host::{self, ConfigFileView, Host};
use crate::{DaemonSettings, Declaration, GroupDeclaration};

mise_util::run_before_main!(init);

fn init() {
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test");
    std::fs::create_dir_all(&home).unwrap();
    mise_util::env::set_var("HOME", &home);
    mise_util::testing::enable(home);
    // Daemons are experimental; mise's own test configuration turns that on.
    mise_settings::set_loader(|| {
        let mut settings = mise_settings::load_defaults()?;
        std::sync::Arc::make_mut(&mut settings).experimental = true;
        Ok(settings)
    });
    mise_util::cache::set_base_cache_keys(vec!["mise-daemons-tests".to_string()]);
    mise_util::user_agent::set("mise-daemons-tests".to_string());
    mise_util::deprecation::set_version(env!("CARGO_PKG_VERSION"));
    mise_util::env::set_mise_env(|| &[]);
    mise_util::shells::set_implicit_inline_shell(|| true);
    host::register(Host {
        is_global_config: |path| path == global_config_path(),
        config_dir_is_ignored: |_, _| false,
        config_path_is_ignored: |_, _| false,
        config_paths_in_dir: |dir| {
            ["mise.toml", ".mise.toml", "config.toml"]
                .iter()
                .map(|name| dir.join(name))
                .filter(|path| path.is_file())
                .collect()
        },
        is_path_trusted,
        config_trust_root: |path| path.parent().unwrap_or(path).to_path_buf(),
        config_root,
        global_config_path: || {
            mise_util::dirs::HOME
                .join(".config")
                .join("mise")
                .join("config.toml")
        },
        parse_config_file: |path| {
            let body = mise_util::file::read_to_string(path)?;
            Ok(Arc::new(DeclaredFile::parse(&body, path)?))
        },
        ceiling_paths: || {
            static NONE: LazyLock<HashSet<PathBuf>> = LazyLock::new(HashSet::new);
            &NONE
        },
        safe_mode: || false,
        ensure_not_safe: |_| Ok(()),
        global_runtime: |_| Box::pin(async { eyre::bail!("no toolsets in these tests") }),
        install_provider_tools: |_, _, _| {
            Box::pin(async { eyre::bail!("no toolsets in these tests") })
        },
        provider_tool_paths: |_, _, _| {
            Box::pin(async { eyre::bail!("no toolsets in these tests") })
        },
    });
}

/// Serializes the tests that import another project, which share the trust list.
pub(crate) static IMPORT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

static TRUSTED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Trust `dir` and everything under it, as `mise trust` does.
pub(crate) fn trust(dir: &Path) {
    TRUSTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(mise_util::file::desymlink_path(dir));
}

fn is_path_trusted(path: &Path) -> bool {
    TRUSTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .any(|trusted| mise_util::file::desymlink_path(path).starts_with(trusted))
}

/// The global config file: its root is the home directory, so it has no project.
fn global_config_path() -> PathBuf {
    mise_util::dirs::HOME
        .join(".config")
        .join("mise")
        .join("config.toml")
}

fn config_root(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new(".")).to_path_buf()
}

#[derive(Debug, Default, Deserialize)]
struct Tables {
    #[serde(default)]
    daemons: IndexMap<String, Declaration>,
    daemons_settings: Option<DaemonSettings>,
    #[serde(default)]
    daemon_groups: IndexMap<String, GroupDeclaration>,
    #[serde(default)]
    daemon_providers: IndexMap<String, toml::Table>,
}

#[derive(Debug)]
pub struct DeclaredFile {
    path: PathBuf,
    tables: Tables,
}

impl DeclaredFile {
    pub(crate) fn parse(body: &str, path: &Path) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            tables: toml::from_str(body)?,
        })
    }
}

impl ConfigFileView for DeclaredFile {
    fn get_path(&self) -> &Path {
        &self.path
    }
    fn project_root(&self) -> Option<PathBuf> {
        (self.path != global_config_path()).then(|| config_root(&self.path))
    }
    fn config_root(&self) -> PathBuf {
        config_root(&self.path)
    }
    fn daemon_declarations(&self) -> IndexMap<String, Declaration> {
        self.tables.daemons.clone()
    }
    fn daemon_settings(&self) -> Option<DaemonSettings> {
        self.tables.daemons_settings.clone()
    }
    fn daemon_group_declarations(&self) -> IndexMap<String, GroupDeclaration> {
        self.tables.daemon_groups.clone()
    }
    fn daemon_providers(&self) -> IndexMap<String, toml::Table> {
        self.tables.daemon_providers.clone()
    }
}
