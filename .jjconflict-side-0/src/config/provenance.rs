use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{is_global_config, is_system_config};

/// The ownership layer of a loaded configuration file.
///
/// This remains attached to values that need to retain their source after
/// configuration layers are composed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConfigFileScope {
    System,
    User,
    Project,
}

impl ConfigFileScope {
    pub(crate) fn is_project(self) -> bool {
        self == Self::Project
    }
}

/// Identifies both the file and ownership layer from which configuration came.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ConfigProvenance {
    path: PathBuf,
    scope: ConfigFileScope,
}

impl ConfigProvenance {
    pub(crate) fn from_path(path: &Path) -> Self {
        let scope = if is_system_config(path) {
            ConfigFileScope::System
        } else if is_global_config(path) {
            ConfigFileScope::User
        } else {
            ConfigFileScope::Project
        };
        Self {
            path: path.to_path_buf(),
            scope,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn scope(&self) -> ConfigFileScope {
        self.scope
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env;

    #[test]
    fn classifies_standard_config_layers() {
        let system_path = env::MISE_SYSTEM_CONFIG_FILE.as_ref().unwrap();
        let user_path = env::MISE_GLOBAL_CONFIG_FILE.as_ref().unwrap();
        let system = ConfigProvenance::from_path(system_path);
        let user = ConfigProvenance::from_path(user_path);
        let project = ConfigProvenance::from_path(Path::new("/workspace/mise.toml"));

        assert_eq!(system.scope(), ConfigFileScope::System);
        assert_eq!(user.scope(), ConfigFileScope::User);
        assert_eq!(project.scope(), ConfigFileScope::Project);
        assert_eq!(system.path(), system_path);
    }
}
