use std::path::{Path, PathBuf};

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};
use eyre::Result;

use super::ProviderBase;

/// Deps provider for user-defined custom rules from mise.toml [deps.*]
#[derive(Debug)]
pub struct CustomDepsProvider {
    base: ProviderBase,
}

impl CustomDepsProvider {
    pub fn new(id: String, config: DepsProviderConfig, project_root: &Path) -> Self {
        Self {
            base: ProviderBase::new(id, project_root, config),
        }
    }
}

impl DepsProvider for CustomDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        self.base.sources(vec![])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base.outputs(vec![])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        let run = self
            .base
            .config
            .run
            .as_ref()
            .ok_or_else(|| eyre::eyre!("deps rule {} has no run command", self.base.id))?;

        DepsCommand::from_string(run, &self.base.project_root, &self.base.config)
    }

    fn applicability(&self) -> DepsProviderApplicability {
        DepsProviderApplicability::require_run(self.base.config.run.as_deref())
    }
}
