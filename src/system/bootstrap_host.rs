//! Registers mise's config layer with the system modules: the loaded `Config`, config files, and
//! the few config-layer functions they call back into.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use eyre::Result;
use indexmap::IndexSet;
use mise_config::bootstrap::BootstrapTomlConfig;
use mise_config::dotfiles::{DotfileGroupsTomlConfig, DotfilesTomlConfig};
use mise_config::history::HistoryTomlConfig;
use mise_util::env_diff::EnvMap;

use crate::config::config_file::mise_toml::MiseToml;
use crate::config::config_file::{self, ConfigFile};
use crate::config::{self, Config, Settings, SettingsExt};
use crate::system::host::{self, BootstrapConfig, ConfigFileView, Host};

/// A loaded config file, seen through the narrow interface the system modules use.
#[derive(Debug)]
struct FileView(Arc<dyn ConfigFile>);

impl ConfigFileView for FileView {
    fn get_path(&self) -> &Path {
        self.0.get_path()
    }
    fn project_root(&self) -> Option<PathBuf> {
        self.0.project_root()
    }
    fn config_root(&self) -> PathBuf {
        self.0.config_root()
    }
    fn bootstrap_config(&self) -> Option<BootstrapTomlConfig> {
        self.0.bootstrap_config()
    }
    fn dotfiles_config(&self) -> Option<DotfilesTomlConfig> {
        self.0.dotfiles_config()
    }
    fn dotfile_groups_config(&self) -> Option<DotfileGroupsTomlConfig> {
        self.0.dotfile_groups_config()
    }
    fn history_config(&self) -> Option<HistoryTomlConfig> {
        self.0.history_config()
    }
}

fn view(file: Arc<dyn ConfigFile>) -> Arc<dyn ConfigFileView> {
    Arc::new(FileView(file))
}

pub fn view_map(map: &config::ConfigMap) -> host::ConfigMap {
    map.iter()
        .map(|(path, file)| (path.clone(), view(file.clone())))
        .collect()
}

#[async_trait(?Send)]
impl BootstrapConfig for Config {
    fn config_files(&self) -> host::ConfigMap {
        view_map(&self.config_files)
    }

    fn vars(&self) -> indexmap::IndexMap<String, String> {
        self.vars.clone()
    }

    fn bootstrap_config_maps(&self) -> Vec<host::ConfigMap> {
        Config::bootstrap_config_maps(self).map(view_map).collect()
    }

    fn selected_bootstrap_config_maps(&self) -> Vec<(PathBuf, host::ConfigMap)> {
        Config::selected_bootstrap_config_maps(self)
            .map(|(root, files)| (root.to_path_buf(), view_map(files)))
            .collect()
    }

    fn is_in_selected_bootstrap_root(&self, config_path: &Path) -> bool {
        Config::selected_bootstrap_config_maps(self)
            .any(|(_, files)| files.contains_key(config_path))
    }

    fn bootstrap_tera_ctx(&self, config_path: &Path) -> &tera::Context {
        Config::bootstrap_tera_ctx(self, config_path)
    }

    fn bootstrap_dry_run_unresolved_vars(&self, config_path: &Path) -> &IndexSet<String> {
        Config::bootstrap_dry_run_unresolved_vars(self, config_path)
    }

    fn add_redactions_excluding(
        &self,
        redactions: Vec<String>,
        env: &EnvMap,
        exclusions: &BTreeSet<String>,
    ) {
        Config::add_redactions_excluding(self, redactions, env, exclusions)
    }

    fn redact(&self, input: &str) -> String {
        Config::redact(self, input)
    }

    async fn get_tracked_config_files(&self) -> Result<host::ConfigMap> {
        Ok(view_map(&Config::get_tracked_config_files(self).await?))
    }

    fn without_environments(&self, environments: &[String]) -> Arc<dyn BootstrapConfig> {
        Config::without_environments(self, environments)
    }
}

pub fn register() {
    host::register(Host {
        config: || Box::pin(async { Ok(Config::get().await? as Arc<dyn BootstrapConfig>) }),
        reload_config: || {
            Box::pin(async { Ok(Config::reset().await? as Arc<dyn BootstrapConfig>) })
        },
        loaded_config: || config::is_loaded().then(|| Config::get_() as Arc<dyn BootstrapConfig>),
        environments_for_config_path: config::environments_for_config_path,
        config_root: config_file::config_root::config_root,
        is_global_config: config::is_global_config,
        is_trusted: config_file::is_trusted,
        system_config_files: config::system_config_files,
        global_config_files: config::global_config_files,
        global_shared_config_path: config::global_shared_config_path,
        config_files_with_incoming: config::config_files_with_incoming,
        safe_mode_ignores_bootstrap: config::safe_mode_ignores_bootstrap,
        describe_command_setting: |path| {
            Ok(<Settings as SettingsExt>::parse_settings_file(path)?
                .history
                .describe_command)
        },
        mise_env_with_auto: || &crate::env::MISE_ENV_WITH_AUTO,
        verify_release_signature: |data, signature| {
            crate::minisign::verify(&crate::minisign::MISE_PUB_KEY, data, signature)
        },
        parse_config_file: |path| Ok(view(Arc::new(MiseToml::from_file(path)?))),
        parse_config_body: |body, path| {
            Ok(view(Arc::new(MiseToml::for_history_preflight(body, path)?)))
        },
    });
}
