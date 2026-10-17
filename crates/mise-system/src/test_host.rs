//! The unit tests below mise have no config system, so they run against a small stand-in for the
//! host: config files are parsed straight from their `[bootstrap]`, `[dotfiles]`,
//! `[dotfile_groups]`, and `[history]` tables, and there is no global or system configuration.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use eyre::Result;
use indexmap::IndexSet;
use mise_config::bootstrap::BootstrapTomlConfig;
use mise_config::dotfiles::{DotfileGroupsTomlConfig, DotfilesTomlConfig};
use mise_config::history::HistoryTomlConfig;
use mise_util::env_diff::EnvMap;
use serde::Deserialize;

use crate::host::{self, BootstrapConfig, ConfigFileView, ConfigMap, Host};

mise_util::run_before_main!(init);

fn init() {
    // Under the crate, not `/tmp`: the tests need ancestors that are not world-writable.
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test");
    std::fs::create_dir_all(&home).unwrap();
    mise_util::env::set_var("HOME", &home);
    mise_util::testing::enable(home);
    mise_settings::set_loader(mise_settings::load_defaults);
    mise_util::cache::set_base_cache_keys(vec!["mise-system-tests".to_string()]);
    mise_util::user_agent::set("mise-system-tests".to_string());
    mise_util::deprecation::set_version(env!("CARGO_PKG_VERSION"));
    mise_util::env::set_mise_env(|| &[]);
    mise_util::shells::set_implicit_inline_shell(|| true);
    host::register(Host {
        config: || {
            Box::pin(async { Ok(Arc::new(TestConfig::default()) as Arc<dyn BootstrapConfig>) })
        },
        reload_config: || {
            Box::pin(async { Ok(Arc::new(TestConfig::default()) as Arc<dyn BootstrapConfig>) })
        },
        loaded_config: || None,
        environments_for_config_path,
        config_root,
        is_global_config: |_| false,
        is_trusted: |_| true,
        system_config_files: IndexSet::new,
        global_config_files: IndexSet::new,
        global_shared_config_path: || {
            mise_util::dirs::HOME
                .join(".config")
                .join("mise")
                .join("config.toml")
        },
        config_files_with_incoming: |_, _| IndexSet::new(),
        safe_mode_ignores_bootstrap: |_| false,
        describe_command_setting: |_| Ok(None),
        mise_env_with_auto: || &[],
        verify_release_signature,
        parse_config_file: |path| {
            let body = mise_util::file::read_to_string(path)?;
            Ok(Arc::new(DeclaredFile::parse(&body, path)?))
        },
        parse_config_body: |body, path| Ok(Arc::new(DeclaredFile::parse(body, path)?)),
    });
}

/// mise's release key as of v2026.8.1, which signed the manifest the remote tests use.
const RELEASE_KEY: &str = "RWTC3g8W3z4RZK3V3qv7fa1QY4JEWyBtqIHW+85QlJpZc5yG+uNYNBSZ";

fn verify_release_signature(data: &[u8], signature: &str) -> Result<()> {
    let key = minisign_verify::PublicKey::from_base64(RELEASE_KEY)?;
    let signature = minisign_verify::Signature::decode(signature)?;
    key.verify(data, &signature, false)?;
    Ok(())
}

/// `config.<env>.toml` and `mise.<env>.toml` declare for `<env>`; every other name is unscoped.
fn environments_for_config_path(path: &Path) -> Vec<String> {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return vec![];
    };
    stem.split_once('.')
        .map(|(_, environment)| vec![environment.to_string()])
        .unwrap_or_default()
}

fn config_root(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new("."));
    let name = parent.file_name().and_then(|name| name.to_str());
    if name == Some(".mise") {
        return parent.parent().unwrap_or(parent).to_path_buf();
    }
    parent.to_path_buf()
}

#[derive(Debug, Default, Deserialize)]
struct Declarations {
    bootstrap: Option<BootstrapTomlConfig>,
    dotfiles: Option<DotfilesTomlConfig>,
    dotfile_groups: Option<DotfileGroupsTomlConfig>,
    history: Option<HistoryTomlConfig>,
}

#[derive(Debug)]
struct DeclaredFile {
    path: PathBuf,
    declarations: Declarations,
}

impl DeclaredFile {
    fn parse(body: &str, path: &Path) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            declarations: toml::from_str(body)?,
        })
    }
}

impl ConfigFileView for DeclaredFile {
    fn get_path(&self) -> &Path {
        &self.path
    }
    fn project_root(&self) -> Option<PathBuf> {
        None
    }
    fn config_root(&self) -> PathBuf {
        config_root(&self.path)
    }
    fn bootstrap_config(&self) -> Option<BootstrapTomlConfig> {
        self.declarations.bootstrap.clone()
    }
    fn dotfiles_config(&self) -> Option<DotfilesTomlConfig> {
        self.declarations.dotfiles.clone()
    }
    fn dotfile_groups_config(&self) -> Option<DotfileGroupsTomlConfig> {
        self.declarations.dotfile_groups.clone()
    }
    fn history_config(&self) -> Option<HistoryTomlConfig> {
        self.declarations.history.clone()
    }
}

#[derive(Default)]
struct TestConfig {
    config_files: ConfigMap,
}

static EMPTY_CONTEXT: LazyLock<tera::Context> = LazyLock::new(tera::Context::new);
static NO_VARS: LazyLock<IndexSet<String>> = LazyLock::new(IndexSet::new);

#[async_trait(?Send)]
impl BootstrapConfig for TestConfig {
    fn config_files(&self) -> ConfigMap {
        self.config_files.clone()
    }
    fn vars(&self) -> indexmap::IndexMap<String, String> {
        Default::default()
    }
    fn bootstrap_config_maps(&self) -> Vec<ConfigMap> {
        vec![self.config_files.clone()]
    }
    fn selected_bootstrap_config_maps(&self) -> Vec<(PathBuf, ConfigMap)> {
        vec![]
    }
    fn is_in_selected_bootstrap_root(&self, _: &Path) -> bool {
        false
    }
    fn bootstrap_tera_ctx(&self, _: &Path) -> &tera::Context {
        &EMPTY_CONTEXT
    }
    fn bootstrap_dry_run_unresolved_vars(&self, _: &Path) -> &IndexSet<String> {
        &NO_VARS
    }
    fn add_redactions_excluding(&self, _: Vec<String>, _: &EnvMap, _: &BTreeSet<String>) {}
    fn redact(&self, input: &str) -> String {
        input.to_string()
    }
    async fn get_tracked_config_files(&self) -> Result<ConfigMap> {
        Ok(ConfigMap::default())
    }
    fn without_environments(&self, _: &[String]) -> Arc<dyn BootstrapConfig> {
        Arc::new(TestConfig {
            config_files: self.config_files.clone(),
        })
    }
}
